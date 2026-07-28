use corqty_test_support::TestDatabase;
use serial_test::serial;
use sqlx::Row;

#[tokio::test]
#[serial]
async fn migrations_create_foundation_tables() {
    let database = TestDatabase::reset().await;
    let row = sqlx::query(
        "SELECT to_regclass('public.users')::text AS users,
                to_regclass('public.organizations')::text AS organizations,
                to_regclass('public.audit_events')::text AS audit_events",
    )
    .fetch_one(&database.owner)
    .await
    .unwrap();

    assert_eq!(row.get::<String, _>("users"), "users");
    assert_eq!(row.get::<String, _>("organizations"), "organizations");
    assert_eq!(row.get::<String, _>("audit_events"), "audit_events");
}
