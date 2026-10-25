use corqty_test_support::TestDatabase;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn migrations_run_as_a_non_superuser_owner() {
    let database = TestDatabase::reset().await;

    let role: (String, bool, bool) = sqlx::query_as(
        "SELECT rolname, rolsuper, rolbypassrls FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&database.owner)
    .await
    .unwrap();
    // BYPASSRLS keeps the SECURITY DEFINER helpers working: they read tables
    // with FORCE ROW LEVEL SECURITY whose policies only name corqty_api.
    assert_eq!(role, ("corqty_owner".to_owned(), false, true));

    let owners: Vec<String> = sqlx::query_scalar(
        "SELECT tableowner FROM pg_tables WHERE schemaname = 'public'
         UNION
         SELECT pg_get_userbyid(proowner) FROM pg_proc
         WHERE pronamespace = 'corqty'::regnamespace",
    )
    .fetch_all(&database.owner)
    .await
    .unwrap();
    assert_eq!(owners, ["corqty_owner"]);
}
