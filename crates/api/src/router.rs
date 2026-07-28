use axum::{
    Router,
    http::StatusCode,
    middleware,
    routing::{get, post},
};
use tower_http::trace::TraceLayer;

use crate::{
    auth::{handlers, middleware as auth_middleware},
    state::AppState,
};

pub fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/auth/me", get(handlers::me))
        .route("/auth/logout", post(handlers::logout))
        .route(
            "/auth/password",
            axum::routing::patch(handlers::change_password),
        )
        .route("/auth/sessions", get(handlers::list_sessions))
        .route(
            "/auth/sessions/{session_id}",
            axum::routing::delete(handlers::revoke_session),
        )
        .layer(middleware::from_fn(auth_middleware::require_csrf))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware::authenticate,
        ));
    let public = Router::new()
        .route("/auth/register", post(handlers::register))
        .route("/auth/login", post(handlers::login))
        .route("/health", get(|| async { StatusCode::NO_CONTENT }));

    Router::new()
        .nest("/api/v1", public.merge(protected))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
