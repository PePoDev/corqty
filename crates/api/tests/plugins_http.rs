mod support;
use axum::{
    body::to_bytes,
    http::{Method, StatusCode},
    response::Response,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use corqty_persistence::worker_queue::WorkerDb;
use serde_json::{Value, json};
use serial_test::serial;
use support::AuthFixture;
use uuid::Uuid;

async fn body(response: Response, status: StatusCode) -> Value {
    let actual = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(actual, status, "{}", String::from_utf8_lossy(&bytes));
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
#[serial]
async fn runtime_install_publish_worker_filtering_and_disable_are_tenant_scoped() {
    let admin = AuthFixture::registered("plugins@example.com", "Components").await;
    let org = format!("/api/v1/orgs/{}", admin.organization_id);
    let manifest: Value =
        serde_json::from_str(include_str!("../../../plugins/example-wasm/manifest.json")).unwrap();
    let upload = json!({"manifest":manifest,"componentBase64":STANDARD.encode(include_bytes!("../../../plugins/example-wasm/hello.wasm"))});
    let plugin = body(
        admin
            .post_json(&format!("{org}/plugins"), &upload.to_string())
            .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(plugin["approved"], true);
    assert!(plugin.get("component").is_none());
    let id = plugin["id"].as_str().unwrap();
    let duplicate = body(
        admin
            .post_json(&format!("{org}/plugins"), &upload.to_string())
            .await,
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(duplicate["code"], "plugin_version_exists");
    let other = admin
        .register_another("other-plugin@example.com", "Other")
        .await;
    assert_eq!(
        other
            .request_json(
                Method::PUT,
                &format!(
                    "/api/v1/orgs/{}/plugins/{id}/enabled",
                    other.organization_id
                ),
                r#"{"enabled":false}"#
            )
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let definition = json!({"key":"example.hello","displayName":"Hello component","category":"Examples",
      "inputs":[],"outputs":[{"name":"result","dataType":"core.string","schema":{"type":"string"},"required":true}],
      "configSchema":{"type":"object"},"uiSchema":{},"defaults":{},"executor":{"key":"hello","version":"1.0.0"}});
    body(
        admin
            .post_json(&format!("{org}/node-definitions"), &definition.to_string())
            .await,
        StatusCode::CREATED,
    )
    .await;
    let workflow = body(
        admin
            .post_json(&format!("{org}/workflows"), r#"{"name":"Component run"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let workflow_path = format!("{org}/workflows/{}", workflow["id"].as_str().unwrap());
    let mut graph = workflow["draft"]["graph"].clone();
    graph["nodes"] = json!([{"id":"hello","definition":{"key":"example.hello","version":1},"displayName":"Hello","position":{"x":0,"y":0},"config":{},"inputs":{},"retry":{"maxAttempts":1},"timeoutSeconds":30}]);
    body(
        admin
            .request_json(
                Method::PUT,
                &format!("{workflow_path}/draft"),
                &json!({"revision":1,"graph":graph}).to_string(),
            )
            .await,
        StatusCode::OK,
    )
    .await;
    body(
        admin
            .post_json(&format!("{workflow_path}/publish"), r#"{"revision":2}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let run = body(
        admin
            .post_json(&format!("{workflow_path}/runs"), "{}")
            .await,
        StatusCode::CREATED,
    )
    .await;
    let old_worker = WorkerDb::new(admin.worker.clone(), Uuid::new_v4());
    old_worker
        .register(&["mock@1.0.0".into()], 1)
        .await
        .unwrap();
    assert!(old_worker.claim(1, 30).await.unwrap().is_empty());
    let runtime = WorkerDb::new(admin.worker.clone(), Uuid::new_v4());
    runtime
        .register_with_contracts(&[], &["1".into()], 1)
        .await
        .unwrap();
    body(
        admin
            .request_json(
                Method::PUT,
                &format!("{org}/plugins/{id}/enabled"),
                r#"{"enabled":false}"#,
            )
            .await,
        StatusCode::OK,
    )
    .await;
    // Existing run is not interrupted by disabling the exact version.
    let jobs = runtime.claim(1, 30).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].run_id.to_string(), run["id"]);
    let payload = runtime
        .runtime_plugin(jobs[0].node_run_id, jobs[0].lease_token)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(payload.manifest, manifest);
    assert!(
        old_worker
            .runtime_plugin(jobs[0].node_run_id, jobs[0].lease_token)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        runtime
            .runtime_plugin(jobs[0].node_run_id, jobs[0].lease_token + 1)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        admin
            .post_json(&format!("{workflow_path}/runs"), "{}")
            .await
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
#[serial]
async fn invalid_components_and_stale_approvals_never_install_or_approve() {
    let admin = AuthFixture::registered("invalid-plugin@example.com", "Components").await;
    let path = format!("/api/v1/orgs/{}/plugins", admin.organization_id);
    let manifest: Value =
        serde_json::from_str(include_str!("../../../plugins/example-wasm/manifest.json")).unwrap();
    let invalid =
        json!({"manifest":manifest,"componentBase64":STANDARD.encode(b"uploaded source code")});
    assert_eq!(
        admin.post_json(&path, &invalid.to_string()).await.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let valid = json!({"manifest":manifest,"componentBase64":STANDARD.encode(include_bytes!("../../../plugins/example-wasm/hello.wasm"))});
    let plugin = body(
        admin.post_json(&path, &valid.to_string()).await,
        StatusCode::CREATED,
    )
    .await;
    let approve = format!("{path}/{}/approval", plugin["id"].as_str().unwrap());
    assert_eq!(
        admin
            .request_json(
                Method::PUT,
                &approve,
                &json!({"checksum":"stale","capabilities":manifest["capabilities"]}).to_string()
            )
            .await
            .status(),
        StatusCode::CONFLICT
    );
    body(
        admin
            .request_json(
                Method::PUT,
                &approve,
                &json!({"checksum":plugin["checksum"],"capabilities":manifest["capabilities"]})
                    .to_string(),
            )
            .await,
        StatusCode::OK,
    )
    .await;
}
