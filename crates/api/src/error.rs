use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use corqty_domain::diagnostic::Diagnostic;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("bad request")]
    BadRequest(&'static str),
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("method not allowed")]
    MethodNotAllowed,
    #[error("conflict")]
    Conflict(&'static str),
    #[error("payload too large")]
    PayloadTooLarge,
    #[error("unsupported media type")]
    UnsupportedMediaType,
    #[error("validation failed")]
    Validation(&'static str),
    /// 422 whose problem body lists the diagnostics.
    #[error("validation failed")]
    Invalid {
        code: &'static str,
        diagnostics: Vec<Diagnostic>,
    },
    /// Logged when rendered; the response never contains the source.
    #[error("internal error")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl ApiError {
    /// Wraps an error that has no `From` conversion (only `sqlx::Error` has one).
    pub fn internal(source: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Self::Internal(source.into())
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        // 22P05 and 22021: PostgreSQL text and jsonb cannot hold a NUL character.
        let nul = error
            .as_database_error()
            .and_then(|database| database.code())
            .is_some_and(|code| code == "22P05" || code == "22021");
        if nul {
            return Self::Validation("unsupported_character");
        }
        Self::internal(error)
    }
}

#[derive(Serialize)]
struct Problem {
    #[serde(rename = "type")]
    kind: &'static str,
    title: &'static str,
    status: u16,
    code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Vec<Diagnostic>>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut diagnostics = None;
        let (status, title, code) = match self {
            Self::BadRequest(code) => (StatusCode::BAD_REQUEST, "Bad request", code),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized", "unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "Forbidden", "forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "Not found", "not_found"),
            Self::MethodNotAllowed => (
                StatusCode::METHOD_NOT_ALLOWED,
                "Method not allowed",
                "method_not_allowed",
            ),
            Self::Conflict(code) => (StatusCode::CONFLICT, "Conflict", code),
            Self::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "Payload too large",
                "payload_too_large",
            ),
            Self::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Unsupported media type",
                "unsupported_media_type",
            ),
            Self::Validation(code) => (StatusCode::UNPROCESSABLE_ENTITY, "Validation failed", code),
            Self::Invalid {
                code,
                diagnostics: found,
            } => {
                diagnostics = Some(found);
                (StatusCode::UNPROCESSABLE_ENTITY, "Validation failed", code)
            }
            Self::Internal(source) => {
                tracing::error!(error = %source, "request failed with an internal error");
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
                diagnostics,
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use axum::{body::to_bytes, http::StatusCode, response::IntoResponse};
    use corqty_domain::diagnostic::Diagnostic;
    use serde_json::{Value, json};

    use super::ApiError;

    async fn render(error: ApiError) -> (StatusCode, String) {
        let response = error.into_response();
        let status = response.status();
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn invalid_problems_list_their_diagnostics() {
        let (status, body) = render(ApiError::Invalid {
            code: "graph_invalid",
            diagnostics: vec![Diagnostic {
                code: "too_many_nodes".to_owned(),
                pointer: "/nodes".to_owned(),
                message: "a graph holds at most 500 nodes".to_owned(),
            }],
        })
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({
                "type": "about:blank",
                "title": "Validation failed",
                "status": 422,
                "code": "graph_invalid",
                "diagnostics": [{
                    "code": "too_many_nodes",
                    "pointer": "/nodes",
                    "message": "a graph holds at most 500 nodes",
                }],
            })
        );
    }

    #[tokio::test]
    async fn internal_problems_never_include_their_source() {
        let source = sqlx::Error::Protocol("connection to db.internal:5432 refused".to_owned());
        let (status, body) = render(ApiError::from(source)).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!body.contains("db.internal"), "{body}");
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({
                "type": "about:blank",
                "title": "Internal error",
                "status": 500,
                "code": "internal",
            })
        );
    }
}
