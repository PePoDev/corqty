use axum::{Extension, Json, extract::State, http::StatusCode};
use corqty_domain::{
    catalog::{NewNodeDefinition, validate_blueprint_metadata, validate_new_definition},
    diagnostic::Diagnostic,
    graph::GraphSnapshot,
    publish::validate_graph,
    rbac::Capability,
};
use corqty_persistence::{
    catalog::{self, BlueprintRecord, NodeDefinitionRecord},
    models::SessionRecord,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authz::authorize,
    error::ApiError,
    extract::{ApiJson, ApiPath},
    state::AppState,
};

/// A list response: `{"items": [...]}`.
#[derive(Serialize)]
pub struct Items<T> {
    pub items: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewBlueprint {
    key: String,
    display_name: String,
    #[serde(default)]
    description: String,
    graph: GraphSnapshot,
}

pub async fn list_node_definitions(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
) -> Result<Json<Items<NodeDefinitionRecord>>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let items = catalog::list_node_definitions(&mut access.tx).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}

pub async fn create_node_definition(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
    ApiJson(definition): ApiJson<NewNodeDefinition>,
) -> Result<(StatusCode, Json<NodeDefinitionRecord>), ApiError> {
    let mut access =
        authorize(&state, &session, organization_id, Capability::EditWorkflows).await?;
    let diagnostics = validate_new_definition(&definition);
    if !diagnostics.is_empty() {
        return Err(ApiError::Invalid {
            code: "node_definition_invalid",
            diagnostics,
        });
    }
    let record = catalog::insert_node_definition(
        &mut access.tx,
        organization_id,
        access.user_id,
        &definition,
    )
    .await
    .map_err(map_version_conflict)?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(record)))
}

/// Only the organization's own definitions can be deprecated: system and foreign ids are 404.
pub async fn deprecate_node_definition(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, definition_id)): ApiPath<(Uuid, Uuid)>,
) -> Result<Json<NodeDefinitionRecord>, ApiError> {
    let mut access =
        authorize(&state, &session, organization_id, Capability::EditWorkflows).await?;
    let record = catalog::deprecate_node_definition(
        &mut access.tx,
        organization_id,
        access.user_id,
        definition_id,
    )
    .await?
    .ok_or(ApiError::NotFound)?;
    access.commit().await?;
    Ok(Json(record))
}

pub async fn list_blueprints(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
) -> Result<Json<Items<BlueprintRecord>>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let items = catalog::list_blueprints(&mut access.tx).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}

/// A blueprint's graph must pass the same validation as a publish, against the definitions
/// the organization can use.
pub async fn create_blueprint(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
    ApiJson(blueprint): ApiJson<NewBlueprint>,
) -> Result<(StatusCode, Json<BlueprintRecord>), ApiError> {
    let mut access =
        authorize(&state, &session, organization_id, Capability::EditWorkflows).await?;
    let mut diagnostics = validate_blueprint_metadata(
        &blueprint.key,
        &blueprint.display_name,
        &blueprint.description,
    );
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    diagnostics.extend(
        validate_graph(&blueprint.graph, &definitions)
            .into_iter()
            .map(|diagnostic| Diagnostic {
                pointer: format!("/graph{}", diagnostic.pointer),
                ..diagnostic
            }),
    );
    if !diagnostics.is_empty() {
        return Err(ApiError::Invalid {
            code: "blueprint_invalid",
            diagnostics,
        });
    }
    let record = catalog::insert_blueprint(
        &mut access.tx,
        organization_id,
        access.user_id,
        &blueprint.key,
        &blueprint.display_name,
        &blueprint.description,
        &blueprint.graph,
    )
    .await?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(record)))
}

/// Two requests computed the same next version; the later insert fails on the unique key.
fn map_version_conflict(error: sqlx::Error) -> ApiError {
    if error
        .as_database_error()
        .is_some_and(|database| database.constraint() == Some("node_definitions_key_version"))
    {
        ApiError::Conflict("node_definition_version_conflict")
    } else {
        error.into()
    }
}
