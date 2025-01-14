use serde::{Deserialize, Serialize};
use sqlx::{
    postgres::{PgQueryResult, PgRow},
    PgPool, Row,
};
use tracing::instrument;

use super::repository;

/// Represents a test coverage
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Default)]
pub struct Coverage {
    /// Number of cases covered
    pub covered: i32,
    /// Total number of cases
    pub total: i32,
}

/// Represents a GCOV JSON coverage summary report
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CoverageSummary {
    pub branch: Coverage,
    pub function: Coverage,
    pub line: Coverage,
}

impl sqlx::FromRow<'_, PgRow> for CoverageSummary {
    fn from_row(row: &'_ PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            branch: Coverage {
                covered: row.try_get("branch_covered")?,
                total: row.try_get("branch_total")?,
            },
            function: Coverage {
                covered: row.try_get("function_covered")?,
                total: row.try_get("function_total")?,
            },
            line: Coverage {
                covered: row.try_get("line_covered")?,
                total: row.try_get("line_total")?,
            },
        })
    }
}

#[derive(Debug, sqlx::Type)]
#[sqlx(transparent)]
pub struct UtcDateTime(sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>);

impl Serialize for UtcDateTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_i64(self.0.timestamp())
    }
}

/// Represents a row in the 'summary' db table
#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct SummaryTableEntry {
    pub id: i32,
    /// Gitea repository the summary belongs to
    pub repository: i32,
    pub brnach: String,
    pub commit: String,
    /// Row insertion time
    pub insert_time: UtcDateTime,
    /// Test coverage summary
    #[sqlx(flatten)]
    pub coverage: CoverageSummary,
}

/// Creates the summary db table if it doesn't exist
#[instrument(err, skip(db))]
pub(super) async fn setup_table(db: &PgPool) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS summary (
                    id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                    insert_time timestamptz,
                    repository_id int REFERENCES repository (id),
                    branch varchar,
                    commit varchar,
                    branch_covered int,
                    branch_total int,
                    function_covered int,
                    function_total int,
                    line_covered int,
                    line_total int
                );"#,
    )
    .execute(db)
    .await
}

/// Inserts a test coverage summary into the summary db table
#[instrument(err, skip(db))]
pub async fn insert_into_table(
    db: &PgPool,
    org: &str,
    repo: &str,
    branch: &str,
    commit: &str,
    coverage: &CoverageSummary,
) -> Result<PgQueryResult, sqlx::Error> {
    let repo_id = repository::fetch_id_or_insert(db, org, repo).await?;
    sqlx::query("INSERT INTO summary(insert_time, repository_id, branch, commit, branch_covered, branch_total, function_covered, function_total, line_covered, line_total) VALUES (now(), $1, $2, $3, $4, $5, $6, $7, $8, $9);")
        .bind(repo_id)
        .bind(branch)
        .bind(commit)
        .bind(coverage.branch.covered)
        .bind(coverage.branch.total)
        .bind(coverage.function.covered)
        .bind(coverage.function.total)
        .bind(coverage.line.covered)
        .bind(coverage.line.total)
        .execute(db)
        .await
}

/// Represents a row in the 'summary' db table
#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct RepoSummary {
    pub insert_time: UtcDateTime,
    pub branch: String,
    pub commit: String,
    #[sqlx(flatten)]
    pub coverage: CoverageSummary,
    pub owner: String,
    pub name: String,
}

/// Fetches the summary table
#[instrument(err, skip(db))]
pub async fn fetch_latest_summaries(db: &PgPool) -> Result<Vec<RepoSummary>, sqlx::Error> {
    let t = sqlx::query_as(
        r#"SELECT * FROM summary a
        INNER JOIN repository ON repository.id=a.repository_id
        WHERE a.insert_time = (SELECT MAX(insert_time) FROM summary b WHERE a.repository_id = b.repository_id);"#,
    )
    .fetch_all(&*db)
    .await;

    if let Err(x) = &t {
        tracing::error!("{x:?}");
    }
    t
}

#[instrument(err, skip(db))]
pub async fn fetch_repo_summaries(
    db: &PgPool,
    owner: &str,
    repo: &str,
) -> Result<Vec<RepoSummary>, sqlx::Error> {
    sqlx::query_as(
        r#"SELECT * FROM summary
        INNER JOIN repository ON repository.id=summary.repository_id
        WHERE repository.owner = $1 and repository.name = $2
        ORDER BY summary.insert_time DESC;"#,
    )
    .bind(owner)
    .bind(repo)
    .fetch_all(&*db)
    .await
}
