use axum::{
    Extension, Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use corqty_persistence::models::SessionRecord;
use serde::Deserialize;

use crate::{
    auth::{
        cookies::{append_auth_cookies, clear_auth_cookies},
        service,
    },
    error::ApiError,
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    email: String,
    password: String,
    organization_name: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

pub async fn register(
    State(state): State<AppState>,
    Json(request): Json<RegisterRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let session = service::register(
        &state,
        &request.email,
        &request.password,
        &request.organization_name,
    )
    .await?;
    let mut headers = HeaderMap::new();
    append_auth_cookies(
        &mut headers,
        &state.config,
        &session.session_token,
        &session.csrf_token,
    )?;
    Ok((
        StatusCode::CREATED,
        headers,
        Json(serde_json::json!({"ok": true})),
    ))
}

pub async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let session = service::login(&state, &request.email, &request.password).await?;
    let mut headers = HeaderMap::new();
    append_auth_cookies(
        &mut headers,
        &state.config,
        &session.session_token,
        &session.csrf_token,
    )?;
    Ok((headers, Json(serde_json::json!({"ok": true}))))
}

pub async fn me(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
) -> Result<Json<service::MeView>, ApiError> {
    Ok(Json(service::me(&state, &session).await?))
}

pub async fn logout(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .store
        .delete_session(session.user_id, session.session_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    let mut headers = HeaderMap::new();
    clear_auth_cookies(&mut headers, &state.config);
    Ok((StatusCode::NO_CONTENT, headers, ()))
}

pub async fn change_password(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<StatusCode, ApiError> {
    service::change_password(
        &state,
        &session,
        &request.current_password,
        &request.new_password,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_sessions(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
) -> Result<Json<Vec<corqty_persistence::models::SessionSummaryRecord>>, ApiError> {
    let sessions = state
        .store
        .list_sessions(session.user_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(Json(sessions))
}

pub async fn revoke_session(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    axum::extract::Path(session_id): axum::extract::Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let removed = state
        .store
        .revoke_user_session(session.user_id, session_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    removed
        .then_some(StatusCode::NO_CONTENT)
        .ok_or(ApiError::NotFound)
}
