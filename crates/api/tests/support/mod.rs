#![allow(dead_code)]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use corqty_api::{config::ApiConfig, router::build_router, state::AppState};
use corqty_persistence::store::PgStore;
use corqty_test_support::TestDatabase;
use tower::ServiceExt;
use uuid::Uuid;

pub struct AuthFixture {
    pub app: Router,
    pub csrf: String,
    pub user_id: Uuid,
    pub organization_id: Uuid,
    cookies: String,
}

impl AuthFixture {
    pub async fn registered(email: &str, organization_name: &str) -> Self {
        let database = TestDatabase::reset().await;
        let app = build_router(AppState::new(
            PgStore::new(database.api),
            ApiConfig::for_tests(),
        ));
        Self::register_on(app, email, organization_name).await
    }

    pub async fn register_another(&self, email: &str, organization_name: &str) -> Self {
        Self::register_on(self.app.clone(), email, organization_name).await
    }

    pub async fn login(&self, email: &str, password: &str) -> Self {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::post("/api/v1/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "email": email,
                            "password": password,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        Self::from_auth_response(self.app.clone(), response).await
    }

    async fn register_on(app: Router, email: &str, organization_name: &str) -> Self {
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/v1/auth/register")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "email": email,
                            "password": "correct horse battery staple",
                            "organizationName": organization_name,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        Self::from_auth_response(app, response).await
    }

    async fn from_auth_response(app: Router, response: Response) -> Self {
        let cookie_pairs: Vec<String> = response
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
        let csrf = cookie_pairs
            .iter()
            .find_map(|cookie| cookie.strip_prefix("corqty_csrf="))
            .unwrap()
            .to_owned();
        let cookies = cookie_pairs.join("; ");

        let me = app
            .clone()
            .oneshot(
                Request::get("/api/v1/auth/me")
                    .header(header::COOKIE, &cookies)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(me.status(), StatusCode::OK);
        let body = to_bytes(me.into_body(), 64 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        Self {
            app,
            csrf,
            user_id: json["user"]["id"].as_str().unwrap().parse().unwrap(),
            organization_id: json["organizations"][0]["organizationId"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap(),
            cookies,
        }
    }

    pub fn cookie_header(&self) -> &str {
        &self.cookies
    }

    pub async fn get(&self, path: &str) -> Response {
        self.app
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::COOKIE, &self.cookies)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    pub async fn post_json(&self, path: &str, json: &str) -> Response {
        self.request_json(Method::POST, path, json).await
    }

    pub async fn patch_json(&self, path: &str, json: &str) -> Response {
        self.request_json(Method::PATCH, path, json).await
    }

    pub async fn delete(&self, path: &str) -> Response {
        self.app
            .clone()
            .oneshot(
                Request::delete(path)
                    .header(header::COOKIE, &self.cookies)
                    .header("x-csrf-token", &self.csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    pub async fn request_json(&self, method: Method, path: &str, json: &str) -> Response {
        self.app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, &self.cookies)
                    .header("x-csrf-token", &self.csrf)
                    .body(Body::from(json.to_owned()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
}
