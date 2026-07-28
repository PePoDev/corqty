use chrono::{Duration, Utc};
use corqty_persistence::{
    auth::{NewRegistration, NewSession},
    store::PgStore,
};
use corqty_test_support::TestDatabase;
use serial_test::serial;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn registration_creates_the_first_admin_and_session() {
    let database = TestDatabase::reset().await;
    let store = PgStore::new(database.api.clone());
    let user_id = Uuid::new_v4();
    let organization_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();

    let result = store
        .register(
            NewRegistration {
                user_id,
                email: "admin@example.com".to_owned(),
                password_hash: "argon-hash".to_owned(),
                organization_id,
                organization_name: "Acme Cloud".to_owned(),
                organization_slug: "acme-cloud-a1b2c3".to_owned(),
            },
            NewSession {
                session_id,
                token_hash: vec![1; 32],
                csrf_hash: vec![2; 32],
                expires_at: Utc::now() + Duration::days(30),
            },
        )
        .await
        .unwrap();

    assert_eq!(result.user_id, user_id);
    assert_eq!(result.organization_id, organization_id);
    assert_eq!(result.role, "admin");

    let session = store
        .session_by_token_hash(&[1; 32])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.user_id, user_id);
    assert_eq!(session.email, "admin@example.com");
}

#[tokio::test]
#[serial]
async fn duplicate_registration_returns_conflict() {
    let database = TestDatabase::reset().await;
    let store = PgStore::new(database.api.clone());

    for suffix in [1_u8, 2_u8] {
        let result = store
            .register(
                NewRegistration {
                    user_id: Uuid::new_v4(),
                    email: "same@example.com".to_owned(),
                    password_hash: "argon-hash".to_owned(),
                    organization_id: Uuid::new_v4(),
                    organization_name: "Acme".to_owned(),
                    organization_slug: format!("acme-{suffix}"),
                },
                NewSession {
                    session_id: Uuid::new_v4(),
                    token_hash: vec![suffix; 32],
                    csrf_hash: vec![suffix + 10; 32],
                    expires_at: Utc::now() + Duration::days(30),
                },
            )
            .await;

        if suffix == 1 {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }
    }
}
