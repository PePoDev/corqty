use corqty_test_support::TestDatabase;
use serial_test::serial;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn concurrent_demotions_cannot_remove_every_admin() {
    let database = TestDatabase::reset().await;
    let organization_id = Uuid::new_v4();
    let first_admin = Uuid::new_v4();
    let second_admin = Uuid::new_v4();
    for (user_id, email) in [
        (first_admin, "first@example.com"),
        (second_admin, "second@example.com"),
    ] {
        sqlx::query("INSERT INTO users(id, email) VALUES ($1, $2)")
            .bind(user_id)
            .bind(email)
            .execute(&database.owner)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO organizations(id, name, slug, created_by)
         VALUES ($1, 'Acme', 'acme', $2)",
    )
    .bind(organization_id)
    .bind(first_admin)
    .execute(&database.owner)
    .await
    .unwrap();
    for user_id in [first_admin, second_admin] {
        sqlx::query(
            "INSERT INTO memberships(organization_id, user_id, role)
             VALUES ($1, $2, 'admin')",
        )
        .bind(organization_id)
        .bind(user_id)
        .execute(&database.owner)
        .await
        .unwrap();
    }

    let mut first = database.owner.begin().await.unwrap();
    let mut second = database.owner.begin().await.unwrap();
    sqlx::query(
        "UPDATE memberships SET role = 'viewer'
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(first_admin)
    .execute(&mut *first)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE memberships SET role = 'viewer'
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(second_admin)
    .execute(&mut *second)
    .await
    .unwrap();

    let (first_result, second_result) = tokio::join!(first.commit(), second.commit());
    assert_ne!(first_result.is_ok(), second_result.is_ok());
    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM memberships
         WHERE organization_id = $1 AND role = 'admin'",
    )
    .bind(organization_id)
    .fetch_one(&database.owner)
    .await
    .unwrap();
    assert_eq!(remaining, 1);
}

#[tokio::test]
#[serial]
async fn rejected_final_admin_change_rolls_back_its_audit_event() {
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

    let mut tx = database.owner.begin().await.unwrap();
    sqlx::query(
        "UPDATE memberships SET role = 'viewer'
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO audit_events(
            organization_id, actor_user_id, action,
            resource_type, resource_id
         ) VALUES ($1, $2, 'membership.role_changed', 'membership', $3)",
    )
    .bind(organization_id)
    .bind(user_id)
    .bind(user_id.to_string())
    .execute(&mut *tx)
    .await
    .unwrap();
    assert!(tx.commit().await.is_err());

    let audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events
         WHERE organization_id = $1
           AND action = 'membership.role_changed'",
    )
    .bind(organization_id)
    .fetch_one(&database.owner)
    .await
    .unwrap();
    assert_eq!(audit_count, 0);
}
