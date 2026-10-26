#![forbid(unsafe_code)]

use corqty_api::{config::ApiConfig, events, loops, serve, state::AppState};
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
    let state = AppState::new(PgStore::new(pool), config);
    let background = loops::spawn(&state);
    let run_events = events::spawn_listener(
        state.config.database_url.clone(),
        state.wake.clone(),
        state.shutdown.clone(),
    );
    let shutdown = state.shutdown.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        tracing::info!("shutting down: finishing open requests");
        shutdown.cancel();
    });
    serve(listener, state).await?;
    background.join_all().await;
    run_events.await?;
    Ok(())
}

/// Resolves on ctrl-c, or on SIGTERM (what `docker compose stop` sends) on unix.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install the ctrl-c handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install the SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}
