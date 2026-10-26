use std::collections::HashMap;

use axum::{Extension, Json, extract::State, http::StatusCode};
use corqty_domain::{
    diagnostic::Diagnostic,
    graph::{GraphSnapshot, check_limits},
    publish::{Definitions, canonical_checksum, shell_diagnostics, validate_graph},
    rbac::Capability,
    slug::slugify,
};
use corqty_persistence::{
    catalog,
    models::SessionRecord,
    workflows::{self, VersionRecord, VersionSummaryRecord, WorkflowRecord, WorkflowSummaryRecord},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    authz::authorize,
    catalog::{Items, blocking},
    error::ApiError,
    extract::{ApiJson, ApiPath},
    organizations::settings::shell_allowed,
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewWorkflow {
    name: String,
    blueprint_id: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftSave {
    revision: i64,
    graph: GraphSnapshot,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphBody {
    graph: GraphSnapshot,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishDraft {
    revision: i64,
}

pub async fn list_workflows(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
) -> Result<Json<Items<WorkflowSummaryRecord>>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let items = workflows::list_workflows(&mut access.tx).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}

/// Creates an empty workflow, or one whose draft is a copy of a blueprint's graph.
pub async fn create_workflow(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
    ApiJson(workflow): ApiJson<NewWorkflow>,
) -> Result<(StatusCode, Json<WorkflowRecord>), ApiError> {
    let mut access =
        authorize(&state, &session, organization_id, Capability::EditWorkflows).await?;
    let name = workflow.name.trim();
    if !(1..=100).contains(&name.chars().count()) {
        return Err(ApiError::Validation("invalid_workflow_name"));
    }
    let graph = match workflow.blueprint_id {
        Some(blueprint_id) => {
            let blueprint = catalog::get_blueprint(&mut access.tx, blueprint_id)
                .await?
                .ok_or(ApiError::NotFound)?;
            with_fresh_ids(blueprint.graph)
        }
        None => GraphSnapshot::empty(),
    };
    let slug = slugify(name, "workflow", Uuid::new_v4());
    let record = workflows::insert_workflow(
        &mut access.tx,
        organization_id,
        access.user_id,
        name,
        &slug,
        &graph,
        workflow.blueprint_id,
    )
    .await?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(record)))
}

pub async fn get_workflow(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id)): ApiPath<(Uuid, Uuid)>,
) -> Result<Json<WorkflowRecord>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let record = workflows::get_workflow(&mut access.tx, workflow_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    access.commit().await?;
    Ok(Json(record))
}

/// Replaces the draft only while it is still at the revision the client last saw.
/// Full validation waits for validate and publish; a save only enforces the size limits.
pub async fn save_draft(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(draft): ApiJson<DraftSave>,
) -> Result<Json<Value>, ApiError> {
    let mut access =
        authorize(&state, &session, organization_id, Capability::EditWorkflows).await?;
    let diagnostics = check_limits(&draft.graph);
    if !diagnostics.is_empty() {
        return Err(ApiError::Invalid {
            code: "graph_invalid",
            diagnostics,
        });
    }
    let saved = workflows::save_draft(
        &mut access.tx,
        organization_id,
        access.user_id,
        workflow_id,
        draft.revision,
        &draft.graph,
    )
    .await?;
    let Some(revision) = saved else {
        return Err(
            if workflows::workflow_exists(&mut access.tx, workflow_id).await? {
                ApiError::Conflict("draft_revision_conflict")
            } else {
                ApiError::NotFound
            },
        );
    };
    access.commit().await?;
    Ok(Json(json!({ "revision": revision })))
}

/// Checks a graph as a publish would, shell nodes included, without storing anything.
pub async fn validate(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(body): ApiJson<GraphBody>,
) -> Result<Json<Value>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::PublishWorkflows,
    )
    .await?;
    if !workflows::workflow_exists(&mut access.tx, workflow_id).await? {
        return Err(ApiError::NotFound);
    }
    let shell_enabled = shell_allowed(&state, &mut access).await?;
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    let (_, diagnostics) = blocking(body.graph, move |graph| {
        publish_diagnostics(graph, &definitions, shell_enabled)
    })
    .await?;
    access.commit().await?;
    Ok(Json(json!({
        "valid": diagnostics.is_empty(),
        "diagnostics": diagnostics,
    })))
}

pub async fn list_versions(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id)): ApiPath<(Uuid, Uuid)>,
) -> Result<Json<Items<VersionSummaryRecord>>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    if !workflows::workflow_exists(&mut access.tx, workflow_id).await? {
        return Err(ApiError::NotFound);
    }
    let items = workflows::list_versions(&mut access.tx, workflow_id).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}

/// Publishes the stored draft, at exactly the revision the client saw, as the workflow's
/// next version. The workflow stays locked until the commit, so publishes of one workflow
/// run one at a time and each draft revision becomes at most one version.
pub async fn publish(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(request): ApiJson<PublishDraft>,
) -> Result<(StatusCode, Json<VersionSummaryRecord>), ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::PublishWorkflows,
    )
    .await?;
    let draft = workflows::lock_workflow(&mut access.tx, workflow_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if draft.revision != request.revision {
        return Err(ApiError::Conflict("draft_revision_conflict"));
    }
    if workflows::version_exists_for_revision(&mut access.tx, workflow_id, draft.revision).await? {
        return Err(ApiError::Conflict("draft_already_published"));
    }
    let shell_enabled = shell_allowed(&state, &mut access).await?;
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    let (graph, diagnostics) = blocking(draft.graph, move |graph| {
        publish_diagnostics(graph, &definitions, shell_enabled)
    })
    .await?;
    if !diagnostics.is_empty() {
        return Err(ApiError::Invalid {
            code: "graph_invalid",
            diagnostics,
        });
    }
    let version = workflows::insert_version(
        &mut access.tx,
        organization_id,
        access.user_id,
        workflow_id,
        draft.revision,
        &graph,
        &canonical_checksum(&graph),
    )
    .await
    .map_err(map_already_published)?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(version)))
}

pub async fn get_version(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id, version)): ApiPath<(Uuid, Uuid, i32)>,
) -> Result<Json<VersionRecord>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let record = workflows::get_version(&mut access.tx, workflow_id, version)
        .await?
        .ok_or(ApiError::NotFound)?;
    access.commit().await?;
    Ok(Json(record))
}

/// Every reason the organization cannot publish `graph`: the publish rules, then each shell
/// node while the organization may not use shell (see `shell_allowed`).
fn publish_diagnostics(
    graph: &GraphSnapshot,
    definitions: &Definitions,
    shell_enabled: bool,
) -> Vec<Diagnostic> {
    let mut diagnostics = validate_graph(graph, definitions);
    if !shell_enabled {
        diagnostics.extend(shell_diagnostics(graph, definitions));
    }
    diagnostics
}

/// The unique key that keeps a draft revision from becoming two versions. The workflow
/// lock already stops a second publish before it gets here; this keeps the answer a 409
/// if that ever changes.
fn map_already_published(error: sqlx::Error) -> ApiError {
    if error
        .as_database_error()
        .is_some_and(|database| database.constraint() == Some("workflow_versions_draft_revision"))
    {
        ApiError::Conflict("draft_already_published")
    } else {
        error.into()
    }
}

/// A blueprint's graph with new UUIDs for its nodes and edges, the edges following
/// their nodes, so a workflow never shares ids with the blueprint it came from.
fn with_fresh_ids(mut graph: GraphSnapshot) -> GraphSnapshot {
    let mut ids = HashMap::new();
    for node in &mut graph.nodes {
        let id = Uuid::new_v4().to_string();
        ids.insert(std::mem::replace(&mut node.id, id.clone()), id);
    }
    for edge in &mut graph.edges {
        edge.id = Uuid::new_v4().to_string();
        for end in [&mut edge.source, &mut edge.target] {
            if let Some(id) = ids.get(&end.node_id) {
                end.node_id.clone_from(id);
            }
        }
    }
    graph
}
