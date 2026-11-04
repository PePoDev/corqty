mod support;

use axum::{
    body::to_bytes,
    http::{Method, StatusCode},
    response::Response,
};
use corqty_api::{config::ApiConfig, router::build_router, state::AppState};
use corqty_persistence::{store::PgStore, worker_queue::WorkerDb};
use corqty_test_support::API_URL;
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use support::AuthFixture;
use uuid::Uuid;

const SHELL_DISABLED: &str =
    "An admin must enable the shell executor before this node can be published or run.";

/// Asserts the status and returns the JSON body.
async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// Registers `<role>@example.com` and adds it to the admin's organization with that role.
async fn member_of(admin: &AuthFixture, role: &str) -> AuthFixture {
    let invitations = format!("/api/v1/orgs/{}/invitations", admin.organization_id);
    let email = format!("{role}@example.com");
    let request = json!({ "email": email, "role": role }).to_string();
    let invitation = body(
        admin.post_json(&invitations, &request).await,
        StatusCode::CREATED,
    )
    .await;
    let member = admin.register_another(&email, "Personal").await;
    let accept = json!({ "token": invitation["token"] }).to_string();
    let accepted = member
        .post_json("/api/v1/invitations/accept", &accept)
        .await;
    assert_eq!(accepted.status(), StatusCode::OK);
    member
}

/// Turns the shell executor of the admin's organization on or off, and returns the answer.
async fn set_shell(admin: &AuthFixture, enabled: bool) -> Value {
    let path = format!("/api/v1/orgs/{}/settings/shell", admin.organization_id);
    let request = json!({ "enabled": enabled }).to_string();
    let response = admin.request_json(Method::PUT, &path, &request).await;
    body(response, StatusCode::OK).await
}

/// The audit events about the admin's organization itself, newest first, as
/// [actorUserId, action, resourceId, metadata].
async fn organization_audit(admin: &AuthFixture) -> Vec<Value> {
    let path = format!("/api/v1/orgs/{}/audit-events", admin.organization_id);
    let events = body(admin.get(&path).await, StatusCode::OK).await;
    events["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["resourceType"] == "organization")
        .map(|event| {
            json!([
                event["actorUserId"],
                event["action"],
                event["resourceId"],
                event["metadata"],
            ])
        })
        .collect()
}

/// A node with no inputs, one attempt and a 60 s timeout.
fn node(id: &str, key: &str, config: Value) -> Value {
    json!({
        "id": id,
        "definition": { "key": key, "version": 1 },
        "displayName": id,
        "position": { "x": 0, "y": 0 },
        "config": config,
        "inputs": {},
        "retry": { "maxAttempts": 1 },
        "timeoutSeconds": 60,
    })
}

#[tokio::test]
#[serial]
async fn admins_turn_the_shell_executor_on_and_off() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let viewer = member_of(&admin, "viewer").await;
    let settings = format!("/api/v1/orgs/{}/settings", admin.organization_id);
    let (enabled, disabled) = (
        json!({ "shellAvailable": true, "shellEnabled": true }),
        json!({ "shellAvailable": true, "shellEnabled": false }),
    );
    assert_eq!(
        body(viewer.get(&settings).await, StatusCode::OK).await,
        disabled
    );

    assert_eq!(set_shell(&admin, true).await, enabled);
    // Setting the value the organization already has changes nothing and audits nothing.
    assert_eq!(set_shell(&admin, true).await, enabled);
    assert_eq!(
        body(viewer.get(&settings).await, StatusCode::OK).await,
        enabled
    );
    assert_eq!(set_shell(&admin, false).await, disabled);
    assert_eq!(set_shell(&admin, false).await, disabled);
    assert_eq!(
        body(viewer.get(&settings).await, StatusCode::OK).await,
        disabled
    );

    let shell = format!("{settings}/shell");
    for request in ["{}", r#"{"enabled":"yes"}"#] {
        let response = admin.request_json(Method::PUT, &shell, request).await;
        let problem = body(response, StatusCode::UNPROCESSABLE_ENTITY).await;
        assert_eq!(problem["code"], "invalid_json", "{request}");
    }

    let event = |action: &str, metadata: Value| {
        json!([admin.user_id, action, admin.organization_id, metadata])
    };
    assert_eq!(
        organization_audit(&admin).await,
        [
            event("organization.shell_disabled", json!({})),
            event("organization.shell_enabled", json!({})),
            event("organization.created", json!({ "name": "Acme" })),
        ]
    );
}

#[tokio::test]
#[serial]
async fn shell_nodes_publish_and_start_only_while_shell_is_enabled() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let editor = member_of(&admin, "editor").await;
    let runner = member_of(&admin, "runner").await;
    let org = format!("/api/v1/orgs/{}", admin.organization_id);

    // An organization definition bound to shell counts exactly like the system preset.
    let script = json!({
        "key": "acme.script",
        "displayName": "Script",
        "category": "Scripts",
        "inputs": [],
        "outputs": [{ "name": "result", "dataType": "core.any", "schema": {} }],
        "configSchema": { "type": "object" },
        "uiSchema": {},
        "defaults": {},
        "executor": { "key": "shell", "version": "1.0.0" },
    });
    let definitions = format!("{org}/node-definitions");
    let created = editor.post_json(&definitions, &script.to_string()).await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let mock = json!({
        "mode": "fixed", "output": { "result": 1 }, "delayMs": 0, "failureMessage": null,
    });
    let graph = json!({
        "schemaVersion": 1,
        "workflowInputSchema": { "type": "object" },
        "nodes": [
            node("mock", "corqty.mock.task", mock),
            node("preset", "corqty.shell.command", json!({ "command": "true", "env": {} })),
            node("script", "acme.script", json!({})),
        ],
        "edges": [],
        "viewport": { "x": 0, "y": 0, "zoom": 1 },
    });
    let workflow = body(
        editor
            .post_json(&format!("{org}/workflows"), r#"{"name":"Scripts"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let workflow = format!("{org}/workflows/{}", workflow["id"].as_str().unwrap());
    let draft = json!({ "revision": 1, "graph": graph }).to_string();
    let saved = editor
        .request_json(Method::PUT, &format!("{workflow}/draft"), &draft)
        .await;
    assert_eq!(saved.status(), StatusCode::OK);

    let shell_nodes = json!([
        { "code": "executor_disabled", "pointer": "/nodes/1/definition", "message": SHELL_DISABLED },
        { "code": "executor_disabled", "pointer": "/nodes/2/definition", "message": SHELL_DISABLED },
    ]);
    let validate = format!("{workflow}/validate");
    let request = json!({ "graph": graph }).to_string();
    assert_eq!(
        body(editor.post_json(&validate, &request).await, StatusCode::OK).await,
        json!({ "valid": false, "diagnostics": shell_nodes }),
    );
    let publish = format!("{workflow}/publish");
    let refused = body(
        editor.post_json(&publish, r#"{"revision":2}"#).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(
        (&refused["code"], &refused["diagnostics"]),
        (&json!("graph_invalid"), &shell_nodes)
    );

    set_shell(&admin, true).await;
    assert_eq!(
        body(editor.post_json(&validate, &request).await, StatusCode::OK).await,
        json!({ "valid": true, "diagnostics": [] }),
    );
    let published = editor.post_json(&publish, r#"{"revision":2}"#).await;
    assert_eq!(published.status(), StatusCode::CREATED);

    // A published version still needs shell enabled whenever a run of it starts.
    set_shell(&admin, false).await;
    let runs = format!("{workflow}/runs");
    let refused = body(
        runner.post_json(&runs, "{}").await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(
        (&refused["code"], &refused["diagnostics"]),
        (&json!("executor_disabled"), &shell_nodes)
    );
    set_shell(&admin, true).await;
    let run = body(runner.post_json(&runs, "{}").await, StatusCode::CREATED).await;
    let run_id: Uuid = run["id"].as_str().unwrap().parse().unwrap();
    let path = format!("{org}/runs/{run_id}");

    // Disabling stops new runs and reruns, but the run that exists keeps its shell nodes.
    set_shell(&admin, false).await;
    let rerun = body(
        runner.post_json(&format!("{path}/rerun"), "{}").await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(
        (&rerun["code"], &rerun["diagnostics"]),
        (&json!("executor_disabled"), &shell_nodes)
    );
    assert_eq!(body(runner.get(&path).await, StatusCode::OK).await, run);
    let worker = WorkerDb::new(admin.worker.clone(), Uuid::now_v7());
    worker
        .register(&["shell@1.0.0".to_owned()], 4)
        .await
        .unwrap();
    let claimed: Vec<_> = worker
        .claim(10, 30)
        .await
        .unwrap()
        .into_iter()
        .map(|job| (job.run_id, job.executor))
        .collect();
    let shell_job = (run_id, "shell@1.0.0".to_owned());
    assert_eq!(claimed, [shell_job.clone(), shell_job]);
}

#[tokio::test]
#[serial]
async fn without_the_operator_switch_no_organization_can_use_shell() {
    let mut admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let org = format!("/api/v1/orgs/{}", admin.organization_id);

    // Published while the deployment offered shell and the organization had it enabled.
    set_shell(&admin, true).await;
    let preset = node(
        "preset",
        "corqty.shell.command",
        json!({ "command": "true", "env": {} }),
    );
    let graph = json!({
        "schemaVersion": 1,
        "workflowInputSchema": { "type": "object" },
        "nodes": [preset],
        "edges": [],
        "viewport": { "x": 0, "y": 0, "zoom": 1 },
    });
    let workflow = body(
        admin
            .post_json(&format!("{org}/workflows"), r#"{"name":"Script"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let workflow = format!("{org}/workflows/{}", workflow["id"].as_str().unwrap());
    let draft = format!("{workflow}/draft");
    let saved = json!({ "revision": 1, "graph": graph }).to_string();
    let saved = admin.request_json(Method::PUT, &draft, &saved).await;
    assert_eq!(saved.status(), StatusCode::OK);
    let publish = format!("{workflow}/publish");
    let published = admin.post_json(&publish, r#"{"revision":2}"#).await;
    assert_eq!(published.status(), StatusCode::CREATED);

    // The API restarts without CORQTY_SHELL_EXECUTOR=enabled.
    let config = ApiConfig {
        shell_executor_available: false,
        ..ApiConfig::for_tests()
    };
    let pool = PgPool::connect(API_URL).await.unwrap();
    admin.app = build_router(AppState::new(PgStore::new(pool), config));
    let settings = format!("{org}/settings");
    assert_eq!(
        body(admin.get(&settings).await, StatusCode::OK).await,
        json!({ "shellAvailable": false, "shellEnabled": true })
    );

    // Shell nodes count as disabled, so they neither publish nor start.
    let shell_node = json!([
        { "code": "executor_disabled", "pointer": "/nodes/0/definition", "message": SHELL_DISABLED },
    ]);
    let saved = json!({ "revision": 2, "graph": graph }).to_string();
    let saved = admin.request_json(Method::PUT, &draft, &saved).await;
    assert_eq!(saved.status(), StatusCode::OK);
    let refused = body(
        admin.post_json(&publish, r#"{"revision":3}"#).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(
        (&refused["code"], &refused["diagnostics"]),
        (&json!("graph_invalid"), &shell_node)
    );
    let refused = body(
        admin.post_json(&format!("{workflow}/runs"), "{}").await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(
        (&refused["code"], &refused["diagnostics"]),
        (&json!("executor_disabled"), &shell_node)
    );

    // No admin can turn shell on, but turning it off still works.
    let shell = format!("{settings}/shell");
    let refused = body(
        admin
            .request_json(Method::PUT, &shell, r#"{"enabled":true}"#)
            .await,
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(refused["code"], "shell_unavailable");
    assert_eq!(
        set_shell(&admin, false).await,
        json!({ "shellAvailable": false, "shellEnabled": false })
    );
}
