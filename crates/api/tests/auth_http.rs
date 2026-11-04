use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use corqty_api::{config::ApiConfig, router::build_router, state::AppState};
use corqty_persistence::store::PgStore;
use corqty_test_support::TestDatabase;
use serial_test::serial;
use tower::ServiceExt;

#[tokio::test]
#[serial]
async fn register_sets_session_and_me_authenticates_it() {
    let database = TestDatabase::reset().await;
    let state = AppState::new(PgStore::new(database.api), ApiConfig::for_tests());
    let app = build_router(state);
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/register")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"email":"admin@example.com","password":"correct horse battery staple","organizationName":"Acme"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let cookies: Vec<_> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap().to_owned())
        .collect();
    let session = cookies
        .iter()
        .find(|cookie| cookie.starts_with("corqty_session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();

    let me = app
        .oneshot(
            Request::get("/api/v1/auth/me")
                .header(header::COOKIE, session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(me.status(), StatusCode::OK);
    let body = to_bytes(me.into_body(), 64 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["user"]["email"], "admin@example.com");
    assert_eq!(json["organizations"][0]["role"], "admin");
    assert_eq!(
        json["organizations"][0]["capabilities"],
        serde_json::json!([
            "view_organization",
            "edit_workflows",
            "publish_workflows",
            "run_workflows",
            "manage_members",
            "manage_service_tokens",
            "manage_secrets",
            "manage_plugins",
            "enable_shell_executor"
        ])
    );
}

#[tokio::test]
#[serial]
async fn mutating_session_route_rejects_missing_csrf_header() {
    let database = TestDatabase::reset().await;
    let state = AppState::new(PgStore::new(database.api), ApiConfig::for_tests());
    let app = build_router(state);

    let registration = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/register")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"email":"admin@example.com","password":"correct horse battery staple","organizationName":"Acme"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let cookies: Vec<_> = registration
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| {
            value
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_owned()
        })
        .collect();
    let cookie_header = cookies.join("; ");
    let csrf = cookies
        .iter()
        .find_map(|cookie| cookie.strip_prefix("corqty_csrf="))
        .unwrap()
        .to_owned();

    let rejected = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/logout")
                .header(header::COOKIE, &cookie_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);

    let accepted = app
        .oneshot(
            Request::post("/api/v1/auth/logout")
                .header(header::COOKIE, cookie_header)
                .header("x-csrf-token", csrf)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
#[serial]
async fn invalid_login_is_unauthorized() {
    let database = TestDatabase::reset().await;
    let app = build_router(AppState::new(
        PgStore::new(database.api),
        ApiConfig::for_tests(),
    ));
    let response = app
        .oneshot(
            Request::post("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"email":"missing@example.com","password":"wrong password value"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
