use anyhow::Result;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::env;
use std::time::Duration;

pub async fn connect() -> Result<PgPool> {
    let url = env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL not set in .env"))?;
    connect_with_url(&url).await
}

pub async fn connect_with_url(url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect(url)
        .await
        .map_err(|e| anyhow::anyhow!("DB connect failed: {}", e))?;

    tracing::info!("connected to postgres");
    Ok(pool)
}
