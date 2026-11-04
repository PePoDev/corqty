mod support;

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use chrono::Utc;
use corqty_api::{automation, config::ApiConfig, state::AppState};
use corqty_persistence::store::PgStore;
use corqty_persistence::worker_queue::WorkerDb;
use corqty_test_support::API_URL;
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use serial_test::serial;
use sha2::Sha256;
use sqlx::PgPool;
use support::AuthFixture;
use tower::ServiceExt;

async fn body(response: Response, status: StatusCode) -> Value {
    let actual = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(actual, status, "{}", String::from_utf8_lossy(&bytes));
    if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    }
}

async fn workflow(admin: &AuthFixture, mock: bool) -> String {
    let org = format!("/api/v1/orgs/{}", admin.organization_id);
    let mut request = json!({"name":"Automation"});
    if mock {
        request["blueprintId"] = json!("00000000-0000-7000-8000-000000000101");
    }
    let created = body(
        admin
            .post_json(&format!("{org}/workflows"), &request.to_string())
            .await,
        StatusCode::CREATED,
    )
    .await;
    let id = created["id"].as_str().unwrap().to_owned();
    body(
        admin
            .post_json(
                &format!("{org}/workflows/{id}/publish"),
                r#"{"revision":1}"#,
            )
            .await,
        StatusCode::CREATED,
    )
    .await;
    id
}

async fn bearer(
    admin: &AuthFixture,
    token: &str,
    method: Method,
    path: &str,
    key: &str,
    input: &str,
) -> Response {
    admin
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header("idempotency-key", key)
                .body(Body::from(input.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
#[serial]
async fn service_tokens_are_once_only_scoped_revocable_and_idempotent() {
    let admin = AuthFixture::registered("admin@example.com", "Automation").await;
    let workflow = workflow(&admin, false).await;
    let org = format!("/api/v1/orgs/{}", admin.organization_id);
    let created = body(
        admin
            .post_json(&format!("{org}/service-tokens"), r#"{"name":"deploy"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let token = created["token"].as_str().unwrap();
    assert_eq!(created["scope"], "workflow:run");
    let listed = body(
        admin.get(&format!("{org}/service-tokens")).await,
        StatusCode::OK,
    )
    .await;
    assert!(listed["items"][0].get("token").is_none());
    let hash: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM service_tokens WHERE id=$1")
        .bind(
            created["id"]
                .as_str()
                .unwrap()
                .parse::<uuid::Uuid>()
                .unwrap(),
        )
        .fetch_one(&admin.owner)
        .await
        .unwrap();
    assert_eq!(hash.len(), 32);
    assert_ne!(hash, token.as_bytes());
    let path = format!("{org}/workflows/{workflow}/runs");
    let first = body(
        bearer(&admin, token, Method::POST, &path, "deploy-1", "{}").await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(first["triggerSource"], "api");
    let second = body(
        bearer(&admin, token, Method::POST, &path, "deploy-1", "{}").await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(first["id"], second["id"]);
    body(
        bearer(
            &admin,
            token,
            Method::POST,
            &path,
            "deploy-1",
            r#"{"input":{"changed":true}}"#,
        )
        .await,
        StatusCode::CONFLICT,
    )
    .await;
    body(
        bearer(&admin, token, Method::GET, &path, "deploy-1", "{}").await,
        StatusCode::FORBIDDEN,
    )
    .await;
    let outsider = admin
        .register_another("outsider@example.com", "Other")
        .await;
    let cross = format!(
        "/api/v1/orgs/{}/workflows/{workflow}/runs",
        outsider.organization_id
    );
    body(
        bearer(&admin, token, Method::POST, &cross, "cross", "{}").await,
        StatusCode::NOT_FOUND,
    )
    .await;
    body(
        admin
            .delete(&format!(
                "{org}/service-tokens/{}",
                created["id"].as_str().unwrap()
            ))
            .await,
        StatusCode::NO_CONTENT,
    )
    .await;
    body(
        bearer(&admin, token, Method::POST, &path, "deploy-2", "{}").await,
        StatusCode::UNAUTHORIZED,
    )
    .await;
    let actor: Option<uuid::Uuid> = sqlx::query_scalar("SELECT created_by FROM runs WHERE id=$1")
        .bind(first["id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap())
        .fetch_one(&admin.owner)
        .await
        .unwrap();
    assert!(actor.is_none());
}

fn signature(secret: &str, timestamp: &str, payload: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(payload.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

async fn hook(
    admin: &AuthFixture,
    path: &str,
    secret: &str,
    timestamp: &str,
    key: &str,
    payload: &str,
) -> Response {
    admin
        .app
        .clone()
        .oneshot(
            Request::post(path)
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-corqty-timestamp", timestamp)
                .header("x-corqty-signature", signature(secret, timestamp, payload))
                .header("idempotency-key", key)
                .body(Body::from(payload.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
#[serial]
async fn signed_webhooks_pin_versions_and_reject_replays_with_changed_bodies() {
    let admin = AuthFixture::registered("admin@example.com", "Automation").await;
    let workflow = workflow(&admin, false).await;
    let org = format!("/api/v1/orgs/{}", admin.organization_id);
    let request = json!({"name":"Inbound","workflowId":workflow,"version":1,"kind":"webhook"});
    let created = body(
        admin
            .post_json(&format!("{org}/triggers"), &request.to_string())
            .await,
        StatusCode::CREATED,
    )
    .await;
    let secret = created["webhookSecret"].as_str().unwrap();
    let path = created["webhookPath"].as_str().unwrap();
    let timestamp = Utc::now().timestamp().to_string();
    let listed = body(admin.get(&format!("{org}/triggers")).await, StatusCode::OK).await;
    assert!(!listed.to_string().contains(secret));
    assert!(listed["items"][0].get("webhookCiphertext").is_none());
    let first = body(
        hook(&admin, path, secret, &timestamp, "event-1", "{}").await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(first["triggerSource"], "webhook");
    assert_eq!(first["version"], 1);
    let replay = body(
        hook(&admin, path, secret, &timestamp, "event-1", "{}").await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(first["id"], replay["id"]);
    body(
        hook(&admin, path, secret, &timestamp, "event-1", r#"{"x":1}"#).await,
        StatusCode::CONFLICT,
    )
    .await;
    body(
        hook(&admin, path, "wrong", &timestamp, "event-2", "{}").await,
        StatusCode::UNAUTHORIZED,
    )
    .await;
    let stale = (Utc::now().timestamp() - 301).to_string();
    body(
        hook(&admin, path, secret, &stale, "event-2", "{}").await,
        StatusCode::UNAUTHORIZED,
    )
    .await;
    let mut update = request;
    update["enabled"] = json!(false);
    body(
        admin
            .request_json(
                Method::PUT,
                &format!("{org}/triggers/{}", created["id"].as_str().unwrap()),
                &update.to_string(),
            )
            .await,
        StatusCode::OK,
    )
    .await;
    body(
        hook(&admin, path, secret, &timestamp, "event-2", "{}").await,
        StatusCode::NOT_FOUND,
    )
    .await;
}

#[tokio::test]
#[serial]
async fn cron_dispatch_is_unique_skips_overlap_and_audits_old_misfires() {
    let admin = AuthFixture::registered("admin@example.com", "Automation").await;
    let workflow = workflow(&admin, true).await;
    let org = format!("/api/v1/orgs/{}", admin.organization_id);
    let request = json!({"name":"Scheduled","workflowId":workflow,"version":1,"kind":"cron","cron":"* * * * *","timezone":"Asia/Bangkok","input":{"region":"eu-west-1"}});
    let trigger = body(
        admin
            .post_json(&format!("{org}/triggers"), &request.to_string())
            .await,
        StatusCode::CREATED,
    )
    .await;
    let id: uuid::Uuid = trigger["id"].as_str().unwrap().parse().unwrap();
    sqlx::query("UPDATE triggers SET next_scheduled_at=date_trunc('minute',now()) - interval '10 minutes' WHERE id=$1")
        .bind(id).execute(&admin.owner).await.unwrap();
    let state = AppState::new(
        PgStore::new(PgPool::connect(API_URL).await.unwrap()),
        ApiConfig::for_tests(),
    );
    let now = Utc::now();
    let (first, second) = tokio::join!(
        automation::dispatch_due(&state, now),
        automation::dispatch_due(&state, now)
    );
    assert_eq!(first.unwrap() + second.unwrap(), 1);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM runs WHERE trigger_id=$1")
        .bind(id)
        .fetch_one(&admin.owner)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let outcomes: Vec<String> = sqlx::query_scalar(
        "SELECT outcome FROM trigger_occurrences WHERE trigger_id=$1 ORDER BY scheduled_for",
    )
    .bind(id)
    .fetch_all(&admin.owner)
    .await
    .unwrap();
    assert_eq!(outcomes[0], "misfire_skipped");
    assert_eq!(outcomes[1], "started");
    assert!(outcomes[2..].iter().all(|value| value == "overlap_skipped"));
    let next: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT next_scheduled_at FROM triggers WHERE id=$1")
            .bind(id)
            .fetch_one(&admin.owner)
            .await
            .unwrap();
    assert!(next > now);
    assert_eq!(automation::dispatch_due(&state, now).await.unwrap(), 0);
    let audits:i64=sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action='trigger.misfire_skipped' AND resource_id=$1")
        .bind(id.to_string()).fetch_one(&admin.owner).await.unwrap();
    assert_eq!(audits, 1);
}

#[tokio::test]
#[serial]
async fn trigger_and_token_management_enforce_the_role_matrix() {
    let admin = AuthFixture::registered("admin@example.com", "Automation").await;
    let workflow = workflow(&admin, false).await;
    let org = format!("/api/v1/orgs/{}", admin.organization_id);
    for role in ["editor", "runner", "viewer"] {
        let member = admin
            .register_another(&format!("{role}@example.com"), "Personal")
            .await;
        sqlx::query("INSERT INTO memberships(organization_id,user_id,role) VALUES ($1,$2,$3)")
            .bind(admin.organization_id)
            .bind(member.user_id)
            .bind(role)
            .execute(&admin.owner)
            .await
            .unwrap();
        body(member.get(&format!("{org}/triggers")).await, StatusCode::OK).await;
        body(
            member.get(&format!("{org}/service-tokens")).await,
            StatusCode::FORBIDDEN,
        )
        .await;
        body(
            member
                .post_json(&format!("{org}/service-tokens"), r#"{"name":"No"}"#)
                .await,
            StatusCode::FORBIDDEN,
        )
        .await;
        let request = json!({"name":"Schedule","workflowId":workflow,"version":1,"kind":"cron","cron":"0 * * * *","timezone":"UTC"});
        body(
            member
                .post_json(&format!("{org}/triggers"), &request.to_string())
                .await,
            if role == "editor" {
                StatusCode::CREATED
            } else {
                StatusCode::FORBIDDEN
            },
        )
        .await;
    }
}

#[tokio::test]
#[serial]
async fn retention_preserves_versions_and_audits_and_rate_limits_are_shared() {
    let admin = AuthFixture::registered("admin@example.com", "Automation").await;
    let workflow = workflow(&admin, false).await;
    let org = format!("/api/v1/orgs/{}", admin.organization_id);
    let run = body(
        admin
            .post_json(&format!("{org}/workflows/{workflow}/runs"), "{}")
            .await,
        StatusCode::CREATED,
    )
    .await;
    sqlx::query("UPDATE runs SET finished_at=now()-interval '91 days' WHERE id=$1")
        .bind(run["id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap())
        .execute(&admin.owner)
        .await
        .unwrap();
    let pool = PgPool::connect(API_URL).await.unwrap();
    sqlx::query("SELECT corqty.retain_runs()")
        .execute(&pool)
        .await
        .unwrap();
    body(
        admin
            .get(&format!("{org}/runs/{}", run["id"].as_str().unwrap()))
            .await,
        StatusCode::NOT_FOUND,
    )
    .await;
    body(
        admin
            .get(&format!("{org}/workflows/{workflow}/versions/1"))
            .await,
        StatusCode::OK,
    )
    .await;
    let audits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action='run.started'")
            .fetch_one(&admin.owner)
            .await
            .unwrap();
    assert_eq!(audits, 1);
    let state = AppState::new(PgStore::new(pool), ApiConfig::for_tests());
    automation::check_rate(&state, "same-client", 1, 60)
        .await
        .unwrap();
    assert!(matches!(
        automation::check_rate(&state, "same-client", 1, 60).await,
        Err(corqty_api::error::ApiError::TooManyRequests("rate_limited"))
    ));
}

#[tokio::test]
#[serial]
async fn per_node_log_caps_truncate_utf8_once() {
    let admin = AuthFixture::registered("admin@example.com", "Automation").await;
    let workflow = workflow(&admin, true).await;
    let org = format!("/api/v1/orgs/{}", admin.organization_id);
    body(
        admin
            .post_json(
                &format!("{org}/workflows/{workflow}/runs"),
                r#"{"input":{"region":"eu-west-1"}}"#,
            )
            .await,
        StatusCode::CREATED,
    )
    .await;
    let worker = WorkerDb::new(admin.worker.clone(), uuid::Uuid::now_v7());
    worker
        .register(&["mock@1.0.0".to_owned()], 1)
        .await
        .unwrap();
    let job = worker.claim(1, 30).await.unwrap().remove(0);
    sqlx::query("UPDATE node_runs SET log_limit_bytes=5 WHERE id=$1")
        .bind(job.node_run_id)
        .execute(&admin.owner)
        .await
        .unwrap();
    assert!(
        worker
            .append_log(job.node_run_id, job.lease_token, "stdout", "ééé")
            .await
            .unwrap()
    );
    assert!(
        worker
            .append_log(job.node_run_id, job.lease_token, "stdout", "discarded")
            .await
            .unwrap()
    );
    let logs:Vec<Value>=sqlx::query_scalar("SELECT payload FROM run_events WHERE payload->>'type' IN ('node.log','node.log_truncated') ORDER BY seq")
        .fetch_all(&admin.owner).await.unwrap();
    assert_eq!(logs.len(), 2);
    assert_eq!(logs[0]["text"], "éé");
    assert_eq!(logs[1]["type"], "node.log_truncated");
    let count: i64 = sqlx::query_scalar("SELECT log_bytes FROM node_runs WHERE id=$1")
        .bind(job.node_run_id)
        .fetch_one(&admin.owner)
        .await
        .unwrap();
    assert_eq!(count, 4);
}
