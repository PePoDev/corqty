#![forbid(unsafe_code)]

use std::sync::Arc;

use corqty_executor_mock::MockExecutor;
use corqty_executor_shell::ShellExecutor;
use corqty_plugin_host::Registry;
use corqty_plugin_sdk::Executor;
use corqty_worker::{WorkerConfig, check_alive, run};
use sqlx::postgres::PgPoolOptions;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The Compose healthcheck reads only the alive file that the running worker touches.
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        return Ok(check_alive(&WorkerConfig::default().alive_file)?);
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "corqty_worker=info".into()),
        )
        .init();
    let database_url =
        std::env::var("DATABASE_URL_WORKER").map_err(|_| "DATABASE_URL_WORKER must be set")?;
    let mut config = WorkerConfig::default();
    if let Ok(capacity) = std::env::var("CORQTY_WORKER_CAPACITY") {
        config.capacity = capacity
            .parse()
            .ok()
            .filter(|capacity| *capacity >= 1)
            .ok_or("CORQTY_WORKER_CAPACITY must be a whole number of at least 1")?;
    }
    let executors: Vec<Arc<dyn Executor>> = vec![Arc::new(MockExecutor), Arc::new(ShellExecutor)];
    let registry = Registry::new(executors)?;
    // One connection for each running attempt, and one to claim with.
    let pool = PgPoolOptions::new()
        .max_connections(config.capacity.unsigned_abs() + 1)
        .connect(&database_url)
        .await?;
    tracing::info!(
        worker_id = %config.worker_id,
        capacity = config.capacity,
        executors = ?registry.advertised(),
        "the worker is ready"
    );
    let shutdown = CancellationToken::new();
    let signal = shutdown.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        signal.cancel();
    });
    run(config, registry, pool, shutdown).await?;
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
