#![forbid(unsafe_code)]

use sqlx::{PgPool, postgres::PgPoolOptions};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

/// The test database as each role connects to it. Tests that need connections of their own,
/// for example a listener or concurrent writers, connect with these.
pub const OWNER_URL: &str = "postgres://corqty_owner:corqty_owner_dev@127.0.0.1:55432/corqty_test";
pub const API_URL: &str = "postgres://corqty_api:corqty_api_dev@127.0.0.1:55432/corqty_test";
pub const WORKER_URL: &str =
    "postgres://corqty_worker:corqty_worker_dev@127.0.0.1:55432/corqty_test";

pub struct TestDatabase {
    pub owner: PgPool,
    pub api: PgPool,
    pub worker: PgPool,
}

impl TestDatabase {
    pub async fn reset() -> Self {
        let owner = PgPoolOptions::new()
            .max_connections(5)
            .connect(OWNER_URL)
            .await
            .expect("connect owner test database");
        MIGRATOR.run(&owner).await.expect("run test migrations");

        // Every table the migrations created, so a new table needs no change here.
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT format('%I.%I', schemaname, tablename)
             FROM pg_tables
             WHERE schemaname = 'public' AND tablename <> '_sqlx_migrations'",
        )
        .fetch_all(&owner)
        .await
        .expect("list test tables");
        sqlx::query(&format!(
            "TRUNCATE TABLE {} RESTART IDENTITY CASCADE",
            tables.join(", ")
        ))
        .execute(&owner)
        .await
        .expect("reset test data");
        // The truncate removed the system catalog that the migration seeded.
        sqlx::query("SELECT corqty.seed_system_catalog()")
            .execute(&owner)
            .await
            .expect("seed system catalog");

        let api = PgPoolOptions::new()
            // A single API connection makes the context-leak test prove that
            // SET LOCAL values disappear before this connection is reused.
            .max_connections(1)
            .connect(API_URL)
            .await
            .expect("connect api test database");

        let worker = PgPoolOptions::new()
            // Enough for eight workers that claim at the same time.
            .max_connections(10)
            .connect(WORKER_URL)
            .await
            .expect("connect worker test database");

        Self { owner, api, worker }
    }
}
