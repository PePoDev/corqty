use axum::{
    Router,
    http::StatusCode,
    middleware,
    routing::{get, post, put},
};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::{
    audit,
    auth::{handlers, middleware as auth_middleware},
    catalog,
    error::ApiError,
    organizations,
    state::AppState,
    workflows,
};

pub fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/auth/me", get(handlers::me))
        .route("/auth/logout", post(handlers::logout))
        .route("/organizations", post(organizations::handlers::create))
        .route(
            "/orgs/{organization_id}/invitations",
            post(organizations::members::create_invitation),
        )
        .route(
            "/invitations/accept",
            post(organizations::members::accept_invitation),
        )
        .route(
            "/orgs/{organization_id}/members",
            get(organizations::members::list_members),
        )
        .route("/orgs/{organization_id}/audit-events", get(audit::list))
        .route(
            "/orgs/{organization_id}/members/{target_user_id}",
            axum::routing::patch(organizations::members::change_role)
                .delete(organizations::members::remove_member),
        )
        .route(
            "/orgs/{organization_id}/node-definitions",
            get(catalog::list_node_definitions).post(catalog::create_node_definition),
        )
        .route(
            "/orgs/{organization_id}/node-definitions/{definition_id}/deprecate",
            post(catalog::deprecate_node_definition),
        )
        .route(
            "/orgs/{organization_id}/blueprints",
            get(catalog::list_blueprints).post(catalog::create_blueprint),
        )
        .route(
            "/orgs/{organization_id}/workflows",
            get(workflows::list_workflows).post(workflows::create_workflow),
        )
        .route(
            "/orgs/{organization_id}/workflows/{workflow_id}",
            get(workflows::get_workflow),
        )
        .route(
            "/orgs/{organization_id}/workflows/{workflow_id}/draft",
            put(workflows::save_draft),
        )
        .route(
            "/orgs/{organization_id}/workflows/{workflow_id}/validate",
            post(workflows::validate),
        )
        .route(
            "/orgs/{organization_id}/workflows/{workflow_id}/versions",
            get(workflows::list_versions),
        )
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
        // `method_not_allowed_fallback` only reaches routes added before it.
        .fallback(|| async { ApiError::NotFound })
        .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}
