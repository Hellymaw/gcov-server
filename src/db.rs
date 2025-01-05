use lazy_static::lazy_static;
use sqlx::postgres::PgQueryResult;
use sqlx::FromRow;
use sqlx::PgPool;
use sqlx::Pool;
use sqlx::Postgres;
use thiserror::Error;

lazy_static! {
    static ref CONNECTION_URL: String = {
        let user = fetch_env_var_exiting("POSTGRES_USER");
        let password = fetch_env_var_exiting("POSTGRES_PASSWORD");
        let db = fetch_env_var_exiting("POSTGRES_DB");
        let host = fetch_env_var_exiting("POSTGRES_HOST");
        let port = fetch_env_var_exiting("POSTGRES_PORT");

        format!("postgres://{user}:{password}@{host}:{port}/{db}")
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

pub mod repository {
    use sqlx::postgres::PgQueryResult;
    use sqlx::FromRow;
    use sqlx::PgPool;

    #[derive(Debug, FromRow, sqlx::Type)]
    #[sqlx(transparent)]
    pub struct RepositoryId(i32);

    #[derive(Debug, FromRow)]
    pub struct RepositoryTableEntry {
        id: RepositoryId,
        owner: String,
        name: String,
    }

    pub(crate) async fn setup_table(db: &PgPool) -> Result<PgQueryResult, sqlx::Error> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS repository (
                    id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                    owner varchar NOT NULL,
                    name varchar NOT NULL,
                    UNIQUE (owner, name)
                );"#,
        )
        .execute(db)
        .await
    }

    pub async fn insert(
        db: &PgPool,
        owner: &str,
        repository: &str,
    ) -> Result<PgQueryResult, sqlx::Error> {
        sqlx::query("INSERT INTO repository VALUES ($1, $2);")
            .bind(owner)
            .bind(repository)
            .execute(db)
            .await
    }

    pub async fn fetch_id(
        db: &PgPool,
        owner: &str,
        name: &str,
    ) -> Result<Option<RepositoryId>, sqlx::Error> {
        sqlx::query_as("SELECT id FROM repository WHERE owner = $1 AND name = $2;")
            .bind(owner)
            .bind(name)
            .fetch_optional(db)
            .await
    }
}

pub mod summary {
    use crate::db::DbError;
    use serde::{Deserialize, Serialize};
    use sqlx::{
        postgres::{PgQueryResult, PgRow},
        PgPool, Row,
    };

    use super::repository;

    /// Represents a test coverage
    #[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
    pub struct Coverage {
        /// Number of cases covered
        pub covered: i32,
        /// Total number of cases
        pub total: i32,
    }

    /// Represents a GCOV JSON coverage summary report
    #[derive(Debug, Serialize, Deserialize)]
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
        /// Row insertion time
        pub insert_time: UtcDateTime,
        /// Test coverage summary
        #[sqlx(flatten)]
        pub coverage: CoverageSummary,
    }

    /// Creates the summary db table if it doesn't exist
    pub(super) async fn setup_table(db: &PgPool) -> Result<PgQueryResult, sqlx::Error> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS summary (
                        id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                        insert_time timestamptz,
                        repository_id int REFERENCES repository (id),
                        branch_covered int,
                        branch_total int,
                        function_covered int,
                        function_total int,
                        line_covered int,
                        line_total int,
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
    ) -> Result<PgQueryResult, sqlx::Error> {
        let repo_id = if let Some(id) = repository::fetch_id(db, org, repo).await? {
            id
        } else {
            repository::insert(db, org, repo).await?;
            repository::fetch_id(db, org, repo)
                .await?
                .ok_or(sqlx::Error::RowNotFound)?
        };

        sqlx::query("INSERT INTO summary VALUES (now(), $1, $2, $3, $4, $5, $6, $7)")
            .bind(repo_id)
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
        pub owner: String,
        pub name: String,
        pub insert_time: UtcDateTime,
        #[sqlx(flatten)]
        pub coverage: CoverageSummary,
    }

    /// Fetches the summary table
    pub async fn fetch_latest_summaries(db: &PgPool) -> Result<Vec<RepoSummary>, sqlx::Error> {
        sqlx::query_as(
            r#"SELECT * FROM summary a
            INNER JOIN repository ON repository.id=summary.repository_id
            WHERE summary.insert_time = (SELECT MAX(insert_time) FROM summary b WHERE a.repo_id = b.repo_id);"#,
        )
        .fetch_all(&*db)
        .await
    }

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
}

pub mod reports {
    use crate::{db::DbError, gcovr};
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
            state.serialize_field("org", &self.org)?;
            state.serialize_field("repo", &self.repo)?;
            state.serialize_field("branch", &self.branch)?;
            state.serialize_field("commit", &self.commit)?;
            state.serialize_field("filepath", &self.filepath.to_string())?;
            state.serialize_field("report", &self.report)?;

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
                commit varchar,
                filepath ltree,
                report jsonb
            );"#,
        )
        .execute(db)
        .await
    }

    /// Fetches the report table
    pub async fn fetch_table(db: &PgPool) -> Result<Vec<ReportTableEntry>, DbError> {
        let resp: Vec<ReportTableEntry> = sqlx::query_as(
            "SELECT insert_time, org, repo, branch, commit, filepath, report FROM reports ORDER BY org, repo, insert_time",
        )
        .fetch_all(&*db)
        .await?;

        Ok(resp)
    }

    pub async fn fetch_specific_record(
        db: &PgPool,
        owner: &str,
        repo: &str,
        commit: &str,
        filepath: &str,
    ) -> Result<ReportTableEntry, DbError> {
        let filepath = filepath.replace('/', "").replace(".", "_");

        let resp: ReportTableEntry = sqlx::query_as(
            "SELECT insert_time, org, repo, branch, commit, filepath, report FROM reports WHERE org = $1 AND repo = $2 AND commit = $3 AND filepath = CAST($4 AS ltree)",
        )
        .bind(owner)
        .bind(repo)
        .bind(commit)
        .bind(filepath)
        .fetch_one(&*db)
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
        filepath: &str,
        report: serde_json::Value,
    ) -> Result<(), DbError> {
        sqlx::query(
            r#"INSERT INTO
                reports(insert_time,org, repo, branch, commit, filepath, report)
            VALUES (now(), $1, $2, $3, $4, CAST($5 AS ltree), $6)"#,
        )
        .bind(organisation)
        .bind(repository)
        .bind(branch)
        .bind(commit)
        .bind(filepath)
        .bind(report)
        .execute(db)
        .await?;

        Ok(())
    }
}

pub mod tmp {
    use serde::Serialize;
    use sqlx::PgPool;

    #[derive(Serialize, Debug)]
    pub struct CoverageNew {
        pub branch: f64,
        pub function: f64,
        pub line: f64,
    }

    #[derive(Serialize, Debug)]
    pub struct Summary {
        pub org: String,
        pub repo: String,
        pub coverage: CoverageNew,
    }

    pub async fn fetch_latest_summaries(
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

    #[derive(Serialize, Debug)]
    pub struct CoverageReport {
        pub branch: String,
        pub commit: String,
        pub coverage: CoverageNew,
    }

    pub async fn fetch_repo_coverage_reports(
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
}
