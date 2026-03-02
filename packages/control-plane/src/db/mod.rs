use anyhow::Result;
use sqlx::{postgres::PgPoolOptions, PgPool};

pub async fn init_db(database_url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;

    // Typically you would run migrations here, e.g. sqlx::migrate!().await?;
    // For MV speed, we will assume they're run out of band or included.
    // If we wanted to run them inline:
    // sqlx::migrate!("./src/db/migrations").run(&pool).await?;

    Ok(pool)
}
