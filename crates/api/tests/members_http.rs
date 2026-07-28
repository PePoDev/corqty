mod support;

use axum::{body::to_bytes, http::StatusCode};
use chrono::{Duration, Utc};
use serial_test::serial;
use support::AuthFixture;

#[tokio::test]
#[serial]
async fn admin_can_invite_and_matching_user_can_accept() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let invite_response = admin
        .post_json(
            &format!("/api/v1/orgs/{}/invitations", admin.organization_id),
            r#"{"email":"editor@example.com","role":"editor"}"#,
        )
        .await;
    assert_eq!(invite_response.status(), StatusCode::CREATED);
    let body = to_bytes(invite_response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let invite: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let token = invite["token"].as_str().unwrap().to_owned();
    let expires_at = invite["expiresAt"]
        .as_str()
        .unwrap()
        .parse::<chrono::DateTime<Utc>>()
        .unwrap();
    assert!(expires_at > Utc::now() + Duration::days(6));
    assert!(expires_at < Utc::now() + Duration::days(8));

    let wrong_user = admin
        .register_another("wrong@example.com", "Wrong Personal")
        .await;
    let mismatch = wrong_user
        .post_json(
            "/api/v1/invitations/accept",
            &serde_json::json!({ "token": token.clone() }).to_string(),
        )
        .await;
    assert_eq!(mismatch.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let editor = admin
        .register_another("editor@example.com", "Personal")
        .await;
    let accepted = editor
        .post_json(
            "/api/v1/invitations/accept",
            &serde_json::json!({ "token": token.clone() }).to_string(),
        )
        .await;
    assert_eq!(accepted.status(), StatusCode::OK);
    let reused = editor
        .post_json(
            "/api/v1/invitations/accept",
            &serde_json::json!({ "token": token }).to_string(),
        )
        .await;
    assert_eq!(reused.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let me = editor.get("/api/v1/auth/me").await;
    let me_body = to_bytes(me.into_body(), 64 * 1024).await.unwrap();
    let me_json: serde_json::Value = serde_json::from_slice(&me_body).unwrap();
    let accepted_membership = me_json["organizations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|membership| membership["organizationId"] == admin.organization_id.to_string())
        .unwrap();
    assert_eq!(accepted_membership["role"], "editor");

    let denied = editor
        .post_json(
            &format!("/api/v1/orgs/{}/invitations", admin.organization_id),
            r#"{"email":"other@example.com","role":"viewer"}"#,
        )
        .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
#[serial]
async fn final_admin_cannot_be_demoted_or_removed() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let demote = admin
        .patch_json(
            &format!(
                "/api/v1/orgs/{}/members/{}",
                admin.organization_id, admin.user_id
            ),
            r#"{"role":"viewer"}"#,
        )
        .await;
    assert_eq!(demote.status(), StatusCode::CONFLICT);

    let remove = admin
        .delete(&format!(
            "/api/v1/orgs/{}/members/{}",
            admin.organization_id, admin.user_id
        ))
        .await;
    assert_eq!(remove.status(), StatusCode::CONFLICT);
}

#[tokio::test]
#[serial]
async fn cross_organization_identifier_returns_not_found() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let own_members = admin
        .get(&format!("/api/v1/orgs/{}/members", admin.organization_id))
        .await;
    assert_eq!(own_members.status(), StatusCode::OK);

    let outsider = admin
        .register_another("outsider@example.com", "Outside")
        .await;
    let response = outsider
        .get(&format!("/api/v1/orgs/{}/members", admin.organization_id))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
