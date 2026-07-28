#![forbid(unsafe_code)]

use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL_OWNER").expect("DATABASE_URL_OWNER must be set");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await?;
    corqty_persistence::migrate::run(&pool).await?;
    Ok(())
}
