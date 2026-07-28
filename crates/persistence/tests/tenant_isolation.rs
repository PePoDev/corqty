use corqty_persistence::tenant::begin_tenant;
use corqty_test_support::TestDatabase;
use serial_test::serial;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn api_role_cannot_read_another_organization() {
    let database = TestDatabase::reset().await;
    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();
    let org_a = Uuid::new_v4();
    let org_b = Uuid::new_v4();

    for (user, email) in [(user_a, "a@example.com"), (user_b, "b@example.com")] {
        sqlx::query("INSERT INTO users(id, email) VALUES ($1, $2)")
            .bind(user)
            .bind(email)
            .execute(&database.owner)
            .await
            .unwrap();
    }
    for (org, user, slug) in [(org_a, user_a, "a"), (org_b, user_b, "b")] {
        sqlx::query(
            "INSERT INTO organizations(id, name, slug, created_by)
             VALUES ($1, 'Test', $2, $3)",
        )
        .bind(org)
        .bind(slug)
        .bind(user)
        .execute(&database.owner)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships(organization_id, user_id, role)
             VALUES ($1, $2, 'admin')",
        )
        .bind(org)
        .bind(user)
        .execute(&database.owner)
        .await
        .unwrap();
    }

    let mut tx = begin_tenant(&database.api, user_a, org_a).await.unwrap();
    let rows = sqlx::query("SELECT id FROM organizations ORDER BY id")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<Uuid, _>("id"), org_a);

    let rows_without_context = sqlx::query("SELECT id FROM organizations")
        .fetch_all(&database.api)
        .await
        .unwrap();
    assert!(rows_without_context.is_empty());

    let (current_user, bypass_rls, superuser): (String, bool, bool) = sqlx::query_as(
        "SELECT rolname, rolbypassrls, rolsuper
         FROM pg_roles
         WHERE rolname = current_user",
    )
    .fetch_one(&database.api)
    .await
    .unwrap();
    assert_eq!(current_user, "corqty_api");
    assert!(!bypass_rls);
    assert!(!superuser);

    let table_owner: String = sqlx::query_scalar(
        "SELECT pg_get_userbyid(relowner)
         FROM pg_class
         WHERE oid = 'public.organizations'::regclass",
    )
    .fetch_one(&database.api)
    .await
    .unwrap();
    assert_ne!(table_owner, current_user);
}
