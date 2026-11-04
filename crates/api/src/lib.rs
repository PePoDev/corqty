#![forbid(unsafe_code)]

pub mod audit;
pub mod auth;
pub mod authz;
pub mod automation;
pub mod catalog;
pub mod config;
pub mod error;
pub mod events;
pub mod extract;
pub mod limits;
pub mod loops;
pub mod organizations;
pub mod plugins;
pub mod router;
pub mod runs;
pub mod schedule;
pub mod secrets;
pub mod state;
pub mod workflows;

/// Serves the API on `listener` until `state.shutdown` is cancelled, then lets open
/// requests finish.
pub async fn serve(
    listener: tokio::net::TcpListener,
    state: state::AppState,
) -> std::io::Result<()> {
    let shutdown = state.shutdown.clone().cancelled_owned();
    axum::serve(
        listener,
        router::build_router(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
}
