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
