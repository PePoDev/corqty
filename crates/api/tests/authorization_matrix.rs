mod support;

use axum::{
    body::to_bytes,
    http::{Method, StatusCode},
};
use chrono::Utc;
use corqty_api::{authz::authorize, config::ApiConfig, error::ApiError, state::AppState};
use corqty_domain::rbac::{Capability, Role};
use corqty_persistence::{models::SessionRecord, store::PgStore};
use corqty_test_support::TestDatabase;
use serial_test::serial;
use support::AuthFixture;
use uuid::Uuid;

/// An admin's organization with an editor, a runner and a viewer who joined
/// through invitations, plus an outsider who only belongs to its own org.
struct Matrix {
    organization_id: Uuid,
    admin_user_id: Uuid,
    members: Vec<(Role, AuthFixture)>,
    outsider: AuthFixture,
}

impl Matrix {
    async fn new() -> Self {
        let admin = AuthFixture::registered("admin@example.com", "Acme").await;
        let mut members = Vec::new();
        for role in [Role::Editor, Role::Runner, Role::Viewer] {
            let email = format!("{role}@example.com");
            let invitation = admin
                .post_json(
                    &format!("/api/v1/orgs/{}/invitations", admin.organization_id),
                    &serde_json::json!({ "email": email, "role": role.as_str() }).to_string(),
                )
                .await;
            assert_eq!(invitation.status(), StatusCode::CREATED);
            let body = to_bytes(invitation.into_body(), 64 * 1024).await.unwrap();
            let invitation: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let member = admin.register_another(&email, "Personal").await;
            let accepted = member
                .post_json(
                    "/api/v1/invitations/accept",
                    &serde_json::json!({ "token": invitation["token"] }).to_string(),
                )
                .await;
            assert_eq!(accepted.status(), StatusCode::OK);
            members.push((role, member));
        }
        let outsider = admin
            .register_another("outsider@example.com", "Outside")
            .await;
        let organization_id = admin.organization_id;
        let admin_user_id = admin.user_id;
        members.insert(0, (Role::Admin, admin));
        Self {
            organization_id,
            admin_user_id,
            members,
            outsider,
        }
    }

    /// Reads must succeed for every role holding the capability; writes are
    /// only asserted for roles lacking it, so no row changes shared state.
    async fn assert_row(&self, method: Method, path: &str, body: &str, capability: Capability) {
        for (role, member) in &self.members {
            let expected = match (role.allows(capability), method == Method::GET) {
                (false, _) => StatusCode::FORBIDDEN,
                (true, true) => StatusCode::OK,
                (true, false) => continue,
            };
            let response = member.request_json(method.clone(), path, body).await;
            assert_eq!(response.status(), expected, "{role} {method} {path}");
        }
        let response = self.outsider.request_json(method.clone(), path, body).await;
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "outsider {method} {path}"
        );
    }
}

#[tokio::test]
#[serial]
async fn member_and_audit_routes_enforce_capabilities() {
    let matrix = Matrix::new().await;
    let org = format!("/api/v1/orgs/{}", matrix.organization_id);
    let member = format!("{org}/members/{}", matrix.admin_user_id);
    for (method, path, body, capability) in [
        (
            Method::GET,
            format!("{org}/members"),
            "",
            Capability::ViewOrganization,
        ),
        (
            Method::GET,
            format!("{org}/audit-events"),
            "",
            Capability::ViewOrganization,
        ),
        (
            Method::POST,
            format!("{org}/invitations"),
            r#"{"email":"new@example.com","role":"viewer"}"#,
            Capability::ManageMembers,
        ),
        (
            Method::PATCH,
            member.clone(),
            r#"{"role":"viewer"}"#,
            Capability::ManageMembers,
        ),
        (Method::DELETE, member, "", Capability::ManageMembers),
    ] {
        matrix.assert_row(method, &path, body, capability).await;
    }
}

#[tokio::test]
#[serial]
async fn org_access_commit_reports_the_deferred_final_admin_check_as_conflict() {
    let database = TestDatabase::reset().await;
    let user_id = Uuid::new_v4();
    let organization_id = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id, email) VALUES ($1, 'admin@example.com')")
        .bind(user_id)
        .execute(&database.owner)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO organizations(id, name, slug, created_by)
         VALUES ($1, 'Acme', 'acme', $2)",
    )
    .bind(organization_id)
    .bind(user_id)
    .execute(&database.owner)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO memberships(organization_id, user_id, role)
         VALUES ($1, $2, 'admin')",
    )
    .bind(organization_id)
    .bind(user_id)
    .execute(&database.owner)
    .await
    .unwrap();
    let state = AppState::new(PgStore::new(database.api), ApiConfig::for_tests());
    let session = SessionRecord {
        session_id: Uuid::new_v4(),
        user_id,
        email: "admin@example.com".to_owned(),
        csrf_hash: Vec::new(),
        expires_at: Utc::now(),
    };

    let mut access = authorize(&state, &session, organization_id, Capability::ManageMembers)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE memberships SET role = 'viewer'
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .execute(&mut *access.tx)
    .await
    .unwrap();

    assert!(matches!(
        access.commit().await,
        Err(ApiError::Conflict("final_admin_required"))
    ));
}
