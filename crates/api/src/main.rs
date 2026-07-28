#![forbid(unsafe_code)]

use corqty_api::{config::ApiConfig, router::build_router, state::AppState};
use corqty_persistence::store::PgStore;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "corqty_api=info,tower_http=info".into()),
        )
        .init();
    let config = ApiConfig::from_env()?;
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&config.database_url)
        .await?;
    let listener = tokio::net::TcpListener::bind(&config.bind_address).await?;
    axum::serve(
        listener,
        build_router(AppState::new(PgStore::new(pool), config)),
    )
    .await?;
    Ok(())
}
