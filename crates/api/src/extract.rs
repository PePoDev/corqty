use axum::{
    extract::{
        FromRequest, FromRequestParts,
        rejection::{JsonRejection, PathRejection},
    },
    http::StatusCode,
};

use crate::error::ApiError;

/// `axum::Json` with problem+json rejections: 400 `invalid_json` for a syntax
/// error, 422 `invalid_json` for a body that does not fit the type, 415 for a
/// missing JSON content type and 413 above axum's 2 MiB default body limit.
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct ApiJson<T>(pub T);

/// `axum::extract::Path` whose rejection is a 404, so a malformed identifier
/// looks exactly like an unknown one.
#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub struct ApiPath<T>(pub T);

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        match rejection.status() {
            StatusCode::PAYLOAD_TOO_LARGE => Self::PayloadTooLarge,
            StatusCode::UNSUPPORTED_MEDIA_TYPE => Self::UnsupportedMediaType,
            StatusCode::UNPROCESSABLE_ENTITY => Self::Validation("invalid_json"),
            _ => Self::BadRequest("invalid_json"),
        }
    }
}

impl From<PathRejection> for ApiError {
    fn from(_: PathRejection) -> Self {
        Self::NotFound
    }
}
