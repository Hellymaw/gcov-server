use axum::{
    extract::{Json, Path, Query},
    response::Html,
    Extension,
};
use serde::Serialize;
use sqlx::postgres::PgPool;
use std::collections::HashMap;
use tera::{Context, Tera};
use tracing::info;

use crate::{db, gcovr};
use crate::{db::reports::ReportTableEntry, gitea};

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

#[derive(Debug, Serialize)]
struct FileBranchTemplate {
    excluded: bool,
    count: usize,
}

#[derive(Serialize, Debug)]
struct FileTemplate<'a> {
    source: &'a str,
    line_number: usize,
    linebranchtaken: usize,
    linebranchtotal: usize,
    branches: Vec<FileBranchTemplate>,
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
    let (commit, path) = path.split_once('/').unwrap_or((&path, ""));

    let mut path = path.to_string();
    if !path.is_empty() {
        path = "/".to_string() + &path;
    }

    let repo_contents =
        gitea::repository::get_repository_entries(&owner, &repo, Some(commit), &path).await?;
    info!("{:?}", repo_contents);

    #[derive(Serialize)]
    struct Test {
        is_dir: bool,
        name: String,
    }

    if let gitea::repository::Entry::Directory(contents) = repo_contents {
        let contents: Vec<Test> = contents
            .iter()
            .map(|c| Test {
                is_dir: matches!(c, gitea::repository::DirectoryEntries::Directory(_)),
                name: c.name().to_string(),
            })
            .collect();

        let mut context = tera::Context::new();
        context.insert("owner", &owner);
        context.insert("repo", &repo);
        context.insert("commit", commit);
        context.insert("path", &path);
        context.insert("files", &contents);

        Ok(Html::from(TEMPLATES.render("tree.html", &context).unwrap()))
    } else {
        Err(anyhow::anyhow!("Not a valid path!").into())
    }
}

pub async fn blob_handler(
    db: Extension<PgPool>,
    Path((owner, repo, path)): Path<(String, String, String)>,
) -> Result<Html<String>, AppError> {
    let (commit, path) = path
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("Need a filepath!"))?;

    let path = "/".to_string() + path;

    let file_data =
        gitea::repository::get_repository_entries(&owner, &repo, Some(commit), &path).await?;

    if let gitea::repository::Entry::File { content } = file_data {
        // NOTE: path is not normalised correctly
        let file_report =
            db::reports::fetch_specific_record(&*db, &owner, &repo, commit, &path).await?;
        let mut entry = file_report.report;
        entry
            .lines
            .sort_by(|a, b| a.line_number.cmp(&b.line_number));

        let source_lines: Vec<FileTemplate> = content
            .lines()
            .zip(entry.lines)
            .map(|(source, entry)| {
                let mut branches = Vec::<FileBranchTemplate>::new();
                let mut branches_taken = 0;
                let mut total_branches = 0;
                for branch in entry.branches {
                    total_branches += 1;
                    if branch.count > 0 {
                        branches_taken += 1;
                    }

                    branches.push(FileBranchTemplate {
                        excluded: false,
                        count: branch.count,
                    });
                }

                FileTemplate {
                    source,
                    line_number: entry.line_number,
                    linebranchtaken: branches_taken,
                    linebranchtotal: total_branches,
                    branches,
                }
            })
            .collect();

        let mut context = tera::Context::new();
        context.insert("source_lines", &source_lines);

        Ok(Html::from(TEMPLATES.render("blob.html", &context)?))
    } else {
        Err(anyhow::anyhow!("Not a valid path!").into())
    }
}

pub async fn ingest_report(
    db: Extension<PgPool>,
    Path((owner, repo, commit)): Path<(String, String, String)>,
    Json(payload): Json<Vec<gcovr::FileEntry>>,
) -> Result<(), AppError> {
    tracing::info!("{payload:?}");

    // TODO: Shift into ::repository::
    let repo_id = if let Some(id) = db::repository::fetch_id(&db, &owner, &repo).await? {
        id
    } else {
        db::repository::insert(&db, &owner, &repo).await?;
        db::repository::fetch_id(&db, &owner, &repo)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?
    };

    let mut summary = db::summary::CoverageSummary::default();
    for entry in payload {
        for lines in &entry.lines {
            summary.line.total += lines.count as i32;
            summary.line.covered += lines.count as i32;

            for branch in &lines.branches {
                summary.branch.covered += branch.count as i32;
                summary.branch.total += branch.count as i32;
            }
        }

        // todo normalise filename
        let filepath = entry.filename.replace("/", ".");

        db::reports::insert_into_table(&*db, &repo_id, "main", &commit, &filepath, &entry).await?;
    }

    let _ = db::summary::insert_into_table(&db, &owner, &repo, "main", &commit, &summary).await;

    Ok(())
}

pub async fn test_ingest_report(
    db: Extension<PgPool>,
    Path((_owner, _repo, _commit, _filepath)): Path<(String, String, String, String)>,
) -> Result<Json<Vec<ReportTableEntry>>, AppError> {
    let table = db::reports::fetch_table(&*db).await?;

    Ok(Json(table))
}

pub async fn root_summary_handler(
    db: Extension<PgPool>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Html<String>, AppError> {
    let root = params.get("root").is_some_and(|x| x.len() > 0);

    let mut summaries = db::summary::fetch_latest_summaries(&*db).await?;

    tracing::info!("{summaries:?}");

    if let Some(owner) = params.get("owner") {
        summaries.retain(|s| s.owner.contains(owner));
    }

    if let Some(repo) = params.get("repo") {
        summaries.retain(|s| s.owner.contains(repo));
    }

    let mut context = tera::Context::new();
    context.insert("summaries", &summaries);
    context.insert("root", &root);

    let output = TEMPLATES
        .render("summary/root_owner.html", &context)
        .unwrap();

    Ok(Html::from(output))
}

pub async fn repo_summaries_handler(
    db: Extension<PgPool>,
    Path((owner, repo)): Path<(String, String)>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<Html<String>, AppError> {
    let reports = db::summary::fetch_repo_summaries(&db, &owner, &repo).await?;

    let mut context = tera::Context::new();
    context.insert("reports", &reports);
    context.insert("repo", &repo);
    context.insert("owner", &owner);

    let output = TEMPLATES.render("summary/repo.html", &context).unwrap();

    Ok(Html::from(output))
}
