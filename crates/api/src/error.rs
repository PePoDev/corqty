use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("conflict")]
    Conflict(&'static str),
    #[error("validation failed")]
    Validation(&'static str),
    #[error("internal error")]
    Internal,
}

#[derive(Serialize)]
struct Problem {
    #[serde(rename = "type")]
    kind: &'static str,
    title: &'static str,
    status: u16,
    code: &'static str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, title, code) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized", "unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "Forbidden", "forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "Not found", "not_found"),
            Self::Conflict(code) => (StatusCode::CONFLICT, "Conflict", code),
            Self::Validation(code) => (StatusCode::UNPROCESSABLE_ENTITY, "Validation failed", code),
            Self::Internal => {
                tracing::error!("request failed with an internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal error",
                    "internal",
                )
            }
        };
        (
            status,
            [(axum::http::header::CONTENT_TYPE, "application/problem+json")],
            Json(Problem {
                kind: "about:blank",
                title,
                status: status.as_u16(),
                code,
            }),
        )
            .into_response()
    }
}
