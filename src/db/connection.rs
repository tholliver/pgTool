use anyhow::Result;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::env;
use std::time::Duration;

use crate::connections::ConnectionProfile;

pub async fn connect() -> Result<PgPool> {
    let url =
        env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL not set in .env"))?;
    connect_with_url(&url).await
}

/// Connect to the server. In browse-all mode, if the first database is missing or
/// not allowed, try `postgres` then `template1`. Auth/network errors are NOT retried.
pub async fn connect_server(profile: &ConnectionProfile, password: &str) -> Result<PgPool> {
    let mut candidates = vec![profile.maintenance_database().to_string()];
    if profile.browse_all_databases() {
        for fb in ["postgres", "template1"] {
            if !candidates.iter().any(|c| c == fb) {
                candidates.push(fb.to_string());
            }
        }
    }
    let mut first_err: Option<anyhow::Error> = None;
    for name in candidates {
        let url = profile.connection_url_for_database(password, &name);
        match connect_with_url(&url).await {
            Ok(pool) => return Ok(pool),
            Err(e) => {
                let msg = e.to_string();
                let retryable =
                    msg.contains("does not exist") || msg.contains("permission denied");
                if first_err.is_none() {
                    first_err = Some(e);
                }
                if !retryable {
                    break;
                }
            }
        }
    }
    Err(first_err.expect("at least one candidate"))
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
