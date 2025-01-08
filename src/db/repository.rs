use sqlx::postgres::PgQueryResult;
use sqlx::FromRow;
use sqlx::PgPool;
use tracing::instrument;

#[derive(Debug, FromRow, sqlx::Type)]
#[sqlx(transparent)]
pub struct RepositoryId(i32);

#[derive(Debug, FromRow)]
pub struct RepositoryTableEntry {
    id: RepositoryId,
    owner: String,
    name: String,
}

#[instrument(err, skip(db))]
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

#[instrument(err, skip(db))]
pub async fn insert(
    db: &PgPool,
    owner: &str,
    repository: &str,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query("INSERT INTO repository(owner, name) VALUES ($1, $2);")
        .bind(owner)
        .bind(repository)
        .execute(db)
        .await
}

#[instrument(err, skip(db))]
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
