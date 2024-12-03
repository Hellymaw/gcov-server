use axum::{
    extract::{Path, Query},
    response::Html,
    Extension,
};
use sqlx::postgres::PgPool;
use std::collections::HashMap;
use tera::{Context, Tera};
use tracing::info;

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
    Path((_owner, _repo, _path)): Path<(String, String, Vec<String>)>,
) -> Html<String> {
    todo!()
}
