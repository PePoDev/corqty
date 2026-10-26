#![forbid(unsafe_code)]

pub mod audit;
pub mod auth;
pub mod authz;
pub mod catalog;
pub mod config;
pub mod error;
pub mod extract;
pub mod organizations;
pub mod router;
pub mod runs;
pub mod state;
pub mod workflows;

/// Serves the API on `listener` until `state.shutdown` is cancelled, then lets open
/// requests finish.
pub async fn serve(
    listener: tokio::net::TcpListener,
    state: state::AppState,
) -> std::io::Result<()> {
    let shutdown = state.shutdown.clone().cancelled_owned();
    axum::serve(listener, router::build_router(state))
        .with_graceful_shutdown(shutdown)
        .await
}
