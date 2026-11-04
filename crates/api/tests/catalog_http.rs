mod support;

use std::pin::pin;

use axum::{body::to_bytes, http::StatusCode, response::Response};
use corqty_test_support::TestDatabase;
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use support::AuthFixture;
use uuid::Uuid;

const SYSTEM_DEFINITION_ID: &str = "00000000-0000-7000-8000-000000000001";

fn definition(key: &str) -> Value {
    json!({
        "key": key,
        "displayName": "Thing",
        "category": "Testing",
        "inputs": [{ "name": "input", "dataType": "core.any", "schema": {} }],
        "outputs": [{ "name": "result", "dataType": "core.any", "schema": {} }],
        "configSchema": {
            "type": "object",
            "properties": { "mode": { "type": "string", "enum": ["fixed", "echo"] } },
            "required": ["mode"],
        },
        "uiSchema": { "mode": { "widget": "select" } },
        "defaults": { "mode": "fixed" },
        "executor": { "key": "mock", "version": "1.0.0" },
    })
}

/// Asserts the status and returns the JSON body.
async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// The id of a created record and its JSON without the id and the creation time.
async fn created(response: Response) -> (String, Value) {
    let mut record = body(response, StatusCode::CREATED).await;
    let object = record.as_object_mut().unwrap();
    assert!(object.remove("createdAt").unwrap().is_string());
    let id = object.remove("id").unwrap().as_str().unwrap().to_owned();
    (id, record)
}

/// The (code, pointer) pairs of a 422 problem with the given code.
async fn diagnostics(response: Response, code: &str) -> Value {
    let problem = body(response, StatusCode::UNPROCESSABLE_ENTITY).await;
    assert_eq!(problem["code"], code);
    problem["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| json!([diagnostic["code"], diagnostic["pointer"]]))
        .collect()
}

/// Each item of an `{items}` body as an array of the given fields.
fn items(listed: &Value, fields: &[&str]) -> Vec<Value> {
    listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| fields.iter().map(|field| item[field].clone()).collect())
        .collect()
}

/// The organization's catalog audit events, newest first, without their ids and times.
async fn catalog_events(member: &AuthFixture, organization_id: Uuid) -> Vec<Value> {
    let path = format!("/api/v1/orgs/{organization_id}/audit-events");
    let events = body(member.get(&path).await, StatusCode::OK).await;
    let fields = [
        "actorUserId",
        "action",
        "resourceType",
        "resourceId",
        "metadata",
    ];
    items(&events, &fields)
        .into_iter()
        .filter(|event| event[2] == "node_definition" || event[2] == "blueprint")
        .collect()
}

/// Registers editor@example.com and adds it to the admin's organization as an editor.
async fn editor_of(admin: &AuthFixture) -> AuthFixture {
    let invitations = format!("/api/v1/orgs/{}/invitations", admin.organization_id);
    let invitation = body(
        admin
            .post_json(
                &invitations,
                r#"{"email":"editor@example.com","role":"editor"}"#,
            )
            .await,
        StatusCode::CREATED,
    )
    .await;
    let editor = admin
        .register_another("editor@example.com", "Personal")
        .await;
    let accept = json!({ "token": invitation["token"] }).to_string();
    let accepted = editor
        .post_json("/api/v1/invitations/accept", &accept)
        .await;
    assert_eq!(accepted.status(), StatusCode::OK);
    editor
}

#[tokio::test]
#[serial]
async fn editors_add_and_deprecate_node_definition_versions() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let editor = editor_of(&admin).await;
    let organization_id = admin.organization_id;
    let definitions = format!("/api/v1/orgs/{organization_id}/node-definitions");
    let thing = definition("acme.thing").to_string();

    let mut ids = Vec::new();
    for version in [1, 2] {
        let (id, record) = created(editor.post_json(&definitions, &thing).await).await;
        assert_eq!(
            record,
            json!({
                "scope": "organization",
                "key": "acme.thing",
                "version": version,
                "displayName": "Thing",
                "description": "",
                "category": "Testing",
                "inputs": [{
                    "name": "input",
                    "dataType": "core.any",
                    "schema": {},
                    "required": false,
                    "description": "",
                }],
                "outputs": [{
                    "name": "result",
                    "dataType": "core.any",
                    "schema": {},
                    "required": false,
                    "description": "",
                }],
                "configSchema": {
                    "type": "object",
                    "properties": { "mode": { "type": "string", "enum": ["fixed", "echo"] } },
                    "required": ["mode"],
                },
                "uiSchema": { "mode": { "widget": "select", "label": null, "help": null } },
                "defaults": { "mode": "fixed" },
                "executor": { "key": "mock", "version": "1.0.0" },
                "deprecated": false,
            })
        );
        ids.push(id);
    }

    let reserved = definition("corqty.custom").to_string();
    assert_eq!(
        body(
            editor.post_json(&definitions, &reserved).await,
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await,
        json!({
            "type": "about:blank",
            "title": "Validation failed",
            "status": 422,
            "code": "node_definition_invalid",
            "diagnostics": [{
                "code": "reserved_key",
                "pointer": "/key",
                "message": "Keys starting with \"corqty.\" belong to the system catalog.",
            }],
        })
    );
    let mut dropdown = definition("acme.thing");
    dropdown["uiSchema"]["mode"]["widget"] = json!("dropdown");
    assert_eq!(
        diagnostics(
            editor.post_json(&definitions, &dropdown.to_string()).await,
            "node_definition_invalid",
        )
        .await,
        json!([["unknown_widget", "/uiSchema/mode/widget"]])
    );

    let deprecate = format!("{definitions}/{}/deprecate", ids[0]);
    let deprecated = body(editor.post_json(&deprecate, "").await, StatusCode::OK).await;
    assert_eq!(deprecated["id"], ids[0]);
    assert_eq!(deprecated["version"], 1);
    assert_eq!(deprecated["deprecated"], true);
    // Deprecating again changes nothing, so it writes no second audit event.
    let again = body(editor.post_json(&deprecate, "").await, StatusCode::OK).await;
    assert_eq!(again, deprecated);

    let outsider = admin
        .register_another("outsider@example.com", "Outside")
        .await;
    let foreign = format!("/api/v1/orgs/{}/node-definitions", outsider.organization_id);
    let (foreign_id, _) = created(outsider.post_json(&foreign, &thing).await).await;
    for id in [SYSTEM_DEFINITION_ID, foreign_id.as_str()] {
        let response = editor
            .post_json(&format!("{definitions}/{id}/deprecate"), "")
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{id}");
    }

    let listed = body(editor.get(&definitions).await, StatusCode::OK).await;
    assert_eq!(
        items(&listed, &["scope", "key", "version", "deprecated"]),
        [
            json!(["organization", "acme.thing", 1, true]),
            json!(["organization", "acme.thing", 2, false]),
            json!(["system", "corqty.mock.normalize-payload", 1, false]),
            json!(["system", "corqty.mock.produce-summary", 1, false]),
            json!(["system", "corqty.mock.task", 1, false]),
            json!(["system", "corqty.shell.command", 1, false]),
            json!(["system", "corqty.simulated.create-rds", 1, false]),
            json!(["system", "corqty.simulated.create-vpc", 1, false]),
            json!(["system", "corqty.simulated.deploy-service", 1, false]),
        ]
    );

    let event = |action: &str, id: &str, version: i32| {
        json!([
            editor.user_id,
            action,
            "node_definition",
            id,
            { "key": "acme.thing", "version": version },
        ])
    };
    assert_eq!(
        catalog_events(&editor, organization_id).await,
        [
            event("node_definition.deprecated", &ids[0], 1),
            event("node_definition.created", &ids[1], 2),
            event("node_definition.created", &ids[0], 1),
        ]
    );
}

#[tokio::test]
#[serial]
async fn editors_add_blueprints_whose_graphs_pass_publish_validation() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let editor = editor_of(&admin).await;
    let organization_id = admin.organization_id;
    let blueprints = format!("/api/v1/orgs/{organization_id}/blueprints");

    let system = body(editor.get(&blueprints).await, StatusCode::OK).await;
    assert_eq!(
        items(&system, &["scope", "key"]),
        [
            json!(["system", "corqty.data-processing"]),
            json!(["system", "corqty.simulated-infrastructure"]),
        ]
    );
    let graph = system["items"][0]["graph"].clone();

    let pipeline = json!({
        "key": "acme.pipeline",
        "displayName": "Pipeline",
        "description": "Normalizes and summarizes a payload.",
        "graph": graph,
    });
    let (id, record) = created(editor.post_json(&blueprints, &pipeline.to_string()).await).await;
    assert_eq!(
        record,
        json!({
            "scope": "organization",
            "key": "acme.pipeline",
            "version": 1,
            "displayName": "Pipeline",
            "description": "Normalizes and summarizes a payload.",
            "graph": graph,
        })
    );

    let mut invalid = pipeline.clone();
    invalid["key"] = json!("Acme Pipeline");
    invalid["graph"]["nodes"][0]["definition"]["key"] = json!("acme.missing");
    assert_eq!(
        diagnostics(
            editor.post_json(&blueprints, &invalid.to_string()).await,
            "blueprint_invalid",
        )
        .await,
        json!([
            ["invalid_key", "/key"],
            ["unknown_definition", "/graph/nodes/0/definition"],
        ])
    );

    let listed = body(editor.get(&blueprints).await, StatusCode::OK).await;
    assert_eq!(
        items(&listed, &["scope", "key", "version"]),
        [
            json!(["organization", "acme.pipeline", 1]),
            json!(["system", "corqty.data-processing", 1]),
            json!(["system", "corqty.simulated-infrastructure", 1]),
        ]
    );
    assert_eq!(
        catalog_events(&editor, organization_id).await,
        [json!([
            editor.user_id,
            "blueprint.created",
            "blueprint",
            id,
            { "key": "acme.pipeline", "version": 1 },
        ])]
    );
}

/// Posts `request` to `path` while another transaction holds an uncommitted version 1 of the
/// same key, which `insert` adds with the organization id as $1. The request computes
/// version 1 too and waits on that row's unique key; the other transaction then commits.
async fn post_during_insert(
    owner: &PgPool,
    admin: &AuthFixture,
    path: &str,
    request: &str,
    insert: &str,
) -> Response {
    let mut other = owner.begin().await.unwrap();
    sqlx::query(insert)
        .bind(admin.organization_id)
        .execute(&mut *other)
        .await
        .unwrap();
    let mut response = pin!(admin.post_json(path, request));
    let waiting =
        "SELECT EXISTS (SELECT 1 FROM pg_locks WHERE locktype = 'transactionid' AND NOT granted)";
    tokio::select! {
        response = &mut response => panic!("the request did not wait: {}", response.status()),
        () = async {
            while !sqlx::query_scalar::<_, bool>(waiting).fetch_one(owner).await.unwrap() {}
        } => other.commit().await.unwrap(),
    }
    response.await
}

#[tokio::test]
#[serial]
async fn a_concurrent_version_of_the_same_key_is_a_conflict() {
    // `registered` resets the database again; this pool only has to stay connected.
    let owner = TestDatabase::reset().await.owner;
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let definitions = format!("/api/v1/orgs/{}/node-definitions", admin.organization_id);
    let thing = definition("acme.thing").to_string();
    let insert = "INSERT INTO node_definitions(
            organization_id, scope, key, version, display_name, category, spec
         )
         VALUES ($1, 'organization', 'acme.thing', 1, 'Thing', 'Testing', '{}')";
    let response = post_during_insert(&owner, &admin, &definitions, &thing, insert).await;
    let conflict = body(response, StatusCode::CONFLICT).await;
    assert_eq!(conflict["code"], "node_definition_version_conflict");
    assert_eq!(
        catalog_events(&admin, admin.organization_id).await,
        Vec::<Value>::new()
    );

    let (_, retried) = created(admin.post_json(&definitions, &thing).await).await;
    assert_eq!(retried["version"], 2);
}

#[tokio::test]
#[serial]
async fn a_concurrent_blueprint_of_the_same_key_is_a_conflict() {
    // `registered` resets the database again; this pool only has to stay connected.
    let owner = TestDatabase::reset().await.owner;
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let blueprints = format!("/api/v1/orgs/{}/blueprints", admin.organization_id);
    let flow = json!({
        "key": "acme.flow",
        "displayName": "Flow",
        "graph": {
            "schemaVersion": 1,
            "workflowInputSchema": { "type": "object", "properties": {} },
            "nodes": [],
            "edges": [],
            "viewport": { "x": 0, "y": 0, "zoom": 1 },
        },
    })
    .to_string();
    let insert = "INSERT INTO blueprints(organization_id, scope, key, version, display_name, graph)
         VALUES ($1, 'organization', 'acme.flow', 1, 'Flow', '{}')";
    let response = post_during_insert(&owner, &admin, &blueprints, &flow, insert).await;
    let conflict = body(response, StatusCode::CONFLICT).await;
    assert_eq!(conflict["code"], "blueprint_version_conflict");
    assert_eq!(
        catalog_events(&admin, admin.organization_id).await,
        Vec::<Value>::new()
    );

    let (_, retried) = created(admin.post_json(&blueprints, &flow).await).await;
    assert_eq!(retried["version"], 2);
}
