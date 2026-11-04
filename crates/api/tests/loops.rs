mod support;

use std::time::Duration;

use axum::{body::to_bytes, http::StatusCode, response::Response};
use corqty_api::{config::ApiConfig, loops, state::AppState};
use corqty_persistence::{store::PgStore, worker_queue::WorkerDb};
use corqty_test_support::API_URL;
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use support::AuthFixture;
use uuid::Uuid;

/// Asserts the status and returns the JSON body.
async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
#[serial]
async fn the_recovery_loop_recovers_expired_leases_until_shutdown() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let org = format!("/api/v1/orgs/{}", admin.organization_id);
    let request = json!({
        "name": "Provision",
        "blueprintId": "00000000-0000-7000-8000-000000000101",
    });
    let created = body(
        admin
            .post_json(&format!("{org}/workflows"), &request.to_string())
            .await,
        StatusCode::CREATED,
    )
    .await;
    let workflow = format!("{org}/workflows/{}", created["id"].as_str().unwrap());
    let published = admin
        .post_json(&format!("{workflow}/publish"), r#"{"revision":1}"#)
        .await;
    assert_eq!(published.status(), StatusCode::CREATED);
    let input = r#"{"input":{"region":"eu-west-1"}}"#;
    let run = body(
        admin.post_json(&format!("{workflow}/runs"), input).await,
        StatusCode::CREATED,
    )
    .await;
    let run = format!("{org}/runs/{}", run["id"].as_str().unwrap());

    // A worker claims the first node, then stops renewing its lease.
    let worker = WorkerDb::new(admin.worker.clone(), Uuid::now_v7());
    worker
        .register(&["mock@1.0.0".to_owned()], 1)
        .await
        .unwrap();
    let job = worker.claim(1, 30).await.unwrap().remove(0);
    sqlx::query(
        "UPDATE node_runs SET lease_expires_at = now() - interval '1 second' WHERE id = $1",
    )
    .bind(job.node_run_id)
    .execute(&admin.owner)
    .await
    .unwrap();

    // The loop's first pass runs at once. The node had its only attempt, so it fails and
    // the run fails with it.
    let pool = PgPool::connect(API_URL).await.unwrap();
    let state = AppState::new(PgStore::new(pool), ApiConfig::for_tests());
    let mut background = loops::spawn(&state);
    let recovered = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let run = body(admin.get(&run).await, StatusCode::OK).await;
            if run["status"] != "queued" {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the first pass recovers the lease within 2 s");
    let states: Vec<&str> = recovered["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["state"].as_str().unwrap())
        .collect();
    assert_eq!(recovered["status"], "failed");
    assert_eq!(states, ["failed", "skipped", "skipped"]);
    assert_eq!(recovered["nodes"][0]["error"]["code"], "lease_expired");

    // The loops end on the shutdown token, although the next pass is 5 s away.
    state.shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(result) = background.join_next().await {
            result.unwrap();
        }
    })
    .await
    .expect("the loops stop within 2 s");
}
