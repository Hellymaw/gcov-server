use lazy_static::lazy_static;
use sqlx::PgPool;
use sqlx::Pool;
use sqlx::Postgres;
use thiserror::Error;
use tracing::instrument;

pub mod reports;
pub mod repository;
pub mod summary;

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
#[instrument]
fn fetch_env_var_exiting(key: &str) -> String {
    match std::env::var(key) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(
                "${} {}. This is required for the program to function.",
                key,
                e
            );
            ::std::process::exit(2);
        }
    }
}

/// Connects to the DB instance and performs any required setup (Like creating tables etc).
pub async fn connect_and_setup() -> Result<Pool<Postgres>, sqlx::Error> {
    let db_pool = PgPool::connect(&CONNECTION_URL).await?;

    let _ = repository::setup_table(&db_pool).await?;
    let _ = summary::setup_table(&db_pool).await?;
    let _ = reports::setup_table(&db_pool).await?;

    Ok(db_pool)
}
