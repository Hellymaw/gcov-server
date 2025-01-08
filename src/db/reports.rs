use crate::{db::DbError, gcovr};
use serde::{ser::SerializeStruct, Serialize};
use sqlx::{postgres::PgQueryResult, PgPool};
use tracing::instrument;

use super::repository;

/// Represents a row in the 'summary' db table
#[derive(sqlx::FromRow, Debug)]
pub struct ReportTableEntry {
    /// Row insertion time
    pub insert_time: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    /// Gitea organisation the repo belongs to
    pub owner: String,
    /// Gitea repository the report belongs to
    pub name: String,
    /// Git branch the report belongs to
    pub branch: String,
    /// Git commit the report belongs to
    pub commit: String,
    /// File in the repository the report belongs to
    pub filepath: sqlx::postgres::types::PgLTree,
    /// Report
    #[sqlx(json)]
    pub report: gcovr::FileEntry,
}

impl Serialize for ReportTableEntry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("ReportTableEntry", 4)?;

        state.serialize_field("insert_time", &self.insert_time.timestamp())?;
        state.serialize_field("owner", &self.owner)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("branch", &self.branch)?;
        state.serialize_field("commit", &self.commit)?;
        state.serialize_field("filepath", &self.filepath.to_string())?;
        state.serialize_field("report", &self.report)?;

        state.end()
    }
}

/// Creates the report db table if it doesn't exist
#[instrument(err, skip(db))]
pub(super) async fn setup_table(db: &PgPool) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS reports (
             report_id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
             insert_time timestamptz,
             repository_id int REFERENCES repository (id),
             branch varchar,
             commit varchar,
             filepath ltree,
             report jsonb
         );"#,
    )
    .execute(db)
    .await
}

/// Fetches the report table
#[instrument(err, skip(db))]
pub async fn fetch_table(db: &PgPool) -> Result<Vec<ReportTableEntry>, DbError> {
    let resp: Vec<ReportTableEntry> = sqlx::query_as(
         "SELECT reports.insert_time, repository.owner, repository.name, reports.branch, reports.commit, reports.filepath, reports.report FROM reports INNER JOIN repository ON repository.id=reports.repository_id ORDER BY repository.owner, repository.name, insert_time;",
     )
     .fetch_all(&*db)
     .await?;

    Ok(resp)
}

#[instrument(err, skip(db))]
pub async fn fetch_specific_record(
    db: &PgPool,
    owner: &str,
    repo: &str,
    commit: &str,
    filepath: &str,
) -> Result<Option<ReportTableEntry>, sqlx::Error> {
    let filepath = filepath.replace('/', "");

    tracing::info!("{filepath:?}");

    let resp: Result<Option<ReportTableEntry>, sqlx::Error> = sqlx::query_as(
         "SELECT reports.insert_time, repository.owner, repository.name, reports.branch, reports.commit, reports.filepath, reports.report FROM reports INNER JOIN repository ON repository.id=reports.repository_id WHERE repository.owner = $1 AND repository.name = $2 AND commit = $3 AND filepath = CAST($4 AS ltree);",
     )
     .bind(owner)
     .bind(repo)
     .bind(commit)
     .bind(filepath)
     .fetch_optional(&*db)
     .await;

    if let Err(x) = &resp {
        tracing::error!("{x:?}");
    }

    resp
}

/// Inserts a test coverage report into the report db table
#[instrument(err, skip(db))]
pub async fn insert_into_table(
    db: &PgPool,
    repository_id: &repository::RepositoryId,
    branch: &str,
    commit: &str,
    filepath: &str,
    report: &gcovr::FileEntry,
) -> Result<(), DbError> {
    sqlx::query(
        r#"INSERT INTO
             reports(insert_time, repository_id, branch, commit, filepath, report)
         VALUES (now(), $1, $2, $3, CAST($4 AS ltree), $5);"#,
    )
    .bind(repository_id)
    .bind(branch)
    .bind(commit)
    .bind(filepath)
    .bind(sqlx::types::Json(report))
    .execute(db)
    .await?;

    Ok(())
}
