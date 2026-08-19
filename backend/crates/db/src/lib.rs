use anyhow::Result;
use config::Settings;
use sqlx::{postgres::PgPoolOptions, PgPool};

pub type DbPool = PgPool;

pub async fn connect(settings: &Settings) -> Result<DbPool> {
    let pool = PgPoolOptions::new()
        .max_connections(settings.db_max_connections)
        .min_connections(settings.db_min_connections)
        .acquire_timeout(settings.db_acquire_timeout())
        .connect(&settings.database_url)
        .await?;
    Ok(pool)
}

pub async fn migrate(pool: &DbPool) -> Result<()> {
    sqlx::migrate!("../../migrations").run(pool).await?;
    Ok(())
}

pub async fn ping(pool: &DbPool) -> Result<()> {
    sqlx::query_scalar::<_, i32>("select 1")
        .fetch_one(pool)
        .await?;
    Ok(())
}
