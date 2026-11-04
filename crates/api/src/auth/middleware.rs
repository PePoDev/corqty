use axum::{
    extract::{Request, State},
    http::Method,
    middleware::Next,
    response::Response,
};
use corqty_persistence::{
    automation::{self, ServicePrincipal},
    models::SessionRecord,
};

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
    if let Some(authorization) = request.headers().get("authorization") {
        let token = authorization
            .to_str()
            .ok()
            .and_then(|value| value.strip_prefix("Bearer "))
            .filter(|token| token.len() <= 200)
            .ok_or(ApiError::Unauthorized)?;
        let parts: Vec<_> = request.uri().path().trim_matches('/').split('/').collect();
        // Nesting strips /api/v1 in some router contexts; identify only this exact shape.
        let parts = if parts.starts_with(&["api", "v1"]) {
            &parts[2..]
        } else {
            &parts[..]
        };
        if request.method() != Method::POST
            || parts.len() != 5
            || parts[0] != "orgs"
            || parts[2] != "workflows"
            || parts[4] != "runs"
        {
            return Err(ApiError::Forbidden);
        }
        let org: uuid::Uuid = parts[1].parse().map_err(|_| ApiError::NotFound)?;
        let principal =
            automation::authenticate_token(state.store.pool(), &SecretToken::hash(token))
                .await?
                .ok_or(ApiError::Unauthorized)?;
        if principal.organization_id != org {
            return Err(ApiError::NotFound);
        }
        crate::automation::check_rate(
            &state,
            &format!("token:{}", principal.id),
            crate::limits::bounded("CORQTY_TOKEN_RATE_PER_MINUTE", 120, 10000),
            60,
        )
        .await?;
        request.extensions_mut().insert(principal);
        return Ok(next.run(request).await);
    }
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
    if request.extensions().get::<ServicePrincipal>().is_some() {
        return Ok(next.run(request).await);
    }
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
