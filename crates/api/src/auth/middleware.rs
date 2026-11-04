use axum::{
    extract::{Request, State},
    http::Method,
    middleware::Next,
    response::Response,
};
use corqty_persistence::models::SessionRecord;

use crate::{
    auth::{
        cookies::{CSRF_COOKIE, SESSION_COOKIE, read_cookie},
        crypto::SecretToken,
    },
    error::ApiError,
    state::AppState,
};

pub async fn authenticate(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = read_cookie(request.headers(), SESSION_COOKIE).ok_or(ApiError::Unauthorized)?;
    let session = state
        .store
        .session_by_token_hash(&SecretToken::hash(&token))
        .await?
        .ok_or(ApiError::Unauthorized)?;
    request.extensions_mut().insert(session);
    Ok(next.run(request).await)
}

pub async fn require_csrf(request: Request, next: Next) -> Result<Response, ApiError> {
    if matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        let session = request
            .extensions()
            .get::<SessionRecord>()
            .ok_or(ApiError::Unauthorized)?;
        let cookie = read_cookie(request.headers(), CSRF_COOKIE).ok_or(ApiError::Forbidden)?;
        let header_token = request
            .headers()
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok())
            .ok_or(ApiError::Forbidden)?;
        if cookie != header_token || SecretToken::hash(header_token) != session.csrf_hash {
            return Err(ApiError::Forbidden);
        }
    }
    Ok(next.run(request).await)
}
