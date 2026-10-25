mod support;

use std::collections::{HashMap, HashSet};

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};
use serial_test::serial;
use support::AuthFixture;
use tower::ServiceExt;
use uuid::Uuid;

const INFRASTRUCTURE_BLUEPRINT_ID: &str = "00000000-0000-7000-8000-000000000101";

fn empty_graph() -> Value {
    json!({
        "schemaVersion": 1,
        "workflowInputSchema": { "type": "object", "properties": {} },
        "nodes": [],
        "edges": [],
        "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
    })
}

/// A mock task node; only the number of nodes matters to the limits.
fn node(id: &str) -> Value {
    json!({
        "id": id,
        "definition": { "key": "corqty.mock.task", "version": 1 },
        "displayName": "Mock task",
        "position": { "x": 0, "y": 0 },
        "config": {},
        "inputs": {},
        "retry": { "maxAttempts": 1 },
        "timeoutSeconds": 900,
    })
}

/// Asserts the status and returns the JSON body.
async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// The (code, pointer) pairs of a diagnostics array.
fn pairs(diagnostics: &Value) -> Vec<Value> {
    diagnostics
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| json!([diagnostic["code"], diagnostic["pointer"]]))
        .collect()
}

/// The name and draft revision of each listed workflow, in list order.
async fn listed(member: &AuthFixture, workflows: &str) -> Vec<Value> {
    let listed = body(member.get(workflows).await, StatusCode::OK).await;
    listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| json!([item["name"], item["draftRevision"]]))
        .collect()
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

/// The node and edge ids of a graph.
fn ids(graph: &Value) -> HashSet<String> {
    ["nodes", "edges"]
        .iter()
        .flat_map(|list| graph[*list].as_array().unwrap())
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .collect()
}

/// The graph with each node id replaced by its definition key and without edge ids, so
/// graphs that differ only in their ids compare equal. Panics on an edge to a missing node.
fn by_definition_key(graph: &Value) -> Value {
    let mut graph = graph.clone();
    let keys: HashMap<String, Value> = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            let id = node["id"].as_str().unwrap().to_owned();
            (id, node["definition"]["key"].clone())
        })
        .collect();
    for node in graph["nodes"].as_array_mut().unwrap() {
        node["id"] = node["definition"]["key"].clone();
    }
    for edge in graph["edges"].as_array_mut().unwrap() {
        edge.as_object_mut().unwrap().remove("id");
        for end in ["source", "target"] {
            edge[end]["nodeId"] = keys[edge[end]["nodeId"].as_str().unwrap()].clone();
        }
    }
    graph
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
async fn editors_create_empty_workflows_and_apply_blueprints() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let editor = editor_of(&admin).await;
    let organization_id = admin.organization_id;
    let workflows = format!("/api/v1/orgs/{organization_id}/workflows");

    let empty = body(
        editor
            .post_json(&workflows, r#"{"name":"  Nightly report  "}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let empty_id = empty["id"].as_str().unwrap();
    let slug = empty["slug"].as_str().unwrap();
    // The name's words, then an 8-character random suffix.
    assert!(slug.starts_with("nightly-report-"), "{slug}");
    assert_eq!(slug.len(), "nightly-report-".len() + 8, "{slug}");
    assert!(empty["draft"]["updatedAt"].is_string());
    assert_eq!(
        empty,
        json!({
            "id": empty_id,
            "name": "Nightly report",
            "slug": slug,
            "publishedVersion": null,
            "publishedDraftRevision": null,
            "draft": {
                "revision": 1,
                "graph": empty_graph(),
                "updatedAt": empty["draft"]["updatedAt"],
            },
        })
    );
    let path = format!("{workflows}/{empty_id}");
    assert_eq!(body(editor.get(&path).await, StatusCode::OK).await, empty);

    for name in ["   ".to_owned(), "x".repeat(101)] {
        let request = json!({ "name": name }).to_string();
        let problem = body(
            editor.post_json(&workflows, &request).await,
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
        assert_eq!(problem["code"], "invalid_workflow_name");
    }
    let unknown = json!({ "name": "Copy", "blueprintId": Uuid::new_v4() }).to_string();
    let response = editor.post_json(&workflows, &unknown).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let blueprints = format!("/api/v1/orgs/{organization_id}/blueprints");
    let blueprint = body(editor.get(&blueprints).await, StatusCode::OK).await["items"][1].clone();
    assert_eq!(blueprint["id"], INFRASTRUCTURE_BLUEPRINT_ID);
    let request = json!({ "name": "Provision staging", "blueprintId": blueprint["id"] });
    let applied = body(
        editor.post_json(&workflows, &request.to_string()).await,
        StatusCode::CREATED,
    )
    .await;
    let graph = &applied["draft"]["graph"];
    // Fresh UUIDs that the blueprint does not use, wired up exactly like the blueprint.
    let (old, new) = (ids(&blueprint["graph"]), ids(graph));
    assert_eq!(new.len(), old.len());
    for id in &new {
        assert!(Uuid::parse_str(id).is_ok(), "{id}");
        assert!(!old.contains(id), "{id}");
    }
    assert_eq!(
        by_definition_key(graph),
        by_definition_key(&blueprint["graph"])
    );

    assert_eq!(
        workflow_events(&editor, organization_id).await,
        [
            json!([
                editor.user_id,
                "workflow.created",
                applied["id"],
                { "blueprintId": INFRASTRUCTURE_BLUEPRINT_ID },
            ]),
            json!([editor.user_id, "workflow.created", empty_id, {}]),
        ]
    );
}

#[tokio::test]
#[serial]
async fn drafts_save_only_at_the_current_revision() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let workflows = format!("/api/v1/orgs/{}/workflows", admin.organization_id);
    let mut created = Vec::new();
    for name in ["First", "Second"] {
        let request = json!({ "name": name }).to_string();
        let workflow = body(
            admin.post_json(&workflows, &request).await,
            StatusCode::CREATED,
        )
        .await;
        created.push(workflow["id"].as_str().unwrap().to_owned());
    }
    assert_eq!(
        listed(&admin, &workflows).await,
        [json!(["Second", 1]), json!(["First", 1])]
    );

    let first = format!("{workflows}/{}", created[0]);
    let draft = format!("{first}/draft");
    let mut graph = empty_graph();
    graph["workflowInputSchema"]["properties"]["region"] = json!({ "type": "string" });
    graph["viewport"] = json!({ "x": 12.5, "y": -40.0, "zoom": 1.5 });
    for revision in [1, 2] {
        let request = json!({ "revision": revision, "graph": graph }).to_string();
        let saved = body(
            admin.request_json(Method::PUT, &draft, &request).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(saved, json!({ "revision": revision + 1 }));
    }
    let stored = body(admin.get(&first).await, StatusCode::OK).await;
    assert_eq!(stored["draft"]["revision"], 3);
    assert_eq!(stored["draft"]["graph"], graph);
    // Saving moved the first workflow to the top.
    assert_eq!(
        listed(&admin, &workflows).await,
        [json!(["First", 3]), json!(["Second", 1])]
    );

    let stale = json!({ "revision": 2, "graph": graph }).to_string();
    let conflict = body(
        admin.request_json(Method::PUT, &draft, &stale).await,
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(conflict["code"], "draft_revision_conflict");

    let mut large = graph.clone();
    large["nodes"] = (0..501).map(|index| node(&format!("n{index}"))).collect();
    let request = json!({ "revision": 3, "graph": large }).to_string();
    assert_eq!(
        body(
            admin.request_json(Method::PUT, &draft, &request).await,
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await,
        json!({
            "type": "about:blank",
            "title": "Validation failed",
            "status": 422,
            "code": "graph_invalid",
            "diagnostics": [{
                "code": "too_many_nodes",
                "pointer": "/nodes",
                "message": "A graph can have at most 500 nodes.",
            }],
        })
    );

    // The session cookie alone is not enough to write.
    let current = json!({ "revision": 3, "graph": graph }).to_string();
    let response = admin
        .app
        .clone()
        .oneshot(
            Request::put(&draft)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, admin.cookie_header())
                .body(Body::from(current.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let unknown = format!("{workflows}/{}/draft", Uuid::new_v4());
    let response = admin.request_json(Method::PUT, &unknown, &current).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Only the two successful saves changed the draft or wrote an audit event.
    let stored = body(admin.get(&first).await, StatusCode::OK).await;
    assert_eq!(stored["draft"]["revision"], 3);
    assert_eq!(
        workflow_events(&admin, admin.organization_id).await,
        [
            json!([admin.user_id, "workflow.draft_saved", created[0], { "revision": 3 }]),
            json!([admin.user_id, "workflow.draft_saved", created[0], { "revision": 2 }]),
            json!([admin.user_id, "workflow.created", created[1], {}]),
            json!([admin.user_id, "workflow.created", created[0], {}]),
        ]
    );
}

#[tokio::test]
#[serial]
async fn validation_reports_diagnostics_without_saving() {
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
    let path = format!("{workflows}/{}", applied["id"].as_str().unwrap());
    let validate = format!("{path}/validate");

    let graph = applied["draft"]["graph"].clone();
    let request = json!({ "graph": graph }).to_string();
    assert_eq!(
        body(editor.post_json(&validate, &request).await, StatusCode::OK).await,
        json!({ "valid": true, "diagnostics": [] })
    );

    let mut invalid = graph;
    invalid["nodes"][0]["definition"]["key"] = json!("acme.missing");
    invalid["nodes"][1]["retry"]["maxAttempts"] = json!(9);
    let request = json!({ "graph": invalid }).to_string();
    let report = body(editor.post_json(&validate, &request).await, StatusCode::OK).await;
    assert_eq!(report["valid"], false);
    assert_eq!(
        pairs(&report["diagnostics"]),
        [
            json!(["unknown_definition", "/nodes/0/definition"]),
            json!(["invalid_retry", "/nodes/1/retry/maxAttempts"]),
        ]
    );

    // Validating wrote nothing: the same draft and revision, and no new audit event.
    assert_eq!(body(editor.get(&path).await, StatusCode::OK).await, applied);
    assert_eq!(
        workflow_events(&editor, admin.organization_id).await.len(),
        1
    );
}

#[tokio::test]
#[serial]
async fn workflows_of_another_organization_are_not_found() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let beta = body(
        admin
            .post_json("/api/v1/organizations", r#"{"name":"Beta"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let beta = format!("/api/v1/orgs/{}", beta["organizationId"].as_str().unwrap());
    let foreign = body(
        admin
            .post_json(&format!("{beta}/workflows"), r#"{"name":"Beta flow"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let blueprint = json!({ "key": "beta.empty", "displayName": "Empty", "graph": empty_graph() });
    let foreign_blueprint = body(
        admin
            .post_json(&format!("{beta}/blueprints"), &blueprint.to_string())
            .await,
        StatusCode::CREATED,
    )
    .await;

    // The admin of both organizations asks through Acme for Beta's workflow and blueprint.
    let workflows = format!("/api/v1/orgs/{}/workflows", admin.organization_id);
    let foreign_id = foreign["id"].as_str().unwrap();
    let foreign_path = format!("{workflows}/{foreign_id}");
    let graph = &foreign["draft"]["graph"];
    let copy = json!({ "name": "Copy", "blueprintId": foreign_blueprint["id"] });
    for (method, path, request) in [
        (Method::GET, foreign_path.clone(), String::new()),
        (
            Method::PUT,
            format!("{foreign_path}/draft"),
            json!({ "revision": 1, "graph": graph }).to_string(),
        ),
        (
            Method::POST,
            format!("{foreign_path}/validate"),
            json!({ "graph": graph }).to_string(),
        ),
        (
            Method::GET,
            format!("{foreign_path}/versions"),
            String::new(),
        ),
        (Method::POST, workflows.clone(), copy.to_string()),
    ] {
        let response = admin.request_json(method.clone(), &path, &request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
    }
    let unchanged = format!("{beta}/workflows/{foreign_id}");
    assert_eq!(
        body(admin.get(&unchanged).await, StatusCode::OK).await,
        foreign
    );

    // Acme lists only its own workflow, which has no versions yet.
    let own = body(
        admin.post_json(&workflows, r#"{"name":"Acme flow"}"#).await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(
        body(admin.get(&workflows).await, StatusCode::OK).await,
        json!({
            "items": [{
                "id": own["id"],
                "name": "Acme flow",
                "slug": own["slug"],
                "draftRevision": 1,
                "publishedVersion": null,
                "publishedDraftRevision": null,
                "updatedAt": own["draft"]["updatedAt"],
            }],
        })
    );
    let versions = format!("{workflows}/{}/versions", own["id"].as_str().unwrap());
    assert_eq!(
        body(admin.get(&versions).await, StatusCode::OK).await,
        json!({ "items": [] })
    );
}
