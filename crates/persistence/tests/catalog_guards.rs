use corqty_test_support::TestDatabase;
use serial_test::serial;
use sqlx::PgConnection;

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

/// Every row-level security mistake the catalog shows, one message each.
async fn security_violations(conn: &mut PgConnection) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT format('table %s lacks forced row-level security or a policy', c.relname)
         FROM pg_class c
         WHERE c.relnamespace = 'public'::regnamespace
           AND c.relkind IN ('r', 'p')
           -- Sign-in reads these before any tenant context exists.
           AND c.relname NOT IN ('_sqlx_migrations', 'users', 'password_credentials', 'sessions')
           AND NOT (
               c.relrowsecurity AND c.relforcerowsecurity
               AND EXISTS (SELECT FROM pg_policy pol WHERE pol.polrelid = c.oid)
           )
         UNION ALL
         SELECT format('function %s is SECURITY DEFINER without search_path', p.oid::regprocedure)
         FROM pg_proc p
         WHERE p.pronamespace = 'corqty'::regnamespace
           AND p.prosecdef
           AND NOT EXISTS (
               SELECT FROM unnest(p.proconfig) AS setting
               WHERE starts_with(setting, 'search_path=')
           )
         UNION ALL
         SELECT format('function %s is SECURITY DEFINER and executable by PUBLIC',
                       p.oid::regprocedure)
         FROM pg_proc p
         WHERE p.pronamespace = 'corqty'::regnamespace
           AND p.prosecdef
           -- A NULL proacl means the default ACL, which lets PUBLIC execute.
           AND EXISTS (
               SELECT FROM aclexplode(coalesce(p.proacl, acldefault('f', p.proowner))) AS acl
               WHERE acl.grantee = 0 AND acl.privilege_type = 'EXECUTE'
           )
         UNION ALL
         SELECT format('table %s grants privileges to corqty_worker', c.relname)
         FROM pg_class c
         WHERE c.relnamespace = 'public'::regnamespace
           AND c.relkind IN ('r', 'p')
           AND (
               has_table_privilege('corqty_worker', c.oid,
                   'SELECT, INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER, MAINTAIN')
               OR has_any_column_privilege('corqty_worker', c.oid,
                   'SELECT, INSERT, UPDATE, REFERENCES')
           )
         ORDER BY 1",
    )
    .fetch_all(conn)
    .await
    .unwrap()
}

#[tokio::test]
#[serial]
async fn migrated_database_has_no_security_violations() {
    let database = TestDatabase::reset().await;
    let mut conn = database.owner.acquire().await.unwrap();

    assert_eq!(security_violations(&mut conn).await, Vec::<String>::new());
}

#[tokio::test]
#[serial]
async fn security_violations_report_unguarded_objects() {
    let database = TestDatabase::reset().await;
    let mut tx = database.owner.begin().await.unwrap();
    // One probe for each kind of violation; the rollback removes them all.
    sqlx::raw_sql(
        "CREATE TABLE public.guard_probe (id int);
         GRANT SELECT ON public.guard_probe TO corqty_worker;
         CREATE FUNCTION corqty.guard_probe() RETURNS int
         LANGUAGE sql SECURITY DEFINER AS 'SELECT 1';",
    )
    .execute(&mut *tx)
    .await
    .unwrap();

    let violations = security_violations(&mut tx).await;
    tx.rollback().await.unwrap();

    assert_eq!(
        violations,
        [
            "function corqty.guard_probe() is SECURITY DEFINER and executable by PUBLIC",
            "function corqty.guard_probe() is SECURITY DEFINER without search_path",
            "table guard_probe grants privileges to corqty_worker",
            "table guard_probe lacks forced row-level security or a policy",
        ]
    );
}

#[tokio::test]
#[serial]
async fn the_worker_role_executes_only_the_worker_functions() {
    let database = TestDatabase::reset().await;

    // Only definer functions run with more rights than the caller's, and the worker role
    // has no table privileges of its own.
    let functions: Vec<String> = sqlx::query_scalar(
        "SELECT p.proname::text
         FROM pg_proc p
         WHERE p.pronamespace = 'corqty'::regnamespace
           AND p.prosecdef
           AND has_function_privilege('corqty_worker', p.oid, 'EXECUTE')
         ORDER BY 1",
    )
    .fetch_all(&database.owner)
    .await
    .unwrap();

    assert_eq!(
        functions,
        [
            "append_node_log",
            "claim_node_runs",
            "complete_node_run",
            "heartbeat",
            "register_worker",
            "start_node_run",
        ]
    );
}
