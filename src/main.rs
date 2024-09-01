use anyhow::anyhow;
use axum::{
    extract::{Json, Path},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Extension, Router,
};
use serde::{ser::SerializeStruct, Deserialize, Serialize};
use sqlx::postgres::PgPool;
use std::collections::HashMap;
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

serde_with::with_prefix!(prefix_branch "branch_");
serde_with::with_prefix!(prefix_function "function_");
serde_with::with_prefix!(prefix_line "line_");

#[derive(Serialize, Deserialize)]
struct Coverage {
    covered: usize,
    total: usize,
    percent: f64,
}

#[derive(Serialize, Deserialize)]
struct CoverageSummary {
    #[serde(flatten, with = "prefix_branch")]
    branch: Coverage,

    #[serde(flatten, with = "prefix_function")]
    function: Coverage,

    #[serde(flatten, with = "prefix_line")]
    line: Coverage,
}

#[derive(sqlx::FromRow, Debug)]
struct SummaryTableEntry {
    #[sqlx(rename = "inserttime")]
    insert_time: sqlx::types::chrono::NaiveDateTime,
    // insert_time: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    org: String,
    repo: String,
    coverage: sqlx::types::JsonValue,
}

impl Serialize for SummaryTableEntry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("SummaryTableEntry", 4)?;

        state.serialize_field("inserttime", &self.insert_time.and_utc().timestamp())?;
        state.serialize_field("org", &self.org)?;
        state.serialize_field("repo", &self.repo)?;
        state.serialize_field("coverage", &self.coverage)?;

        state.end()
    }
}

#[derive(Serialize, Debug)]
struct GiteaOrg {
    name: String,
    repos: Vec<SummaryTableEntry>,
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

    let db_pool = PgPool::connect(&construct_db_connection_string())
        .await
        .expect("If the DB isn't active, we're dead in the water");

    let app = Router::new()
        .route("/:org/:repo/summary", post(summary_handler))
        .route("/", get(root_handler))
        .layer(Extension(db_pool))
        .layer(TraceLayer::new_for_http());

    let bind_addr = std::env::var("BIND_ADDRESS").unwrap_or("0.0.0.0:1001".to_string());
    let listener = tokio::net::TcpListener::bind(bind_addr).await.unwrap();

    axum::serve(listener, app).await.unwrap();
}

async fn root_handler(db: Extension<PgPool>) -> Result<Html<String>, AppError> {
    let resp: Vec<SummaryTableEntry> = sqlx::query_as(
        "SELECT DISTINCT org, repo, coverage, MAX(inserttime) AS inserttime FROM summary GROUP BY org, repo, coverage",
    )
    .fetch_all(&*db)
    .await?;

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

    let tera = Tera::new("templates/**/*").unwrap();
    let mut context = tera::Context::new();
    context.insert("orgs", &orgs);

    let output = tera.render("base.html", &context)?;

    Ok(Html::from(output))
}

async fn summary_handler(
    db: Extension<PgPool>,
    Path((org, repo)): Path<(String, String)>,
    Json(payload): Json<CoverageSummary>,
) -> Result<(), AppError> {
    let json_coverage = serde_json::to_value(payload)?;

    let resp = sqlx::query("INSERT INTO summary VALUES (now(), $1, $2, $3)")
        .bind(&org)
        .bind(&repo)
        .bind(json_coverage)
        .execute(&*db)
        .await?;

    if resp.rows_affected() == 1 {
        Ok(())
    } else {
        Err(anyhow!("Unable to insert to DB").into())
    }
}

fn construct_db_connection_string() -> String {
    let pg_password = std::env::var("POSTGRES_PASSWORD").expect("This is a required env var");
    let pg_db = std::env::var("POSTGRES_DB").expect("This is a required env var");

    format!("postgres://postgres:{pg_password}@db/{pg_db}")
}
