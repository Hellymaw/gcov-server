use lazy_static::lazy_static;
use sqlx::PgPool;
use sqlx::Pool;
use sqlx::Postgres;
use thiserror::Error;

lazy_static! {
    static ref CONNECTION_URL: String = {
        let pg_password = fetch_env_var_exiting("POSTGRES_PASSWORD");
        let pg_db = fetch_env_var_exiting("POSTGRES_DB");

        format!("postgres://postgres:{pg_password}@db/{pg_db}")
    };
}

#[derive(Error, Debug)]
pub enum DbError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Fetches the environment variable `key` from the process, exiting the process on error.
fn fetch_env_var_exiting(key: &str) -> String {
    match std::env::var(key) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "${} {}. This is required for the program to function.",
                key, e
            );
            ::std::process::exit(2);
        }
    }
}

/// Connects to the DB instance and performs any required setup (Like creating tables etc).
pub async fn connect_and_setup() -> Result<Pool<Postgres>, sqlx::Error> {
    let db_pool = PgPool::connect(&CONNECTION_URL).await?;

    let _ = summary::setup_table(&db_pool).await?;
    let _ = reports::setup_table(&db_pool).await?;

    Ok(db_pool)
}

pub mod summary {
    use crate::db::DbError;
    use serde::{ser::SerializeStruct, Deserialize, Serialize};
    use sqlx::{postgres::PgQueryResult, PgPool};

    // GCOV generates the JSON with flat fields in the form "branch_covered", "function_covered", etc
    // This means we can extract the commonality within `Coverage`
    serde_with::with_prefix!(prefix_branch "branch_");
    serde_with::with_prefix!(prefix_function "function_");
    serde_with::with_prefix!(prefix_line "line_");

    /// Represents a test coverage
    #[derive(Serialize, Deserialize)]
    pub struct Coverage {
        /// Number of cases covered
        pub covered: usize,
        /// Total number of cases
        pub total: usize,
        /// Percentage of cases covered, i.e. `covered / total`
        pub percent: f64,
    }

    /// Represents a GCOV JSON coverage summary report
    #[derive(Serialize, Deserialize)]
    pub struct CoverageSummary {
        #[serde(flatten, with = "prefix_branch")]
        pub branch: Coverage,
        #[serde(flatten, with = "prefix_function")]
        pub function: Coverage,
        #[serde(flatten, with = "prefix_line")]
        pub line: Coverage,
    }

    /// Represents a row in the 'summary' db table
    #[derive(sqlx::FromRow, Debug)]
    pub struct SummaryTableEntry {
        /// Row insertion time
        pub insert_time: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
        /// Gitea organisation the repo belongs to
        pub org: String,
        /// Gitea repository the summary belongs to
        pub repo: String,
        /// Test coverage summary
        pub coverage: sqlx::types::JsonValue,
    }

    impl Serialize for SummaryTableEntry {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            let mut state = serializer.serialize_struct("SummaryTableEntry", 4)?;

            state.serialize_field("insert_time", &self.insert_time.timestamp())?;
            state.serialize_field("org", &self.org)?;
            state.serialize_field("repo", &self.repo)?;
            state.serialize_field("coverage", &self.coverage)?;

            state.end()
        }
    }

    /// Creates the summary db table if it doesn't exist
    pub(super) async fn setup_table(db: &PgPool) -> Result<PgQueryResult, sqlx::Error> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS summary (
                        insert_time timestamptz, 
                        org varchar, 
                        repo varchar, 
                        coverage jsonb
                    );"#,
        )
        .execute(db)
        .await
    }

    /// Inserts a test coverage summary into the summary db table
    pub async fn insert_into_table(
        db: &PgPool,
        org: &str,
        repo: &str,
        coverage: &CoverageSummary,
    ) -> Result<(), DbError> {
        let json_coverage = serde_json::to_value(coverage)?;

        let _resp = sqlx::query("INSERT INTO summary VALUES (now(), $1, $2, $3)")
            .bind(org)
            .bind(repo)
            .bind(json_coverage)
            .execute(db)
            .await?;

        Ok(())
    }

    /// Fetches the summary table
    pub async fn fetch_table(db: &PgPool) -> Result<Vec<SummaryTableEntry>, DbError> {
        let resp: Vec<SummaryTableEntry> = sqlx::query_as(
            "SELECT DISTINCT org, repo, coverage, MAX(inserttime) AS inserttime FROM summary GROUP BY org, repo, coverage",
        )
        .fetch_all(&*db)
        .await?;

        Ok(resp)
    }
}

pub mod reports {
    use crate::db::DbError;
    use serde::{ser::SerializeStruct, Serialize};
    use sqlx::{postgres::PgQueryResult, PgPool};

    /// Represents a row in the 'summary' db table
    #[derive(sqlx::FromRow, Debug)]
    pub struct ReportTableEntry {
        /// Row insertion time
        pub insert_time: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
        /// Gitea organisation the repo belongs to
        pub org: String,
        /// Gitea repository the report belongs to
        pub repo: String,
        /// Git branch the report belongs to
        pub branch: String,
        /// Git commit the report belongs to
        pub commit: String,
    }

    impl Serialize for ReportTableEntry {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            let mut state = serializer.serialize_struct("ReportTableEntry", 4)?;

            state.serialize_field("insert_time", &self.insert_time.timestamp())?;
            state.serialize_field("org", &self.org)?;
            state.serialize_field("repo", &self.repo)?;
            state.serialize_field("branch", &self.branch)?;
            state.serialize_field("commit", &self.commit)?;

            state.end()
        }
    }

    /// Creates the report db table if it doesn't exist
    pub(super) async fn setup_table(db: &PgPool) -> Result<PgQueryResult, sqlx::Error> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS reports (
                        report_id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                        insert_time timestamptz,
                        org varchar,
                        repo varchar,
                        branch varchar,
                        commit varchar
                    );"#,
        )
        .execute(db)
        .await
    }

    /// Fetches the report table
    pub async fn fetch_table(db: &PgPool) -> Result<Vec<ReportTableEntry>, DbError> {
        let resp: Vec<ReportTableEntry> = sqlx::query_as(
            "SELECT insert_time, org, repo, branch, commit FROM reports ORDER BY org, repo, insert_time",
        )
        .fetch_all(&*db)
        .await?;

        Ok(resp)
    }

    /// Inserts a test coverage report into the report db table
    pub async fn insert_into_table(
        db: &PgPool,
        organisation: &str,
        repository: &str,
        branch: &str,
        commit: &str,
    ) -> Result<(), DbError> {
        let _resp = sqlx::query("INSERT INTO reports(insert_time, org, repo, branch, commit) VALUES (now(), $1, $2, $3, $4)")
            .bind(organisation)
            .bind(repository)
            .bind(branch)
            .bind(commit)
            .execute(db)
            .await?;

        Ok(())
    }
}
