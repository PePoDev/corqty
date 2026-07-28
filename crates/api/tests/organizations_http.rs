mod support;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use serial_test::serial;
use support::AuthFixture;
use tower::ServiceExt;

#[tokio::test]
#[serial]
async fn authenticated_user_can_create_a_second_organization() {
    let fixture = AuthFixture::registered("admin@example.com", "Acme").await;
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::post("/api/v1/organizations")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, fixture.cookie_header())
                .header("x-csrf-token", &fixture.csrf)
                .body(Body::from(r#"{"name":"Second Org"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let me = fixture
        .app
        .clone()
        .oneshot(
            Request::get("/api/v1/auth/me")
                .header(header::COOKIE, fixture.cookie_header())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(me.into_body(), 64 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["organizations"].as_array().unwrap().len(), 2);
}
