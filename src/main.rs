use anyhow::anyhow;
use axum::{
    extract::{Json, Path, Query},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Extension, Router,
};
use lazy_static::lazy_static;
use serde::Serialize;
use sqlx::postgres::{PgPool, PgRow};
use std::{collections::HashMap, fs, vec};
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

pub mod app;
pub mod gcovr;
pub mod gitea;

use app::TEMPLATES;

const MAX_LOG_FILES: usize = 48;

#[derive(Serialize, Debug)]
struct GiteaOrg {
    name: String,
    repos: Vec<SummaryTableEntry>,
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
        .route("/", get(app::root_handler))
        .route("/:owner", get(app::owner_handler))
        .route("/:owner/:repo", get(app::repo_handler))
        .route("/:owner/:repo/tree/*path", get(app::tree_handler))
        .route("/:owner/:repo/blob/*path", get(app::blob_handler))
        .route("/:org/:repo/summaries", get(repo_summaries_handler))
        .route("/:org/:repo/coverage", get(coverage_handler))
        .route(
            "/:org/:repo/:commit/coverage",
            get(directory_coverage_handler),
        )
        .route(
            "/:org/:repo/:commit/:file/coverage",
            get(file_coverage_handler),
        )
        .route("/summaries", get(test_handler))
        .layer(Extension(db_pool))
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

async fn directory_coverage_handler(
    db: Extension<PgPool>,
    Path((org, repo, commit)): Path<(String, String, String)>,
) -> Html<String> {
    let repo_contents = gitea::get_repo_contents(&org, &repo, &commit).await;

    let mut context = tera::Context::new();
    context.insert("files", &repo_contents);

    let output = TEMPLATES
        .render("coverage/directory.html", &context)
        .unwrap();

    Html::from(output)
}

#[derive(Serialize, Debug)]
struct FileTemplate<'a> {
    source: &'a str,
    line_number: usize,
}

async fn file_coverage_handler(
    db: Extension<PgPool>,
    Path((org, repo, commit, file)): Path<(String, String, String, String)>,
) -> Html<String> {
    let file_data = gitea::get_file(std::path::Path::new(".")).await;

    let mut entry = gcovr::fake_file_entry();
    entry
        .lines
        .sort_by(|a, b| a.line_number.cmp(&b.line_number));

    let source_lines: Vec<FileTemplate> = file_data
        .lines()
        .zip(entry.lines)
        .map(|(source, entry)| FileTemplate {
            source,
            line_number: entry.line_number,
        })
        .collect();

    let mut context = tera::Context::new();
    context.insert("source_lines", &source_lines);

    let output = TEMPLATES.render("coverage/file.html", &context).unwrap();

    Html::from(output)
}

#[derive(Serialize, Debug)]
struct CoverageNew {
    branch: f64,
    function: f64,
    line: f64,
}

#[derive(Serialize, Debug)]
struct Summary {
    org: String,
    repo: String,
    coverage: CoverageNew,
}

async fn fetch_latest_summaries(
    db: &PgPool,
    org: Option<&String>,
    repo: Option<&String>,
) -> Vec<Summary> {
    let mut data = vec![
        Summary {
            org: "org1".to_string(),
            repo: "repo1".to_string(),
            coverage: CoverageNew {
                branch: 10.0,
                function: 20.1,
                line: 30.5,
            },
        },
        Summary {
            org: "org1".to_string(),
            repo: "repo2".to_string(),
            coverage: CoverageNew {
                branch: 10.0,
                function: 20.1,
                line: 30.5,
            },
        },
        Summary {
            org: "org1".to_string(),
            repo: "repo1".to_string(),
            coverage: CoverageNew {
                branch: 10.0,
                function: 20.1,
                line: 30.5,
            },
        },
    ];

    if let Some(org) = org {
        data.retain(|x| x.org.contains(org));
    }

    if let Some(repo) = repo {
        data.retain(|x| x.repo.contains(repo));
    }

    data
}

async fn test_handler(
    db: Extension<PgPool>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    let root = params.get("root").is_some_and(|x| x.len() > 0);

    let summaries = fetch_latest_summaries(&*db, params.get("owner"), params.get("repo")).await;

    let mut context = tera::Context::new();
    context.insert("summaries", &summaries);
    context.insert("root", &root);

    let output = TEMPLATES
        .render("coverage/root_owner.html", &context)
        .unwrap();

    Html::from(output)
}

#[derive(Serialize, Debug)]
struct CoverageReport {
    branch: String,
    commit: String,
    coverage: CoverageNew,
}

async fn fetch_repo_coverage_reports(
    _org: &str,
    _repo: &str,
    branch: Option<&String>,
) -> Vec<CoverageReport> {
    let mut data = vec![
        CoverageReport {
            branch: "feature1".to_string(),
            commit: "qwe".to_string(),
            coverage: CoverageNew {
                branch: 10.0,
                function: 20.1,
                line: 30.5,
            },
        },
        CoverageReport {
            branch: "feature1".to_string(),
            commit: "zxc".to_string(),
            coverage: CoverageNew {
                branch: 10.0,
                function: 20.1,
                line: 30.5,
            },
        },
        CoverageReport {
            branch: "feature2".to_string(),
            commit: "asd".to_string(),
            coverage: CoverageNew {
                branch: 10.0,
                function: 20.1,
                line: 30.5,
            },
        },
    ];

    if let Some(branch) = branch {
        data.retain(|x| x.branch.contains(branch));
    }

    data
}

async fn coverage_handler(
    db: Extension<PgPool>,
    Path((org, repo)): Path<(String, String)>,
) -> Html<String> {
    let mut context = tera::Context::new();
    context.insert("repo", &repo);
    context.insert("org", &org);

    let output = TEMPLATES.render("coverage/repo.html", &context).unwrap();

    Html::from(output)
}

async fn repo_summaries_handler(
    db: Extension<PgPool>,
    Path((owner, repo)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    let reports = fetch_repo_coverage_reports(&owner, &repo, params.get("branch")).await;

    let mut context = tera::Context::new();
    context.insert("reports", &reports);
    context.insert("repo", &repo);
    context.insert("owner", &owner);

    let output = TEMPLATES
        .render("coverage/repo_summary.html", &context)
        .unwrap();

    Html::from(output)
}
