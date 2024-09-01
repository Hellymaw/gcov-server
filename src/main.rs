use axum::{
    extract::{Json, Path},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde::Serialize;
use tera::Tera;
use tower_http::trace::TraceLayer;
use tracing;
use tracing_appender;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const MAX_LOG_FILES: usize = 48;

struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong: {}", self.0),
        )
            .into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

#[tokio::main]
async fn main() {
    let log_dir = std::env::var("LOG_DIR").unwrap_or("./logs".to_string());
    let log_suffix = std::env::var("LOG_SUFFIX").unwrap_or("log".to_string());

    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::HOURLY)
        .filename_suffix(&log_suffix)
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)
        .expect("Failed to initialise rolling file appender");

    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::fmt::layer().with_writer(non_blocking))
        .init();

    let app = Router::new()
        .route("/:org/:repo/summary", post(summary_handler))
        .route("/", get(root_handler))
        .layer(TraceLayer::new_for_http());

    let bind_addr = std::env::var("BIND_ADDRESS").unwrap_or("0.0.0.0:1001".to_string());
    let listener = tokio::net::TcpListener::bind(bind_addr).await.unwrap();

    axum::serve(listener, app).await.unwrap();
}

#[derive(Serialize)]
struct GiteaRepo {
    name: String,
    coverage: f64,
}

#[derive(Serialize)]
struct GiteaOrg {
    name: String,
    repos: Vec<GiteaRepo>,
}

async fn root_handler() -> Result<Html<String>, AppError> {
    let tera = Tera::new("templates/**/*").unwrap();

    let orgs = vec![GiteaOrg {
        name: "test".to_string(),
        repos: vec![GiteaRepo {
            name: "a".to_string(),
            coverage: 55.5,
        }],
    }];

    let mut context = tera::Context::new();
    context.insert("orgs", &orgs);

    let output = tera.render("base.html", &context)?;

    Ok(Html::from(output))
}

async fn summary_handler(
    Path((org, repo)): Path<(String, String)>,
    Json(payload): Json<serde_json::Value>,
) -> Result<(), AppError> {
    tracing::debug!(%payload);

    let mut path = std::path::PathBuf::new();
    path.push("./coverage");
    path.push(&org);
    path.push(&repo);

    std::fs::create_dir_all(path)?;

    Ok(())
}
