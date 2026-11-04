mod support;

use axum::{
    body::to_bytes,
    http::{Method, StatusCode},
    response::Response,
};
use corqty_domain::{graph::GraphSnapshot, publish::canonical_checksum};
use serde_json::{Value, json};
use serial_test::serial;
use support::AuthFixture;
use uuid::Uuid;

const INFRASTRUCTURE_BLUEPRINT_ID: &str = "00000000-0000-7000-8000-000000000101";

/// Asserts the status and returns the JSON body.
async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// The server's checksum of a graph as the API returned it.
fn checksum(graph: &Value) -> String {
    let graph: GraphSnapshot = serde_json::from_value(graph.clone()).unwrap();
    canonical_checksum(&graph)
}

/// The organization's workflow audit events, newest first, as
/// [actorUserId, action, resourceId, metadata].
async fn workflow_events(member: &AuthFixture, organization_id: Uuid) -> Vec<Value> {
    let path = format!("/api/v1/orgs/{organization_id}/audit-events");
    let events = body(member.get(&path).await, StatusCode::OK).await;
    events["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["resourceType"] == "workflow")
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
async fn editors_publish_each_draft_revision_once() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let editor = editor_of(&admin).await;
    let workflows = format!("/api/v1/orgs/{}/workflows", admin.organization_id);
    let request = json!({
        "name": "Provision staging",
        "blueprintId": INFRASTRUCTURE_BLUEPRINT_ID,
    });
    let applied = body(
        editor.post_json(&workflows, &request.to_string()).await,
        StatusCode::CREATED,
    )
    .await;
    let workflow_id = applied["id"].as_str().unwrap();
    let path = format!("{workflows}/{workflow_id}");
    let publish = format!("{path}/publish");

    let graph = applied["draft"]["graph"].clone();
    let first = body(
        editor.post_json(&publish, r#"{"revision":1}"#).await,
        StatusCode::CREATED,
    )
    .await;
    assert!(Uuid::parse_str(first["id"].as_str().unwrap()).is_ok());
    assert!(first["publishedAt"].is_string());
    assert_eq!(
        first,
        json!({
            "id": first["id"],
            "version": 1,
            "checksum": checksum(&graph),
            "draftRevision": 1,
            "publishedAt": first["publishedAt"],
        })
    );
    let again = body(
        editor.post_json(&publish, r#"{"revision":1}"#).await,
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(again["code"], "draft_already_published");

    // Once the draft moves on, its old revision can no longer be published.
    let mut edited = graph.clone();
    edited["viewport"]["zoom"] = json!(2.0);
    let save = json!({ "revision": 1, "graph": edited }).to_string();
    let saved = body(
        editor
            .request_json(Method::PUT, &format!("{path}/draft"), &save)
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(saved, json!({ "revision": 2 }));
    let stale = body(
        editor.post_json(&publish, r#"{"revision":1}"#).await,
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(stale["code"], "draft_revision_conflict");
    let second = body(
        editor.post_json(&publish, r#"{"revision":2}"#).await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(second["version"], 2);
    assert_eq!(second["draftRevision"], 2);
    assert_eq!(second["checksum"], checksum(&edited));
    // The viewport is part of the checksum.
    assert_ne!(checksum(&edited), checksum(&graph));

    // Versions are listed newest first, and each keeps the graph it was published from.
    let versions = format!("{path}/versions");
    assert_eq!(
        body(editor.get(&versions).await, StatusCode::OK).await,
        json!({ "items": [second, first] })
    );
    let mut version = first.clone();
    version["graph"] = graph;
    let detail = format!("{versions}/1");
    assert_eq!(
        body(editor.get(&detail).await, StatusCode::OK).await,
        version
    );
    let missing = editor.get(&format!("{versions}/3")).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let stored = body(editor.get(&path).await, StatusCode::OK).await;
    assert_eq!(stored["publishedVersion"], 2);
    assert_eq!(stored["publishedDraftRevision"], 2);

    // Only the two successful publishes wrote an audit event.
    let published = |number: u32, revision: u32, graph: &Value| {
        json!([
            editor.user_id,
            "workflow.published",
            workflow_id,
            { "version": number, "draftRevision": revision, "checksum": checksum(graph) },
        ])
    };
    assert_eq!(
        workflow_events(&editor, admin.organization_id).await,
        [
            published(2, 2, &edited),
            json!([editor.user_id, "workflow.draft_saved", workflow_id, { "revision": 2 }]),
            published(1, 1, &version["graph"]),
            json!([
                editor.user_id,
                "workflow.created",
                workflow_id,
                { "blueprintId": INFRASTRUCTURE_BLUEPRINT_ID },
            ]),
        ]
    );
}

#[tokio::test]
#[serial]
async fn invalid_and_foreign_drafts_are_not_published() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let beta = body(
        admin
            .post_json("/api/v1/organizations", r#"{"name":"Beta"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let beta = format!("/api/v1/orgs/{}", beta["organizationId"].as_str().unwrap());
    let definition = r#"{"key":"beta.thing","displayName":"Thing","category":"Testing",
        "inputs":[],"outputs":[],"configSchema":{"type":"object"},"uiSchema":{},"defaults":{},
        "executor":{"key":"mock","version":"1.0.0"}}"#;
    let created = admin
        .post_json(&format!("{beta}/node-definitions"), definition)
        .await;
    assert_eq!(created.status(), StatusCode::CREATED);

    let workflows = format!("/api/v1/orgs/{}/workflows", admin.organization_id);
    let created = body(
        admin.post_json(&workflows, r#"{"name":"Acme flow"}"#).await,
        StatusCode::CREATED,
    )
    .await;
    let path = format!("{workflows}/{}", created["id"].as_str().unwrap());
    let publish = format!("{path}/publish");
    // The empty draft is valid, so revision 1 becomes version 1.
    body(
        admin.post_json(&publish, r#"{"revision":1}"#).await,
        StatusCode::CREATED,
    )
    .await;

    // Revision 2 uses Beta's definition, which Acme's catalog does not contain.
    let mut graph = created["draft"]["graph"].clone();
    graph["nodes"] = json!([{
        "id": "thing",
        "definition": { "key": "beta.thing", "version": 1 },
        "displayName": "Thing",
        "position": { "x": 0, "y": 0 },
        "config": {},
        "inputs": {},
        "retry": { "maxAttempts": 1 },
        "timeoutSeconds": 900,
    }]);
    let save = json!({ "revision": 1, "graph": graph }).to_string();
    let saved = admin
        .request_json(Method::PUT, &format!("{path}/draft"), &save)
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let events = workflow_events(&admin, admin.organization_id).await;
    assert_eq!(
        body(
            admin.post_json(&publish, r#"{"revision":2}"#).await,
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await,
        json!({
            "type": "about:blank",
            "title": "Validation failed",
            "status": 422,
            "code": "graph_invalid",
            "diagnostics": [{
                "code": "unknown_definition",
                "pointer": "/nodes/0/definition",
                "message": "The node definition does not exist.",
            }],
        })
    );
    // Still one version, version 1 is still the published one, and no audit event was added.
    let versions = body(admin.get(&format!("{path}/versions")).await, StatusCode::OK).await;
    assert_eq!(versions["items"].as_array().unwrap().len(), 1);
    let stored = body(admin.get(&path).await, StatusCode::OK).await;
    assert_eq!(stored["draft"]["revision"], 2);
    assert_eq!(stored["publishedVersion"], 1);
    assert_eq!(stored["publishedDraftRevision"], 1);
    assert_eq!(workflow_events(&admin, admin.organization_id).await, events);

    // Beta's workflow has version 1 and an unpublished revision 2. Through Acme's URLs
    // neither is found, and an unknown workflow id is not found either.
    let foreign = body(
        admin
            .post_json(&format!("{beta}/workflows"), r#"{"name":"Beta flow"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let beta_path = format!("{beta}/workflows/{}", foreign["id"].as_str().unwrap());
    body(
        admin
            .post_json(&format!("{beta_path}/publish"), r#"{"revision":1}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let save = json!({ "revision": 1, "graph": foreign["draft"]["graph"] }).to_string();
    let saved = admin
        .request_json(Method::PUT, &format!("{beta_path}/draft"), &save)
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let foreign_path = format!("{workflows}/{}", foreign["id"].as_str().unwrap());
    let unknown_path = format!("{workflows}/{}", Uuid::new_v4());
    for (method, path, request) in [
        (
            Method::POST,
            format!("{foreign_path}/publish"),
            r#"{"revision":2}"#,
        ),
        (Method::GET, format!("{foreign_path}/versions/1"), ""),
        (
            Method::POST,
            format!("{unknown_path}/publish"),
            r#"{"revision":1}"#,
        ),
        (Method::GET, format!("{unknown_path}/versions/1"), ""),
    ] {
        let response = admin.request_json(method.clone(), &path, request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
    }
    let versions = format!("{beta_path}/versions");
    let versions = body(admin.get(&versions).await, StatusCode::OK).await;
    assert_eq!(versions["items"].as_array().unwrap().len(), 1);
}
