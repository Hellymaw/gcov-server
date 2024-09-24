use axum::{
    extract::{Json, Path},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Extension, Router,
};
use lazy_static::lazy_static;
use serde::Serialize;
use sqlx::postgres::PgPool;
use std::{collections::HashMap, vec};
use tera::Tera;
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing;
use tracing_appender;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod db;
use db::summary::{CoverageSummary, SummaryTableEntry};

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

#[derive(Serialize, Debug)]
struct GiteaOrg {
    name: String,
    repos: Vec<SummaryTableEntry>,
}

lazy_static! {
    static ref TEMPLATES: Tera = {
        let tera = match Tera::new("templates/**/*") {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Parsing error(s): {}", e);
                ::std::process::exit(1);
            }
        };
        tera
    };
}

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
        .route("/report/orgs", get(report_orgs_handler))
        .route("/:org/:repo/summary", post(summary_handler))
        .route("/summary", get(root_summary_handler))
        .layer(Extension(db_pool))
        .nest_service("/reports", tower_http::services::ServeDir::new("reports"))
        .fallback_service(
            ServeDir::new("assets").not_found_service(ServeFile::new("assets/index.html")),
        )
        .layer(TraceLayer::new_for_http());

    let bind_addr = std::env::var("BIND_ADDRESS").unwrap_or("0.0.0.0:1001".to_string());
    let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Error binding to {}: {}", bind_addr, e);
            ::std::process::exit(2);
        }
    };

    axum::serve(listener, app).await.unwrap();
}

#[derive(Serialize, Debug)]
struct OrgList {
    orgs: Vec<String>,
}

impl OrgList {
    fn new() -> Self {
        OrgList { orgs: Vec::new() }
    }
}

async fn report_orgs_handler(_db: Extension<PgPool>) -> Result<Json<OrgList>, AppError> {
    let mut reponse = OrgList::new();

    let mut dir = tokio::fs::read_dir("reports").await?;
    while let Some(entry) = dir.next_entry().await? {
        let file_type = entry.file_type().await?;
        if file_type.is_dir() {
            if let Ok(file_name) = entry.file_name().into_string() {
                reponse.orgs.push(file_name);
            }
        }
    }

    Ok(Json(reponse))
}

async fn root_summary_handler(db: Extension<PgPool>) -> Result<Html<String>, AppError> {
    let orgs = if let Ok(resp) = db::summary::fetch_table(&*db).await {
        let mut orgs: HashMap<String, Vec<SummaryTableEntry>> = HashMap::new();
        for entry in resp {
            if let Some(vals) = orgs.get_mut(&entry.org) {
                vals.push(entry);
            } else {
                orgs.insert(entry.org.clone(), vec![entry]);
            }
        }

        let orgs: Vec<GiteaOrg> = orgs
            .into_iter()
            .map(|(k, v)| GiteaOrg { name: k, repos: v })
            .collect();

        tracing::error!("{:?}", orgs);

        orgs
    } else {
        Vec::new()
    };

    let mut context = tera::Context::new();
    context.insert("orgs", &orgs);

    let output = TEMPLATES.render("base.html", &context)?;

    Ok(Html::from(output))
}

async fn summary_handler(
    db: Extension<PgPool>,
    Path((org, repo)): Path<(String, String)>,
    Json(payload): Json<CoverageSummary>,
) -> Result<(), AppError> {
    db::summary::insert_into_table(&*db, &org, &repo, &payload)
        .await
        .map_err(|e| e.into())
}
