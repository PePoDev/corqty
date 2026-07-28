#![forbid(unsafe_code)]

use sqlx::{PgPool, postgres::PgPoolOptions};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

pub struct TestDatabase {
    pub owner: PgPool,
    pub api: PgPool,
}

impl TestDatabase {
    pub async fn reset() -> Self {
        let owner = PgPoolOptions::new()
            .max_connections(5)
            .connect("postgres://corqty_owner:corqty_owner_dev@127.0.0.1:55432/corqty_test")
            .await
            .expect("connect owner test database");
        MIGRATOR.run(&owner).await.expect("run test migrations");

        sqlx::query(
            "TRUNCATE TABLE
                audit_events,
                invitations,
                memberships,
                organizations,
                sessions,
                password_credentials,
                users
             RESTART IDENTITY CASCADE",
        )
        .execute(&owner)
        .await
        .expect("reset test data");

        let api = PgPoolOptions::new()
            // A single API connection makes the context-leak test prove that
            // SET LOCAL values disappear before this connection is reused.
            .max_connections(1)
            .connect("postgres://corqty_api:corqty_api_dev@127.0.0.1:55432/corqty_test")
            .await
            .expect("connect api test database");

        Self { owner, api }
    }
}
