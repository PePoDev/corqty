mod support;

use axum::{
    body::{Body, to_bytes},
    http::{HeaderValue, Request, header},
    response::Response,
};
use serde_json::{Value, json};
use serial_test::serial;
use support::AuthFixture;
use tower::ServiceExt;

fn expected(status: u16, title: &str, code: &str) -> Value {
    json!({ "type": "about:blank", "title": title, "status": status, "code": code })
}

/// Asserts the response is a problem whose body carries the response status.
async fn problem(response: Response) -> Value {
    let status = response.status();
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).cloned(),
        Some(HeaderValue::from_static("application/problem+json")),
        "{status}"
    );
    let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let problem: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(problem["status"], status.as_u16());
    problem
}

#[tokio::test]
#[serial]
async fn rejected_json_bodies_return_problems_without_diagnostics() {
    let fixture = AuthFixture::registered("admin@example.com", "Acme").await;
    let credentials = r#"{"email":"admin@example.com","password":"correct horse battery staple"}"#;
    let oversized = format!(
        r#"{{"email":"admin@example.com","password":"{}"}}"#,
        "a".repeat(2 * 1024 * 1024)
    );
    for (content_type, body, problem_body) in [
        (
            "application/json",
            r#"{"email":"#.to_owned(),
            expected(400, "Bad request", "invalid_json"),
        ),
        (
            "application/json",
            r#"{"email":"admin@example.com","password":"correct horse battery staple","remember":true}"#.to_owned(),
            expected(422, "Validation failed", "invalid_json"),
        ),
        (
            "application/json",
            r#"{"email":"admin@example.com","password":7}"#.to_owned(),
            expected(422, "Validation failed", "invalid_json"),
        ),
        (
            "text/plain",
            credentials.to_owned(),
            expected(415, "Unsupported media type", "unsupported_media_type"),
        ),
        (
            "application/json",
            oversized,
            expected(413, "Payload too large", "payload_too_large"),
        ),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::post("/api/v1/auth/login")
                    .header(header::CONTENT_TYPE, content_type)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(problem(response).await, problem_body);
    }
}

#[tokio::test]
#[serial]
async fn unknown_routes_methods_and_identifiers_return_problems() {
    let fixture = AuthFixture::registered("admin@example.com", "Acme").await;
    for (path, problem_body) in [
        (
            "/api/v1/orgs/not-a-uuid/members",
            expected(404, "Not found", "not_found"),
        ),
        (
            "/api/v1/no-such-route",
            expected(404, "Not found", "not_found"),
        ),
        (
            "/api/v1/auth/login",
            expected(405, "Method not allowed", "method_not_allowed"),
        ),
    ] {
        assert_eq!(
            problem(fixture.get(path).await).await,
            problem_body,
            "{path}"
        );
    }
}
