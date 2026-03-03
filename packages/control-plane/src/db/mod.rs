use anyhow::Result;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::time::Duration;

pub mod models;

pub async fn init_db(database_url: &str) -> Result<PgPool> {
    let mut retries = 10;
    
    let pool = loop {
        match PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await 
        {
            Ok(pool) => break pool,
            Err(e) => {
                if retries == 0 {
                    return Err(e.into());
                }
                tracing::warn!("Failed to connect to DB, retrying... ({})", e);
                tokio::time::sleep(Duration::from_secs(3)).await;
                retries -= 1;
            }
        }
    };

    tracing::info!("Running database migrations...");
    sqlx::migrate!("./src/db/migrations").run(&pool).await?;
    tracing::info!("Migrations completed successfully");

    Ok(pool)
}
