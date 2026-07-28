mod support;

use axum::{body::to_bytes, http::StatusCode};
use serial_test::serial;
use support::AuthFixture;

#[tokio::test]
#[serial]
async fn authentication_and_invitation_write_safe_audit_events() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let second_session = admin
        .login("admin@example.com", "correct horse battery staple")
        .await;
    let changed = second_session
        .patch_json(
            "/api/v1/auth/password",
            r#"{
                "currentPassword":"correct horse battery staple",
                "newPassword":"a newer correct horse battery staple"
            }"#,
        )
        .await;
    assert_eq!(changed.status(), StatusCode::NO_CONTENT);

    let sessions_before = second_session.get("/api/v1/auth/sessions").await;
    let sessions_before_body = to_bytes(sessions_before.into_body(), 64 * 1024)
        .await
        .unwrap();
    let sessions_before_json: serde_json::Value =
        serde_json::from_slice(&sessions_before_body).unwrap();
    let current_session_id = sessions_before_json[0]["id"].as_str().unwrap().to_owned();

    let third_session = second_session
        .login("admin@example.com", "a newer correct horse battery staple")
        .await;

    let sessions_after = second_session.get("/api/v1/auth/sessions").await;
    let sessions_after_body = to_bytes(sessions_after.into_body(), 64 * 1024)
        .await
        .unwrap();
    let sessions_after_json: serde_json::Value =
        serde_json::from_slice(&sessions_after_body).unwrap();
    let third_session_id = sessions_after_json
        .as_array()
        .unwrap()
        .iter()
        .find_map(|session| {
            let id = session["id"].as_str()?;
            (id != current_session_id).then(|| id.to_owned())
        })
        .unwrap();
    let revoked = second_session
        .delete(&format!("/api/v1/auth/sessions/{third_session_id}"))
        .await;
    assert_eq!(revoked.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        third_session.get("/api/v1/auth/me").await.status(),
        StatusCode::UNAUTHORIZED,
    );

    let logout_session = second_session
        .login("admin@example.com", "a newer correct horse battery staple")
        .await;
    let logged_out = logout_session.post_json("/api/v1/auth/logout", "{}").await;
    assert_eq!(logged_out.status(), StatusCode::NO_CONTENT);

    let invite = second_session
        .post_json(
            &format!("/api/v1/orgs/{}/invitations", admin.organization_id),
            r#"{"email":"viewer@example.com","role":"viewer"}"#,
        )
        .await;
    assert_eq!(invite.status(), StatusCode::CREATED);
    let invite_body = to_bytes(invite.into_body(), 64 * 1024).await.unwrap();
    let invite_json: serde_json::Value = serde_json::from_slice(&invite_body).unwrap();
    let token = invite_json["token"].as_str().unwrap().to_owned();

    let response = second_session
        .get(&format!(
            "/api/v1/orgs/{}/audit-events",
            admin.organization_id
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let actions: Vec<_> = json["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"organization.created"));
    assert!(actions.contains(&"account.registered"));
    assert!(actions.contains(&"auth.login"));
    assert!(actions.contains(&"account.password_changed"));
    assert!(actions.contains(&"session.revoked"));
    assert!(actions.contains(&"auth.logout"));
    assert!(actions.contains(&"invitation.created"));
    assert!(
        !body
            .windows(token.len())
            .any(|window| window == token.as_bytes())
    );
}
