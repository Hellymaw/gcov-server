use axum::{routing::get, Extension, Router};
use tower_http::trace::TraceLayer;
use tracing_appender;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod app;
mod db;
mod gcovr;
mod gitea;

const MAX_LOG_FILES: usize = 48;

fn configure_logging() -> Result<(), tracing_appender::rolling::InitError> {
    let log_dir = std::env::var("LOG_DIR").unwrap_or("./logs".to_string());
    let log_suffix = std::env::var("LOG_SUFFIX").unwrap_or("log".to_string());

    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::HOURLY)
        .filename_suffix(&log_suffix)
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)?;

    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::fmt::layer().with_writer(non_blocking))
        .init();

    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(e) = configure_logging() {
        eprintln!("Error occurred setting up logging: {}", e);
        ::std::process::exit(1);
    }

    let db_pool = match db::connect_and_setup().await {
        Ok(db) => db,
        Err(e) => {
            eprintln!("Error occurred setting up database: {}", e);
            ::std::process::exit(3);
        }
    };

    let app = Router::new()
        .route("/", get(app::root_handler))
        .route("/:owner", get(app::owner_handler))
        .route("/:owner/:repo", get(app::repo_handler))
        .route("/:owner/:repo/tree/*path", get(app::tree_handler))
        .route("/:owner/:repo/blob/*path", get(app::blob_handler))
        .route("/:org/:repo/summaries", get(app::repo_summaries_handler))
        .route("/summaries", get(app::root_summary_handler))
        .route(
            "/report/:owner/:repo/:commit/*path",
            get(app::test_ingest_report).post(app::ingest_report),
        )
        .layer(Extension(db_pool))
        .layer(TraceLayer::new_for_http());

    let bind_addr = std::env::var("BIND_ADDRESS").unwrap_or("0.0.0.0:3003".to_string());
    let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Error binding to {}: {}", bind_addr, e);
            ::std::process::exit(2);
        }
    };

    axum::serve(listener, app).await.unwrap();
}
