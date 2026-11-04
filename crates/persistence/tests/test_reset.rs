use corqty_test_support::TestDatabase;
use serial_test::serial;

/// Rows that `TestDatabase::reset()` puts back after truncating a table.
fn seeded_rows(table: &str) -> i64 {
    match table {
        "node_definitions" => 6,
        "blueprints" => 2,
        _ => 0,
    }
}

#[tokio::test]
#[serial]
async fn reset_truncates_every_table_in_public() {
    let database = TestDatabase::reset().await;
    // reset_probe stands in for a table that a later migration adds: reset()
    // has to find it in the catalog rather than in a list of known tables.
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS public.reset_probe (id integer);
         INSERT INTO reset_probe VALUES (1);
         INSERT INTO users(id, email)
         VALUES (gen_random_uuid(), 'admin@example.com');
         INSERT INTO organizations(id, name, slug, created_by)
         SELECT gen_random_uuid(), 'Acme', 'acme', id FROM users;
         INSERT INTO memberships(organization_id, user_id, role)
         SELECT organizations.id, users.id, 'admin' FROM organizations, users;
         INSERT INTO invitations(
             id, organization_id, email, role, token_hash, invited_by, expires_at
         )
         SELECT gen_random_uuid(), organizations.id, 'viewer@example.com',
                'viewer', 'token'::bytea, users.id, now() + interval '1 day'
         FROM organizations, users;
         INSERT INTO audit_events(
             organization_id, actor_user_id, action, resource_type, resource_id
         )
         SELECT organizations.id, users.id, 'invitation.created', 'invitation',
                'viewer@example.com'
         FROM organizations, users;",
    )
    .execute(&database.owner)
    .await
    .unwrap();

    let database = TestDatabase::reset().await;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT tablename::text FROM pg_tables
         WHERE schemaname = 'public' AND tablename <> '_sqlx_migrations'
         ORDER BY tablename",
    )
    .fetch_all(&database.owner)
    .await
    .unwrap();
    let mut rows = Vec::new();
    for table in &tables {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&database.owner)
            .await
            .unwrap();
        rows.push((table.as_str(), count));
    }
    sqlx::query("DROP TABLE reset_probe")
        .execute(&database.owner)
        .await
        .unwrap();

    let expected: Vec<_> = tables
        .iter()
        .map(|table| (table.as_str(), seeded_rows(table)))
        .collect();
    assert!(tables.iter().any(|table| table == "reset_probe"));
    assert_eq!(rows, expected);
}
