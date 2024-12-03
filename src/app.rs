use axum::{
    extract::{Path, Query},
    response::Html,
    Extension,
};
use serde::Serialize;
use sqlx::postgres::PgPool;
use std::collections::HashMap;
use tera::{Context, Tera};
use tracing::info;

use crate::gcovr;
use crate::gitea;

pub struct AppError(anyhow::Error);

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
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
struct FileTemplate<'a> {
    source: &'a str,
    line_number: usize,
}

lazy_static::lazy_static! {
    pub static ref TEMPLATES: Tera = Tera::new("templates/**/*").unwrap();
}

pub async fn root_handler(_db: Extension<PgPool>) -> Result<Html<String>, AppError> {
    Ok(Html(TEMPLATES.render("root.html", &Context::new())?))
}

pub async fn owner_handler(
    _db: Extension<PgPool>,
    Path(owner): Path<String>,
) -> Result<Html<String>, AppError> {
    let mut context = Context::new();
    context.insert("owner", &owner);
    Ok(Html(TEMPLATES.render("owner.html", &context)?))
}

pub async fn repo_handler(
    _db: Extension<PgPool>,
    Path((owner, repo)): Path<(String, String)>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<Html<String>, AppError> {
    let mut context = tera::Context::new();
    context.insert("repo", &repo);
    context.insert("owner", &owner);

    Ok(Html::from(TEMPLATES.render("repo.html", &context)?))
}

pub async fn tree_handler(
    _db: Extension<PgPool>,
    Path((owner, repo, path)): Path<(String, String, String)>,
) -> Result<Html<String>, AppError> {
    let path: Vec<&str> = path.split('/').collect();

    let commit = path.first().ok_or(anyhow::anyhow!("Need a commit SHA!"))?;

    let repo_contents = gitea::get_repo_contents(&owner, &repo, commit).await;

    info!("{:?}", repo_contents);

    let mut context = tera::Context::new();
    context.insert("owner", &owner);
    context.insert("repo", &repo);
    context.insert("commit", commit);
    context.insert("files", &repo_contents);

    Ok(Html::from(TEMPLATES.render("tree.html", &context).unwrap()))
}

pub async fn blob_handler(
    _db: Extension<PgPool>,
    Path((_owner, _repo, path)): Path<(String, String, String)>,
) -> Result<Html<String>, AppError> {
    let (_commit, path) = path
        .split_once('/')
        .ok_or(anyhow::anyhow!("Need a filepath!"))?;

    let file_data = gitea::get_file(&std::path::Path::new(path)).await;

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

    Ok(Html::from(TEMPLATES.render("blob.html", &context)?))
}

pub mod tmp {
    use axum::extract::{Path, Query};
    use axum::response::Html;
    use axum::Extension;
    use sqlx::PgPool;
    use std::collections::HashMap;

    use crate::db;
    use crate::TEMPLATES;

    pub async fn root_summary_handler(
        db: Extension<PgPool>,
        Query(params): Query<HashMap<String, String>>,
    ) -> Html<String> {
        let root = params.get("root").is_some_and(|x| x.len() > 0);

        let summaries =
            db::tmp::fetch_latest_summaries(&*db, params.get("owner"), params.get("repo")).await;

        let mut context = tera::Context::new();
        context.insert("summaries", &summaries);
        context.insert("root", &root);

        let output = TEMPLATES
            .render("summary/root_owner.html", &context)
            .unwrap();

        Html::from(output)
    }

    pub async fn repo_summaries_handler(
        _db: Extension<PgPool>,
        Path((owner, repo)): Path<(String, String)>,
        Query(params): Query<HashMap<String, String>>,
    ) -> Html<String> {
        let reports =
            db::tmp::fetch_repo_coverage_reports(&owner, &repo, params.get("branch")).await;

        let mut context = tera::Context::new();
        context.insert("reports", &reports);
        context.insert("repo", &repo);
        context.insert("owner", &owner);

        let output = TEMPLATES.render("summary/repo.html", &context).unwrap();

        Html::from(output)
    }
}
