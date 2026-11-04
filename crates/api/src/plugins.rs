use crate::{
    authz::authorize,
    catalog::Items,
    error::ApiError,
    extract::{ApiJson, ApiPath},
    state::AppState,
};
use axum::{Extension, Json, extract::State, http::StatusCode};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use corqty_domain::{
    catalog::{BUILTIN_EXECUTORS, NodeDefinitionSpec},
    diagnostic::Diagnostic,
    graph::GraphSnapshot,
    publish::Definitions,
    rbac::Capability,
};
use corqty_persistence::{
    models::SessionRecord,
    plugins::{self, PluginRecord},
};
use corqty_plugin_host::{
    PluginCapability, PluginManifest, RuntimePlugin,
    runtime::{MAX_COMPONENT_BYTES, checksum},
};
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Upload {
    manifest: PluginManifest,
    component_base64: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    checksum: String,
    capabilities: Vec<PluginCapability>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enabled {
    enabled: bool,
}

pub async fn list(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(org): ApiPath<Uuid>,
) -> Result<Json<Items<PluginRecord>>, ApiError> {
    let mut access = authorize(&state, &session, org, Capability::ManagePlugins).await?;
    let items = plugins::list(&mut access.tx).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}
pub async fn install(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(org): ApiPath<Uuid>,
    ApiJson(upload): ApiJson<Upload>,
) -> Result<(StatusCode, Json<PluginRecord>), ApiError> {
    let mut access = authorize(&state, &session, org, Capability::ManagePlugins).await?;
    if upload.component_base64.len() > MAX_COMPONENT_BYTES.div_ceil(3) * 4 {
        return Err(ApiError::PayloadTooLarge);
    }
    let bytes = STANDARD
        .decode(&upload.component_base64)
        .map_err(|_| ApiError::Validation("invalid_component"))?;
    let digest = checksum(&bytes);
    let plugin = RuntimePlugin::new(upload.manifest.clone(), bytes.clone(), vec![])
        .await
        .map_err(plugin_error)?;
    plugin.verify().await.map_err(plugin_error)?;
    let manifest = serde_json::to_value(&upload.manifest).map_err(ApiError::internal)?;
    let record = plugins::insert(
        &mut access.tx,
        plugins::NewPlugin {
            org,
            actor: access.user_id,
            key: &upload.manifest.key,
            version: &upload.manifest.version,
            host_contract: &upload.manifest.host_contract,
            manifest: &manifest,
            checksum: &digest,
            component: &bytes,
        },
    )
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .is_some_and(|e| e.is_unique_violation())
        {
            ApiError::Conflict("plugin_version_exists")
        } else {
            error.into()
        }
    })?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(record)))
}
pub async fn approve(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(body): ApiJson<Approval>,
) -> Result<Json<PluginRecord>, ApiError> {
    let mut access = authorize(&state, &session, org, Capability::ManagePlugins).await?;
    let record = plugins::get(&mut access.tx, id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let capabilities = serde_json::to_value(body.capabilities).map_err(ApiError::internal)?;
    if !state.config.shell_executor_available
        && capabilities
            .as_array()
            .is_some_and(|caps| caps.iter().any(|cap| cap["type"] == "command-exec"))
    {
        return Err(ApiError::Conflict("command_exec_unavailable"));
    }
    if body.checksum != record.checksum
        || record.manifest.get("capabilities") != Some(&capabilities)
    {
        return Err(ApiError::Conflict("plugin_approval_mismatch"));
    }
    let record =
        plugins::approve(&mut access.tx, org, access.user_id, &record, &capabilities).await?;
    access.commit().await?;
    Ok(Json(record))
}
pub async fn set_enabled(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(body): ApiJson<Enabled>,
) -> Result<Json<PluginRecord>, ApiError> {
    let mut access = authorize(&state, &session, org, Capability::ManagePlugins).await?;
    let record = plugins::set_enabled(&mut access.tx, org, access.user_id, id, body.enabled)
        .await?
        .ok_or(ApiError::NotFound)?;
    access.commit().await?;
    Ok(Json(record))
}

fn plugin_error(error: corqty_plugin_sdk::ExecutorError) -> ApiError {
    ApiError::Invalid {
        code: "plugin_invalid",
        diagnostics: vec![Diagnostic::new(&error.code, "", error.message)],
    }
}

/// Validates runtime identities against the caller's tenant-scoped catalog. The row share
/// lock makes publish/run creation atomic with plugin disabling.
pub async fn validate_runtime_graph(
    conn: &mut PgConnection,
    graph: &GraphSnapshot,
    definitions: &Definitions,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), ApiError> {
    for (index, node) in graph.nodes.iter().enumerate() {
        if diagnostics.len() > corqty_domain::diagnostic::MAX_DIAGNOSTICS {
            break;
        }
        let Some(spec) = definitions.get(&(node.definition.key.clone(), node.definition.version))
        else {
            continue;
        };
        validate_runtime_config(
            conn,
            spec,
            &node.config,
            &format!("/nodes/{index}"),
            diagnostics,
        )
        .await?;
    }
    *diagnostics = corqty_domain::diagnostic::capped(std::mem::take(diagnostics));
    Ok(())
}
pub async fn validate_runtime_config(
    conn: &mut PgConnection,
    spec: &NodeDefinitionSpec,
    config: &Value,
    prefix: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), ApiError> {
    if BUILTIN_EXECUTORS.contains(&(spec.executor.key.as_str(), spec.executor.version.as_str())) {
        return Ok(());
    }
    let identity_pointer = if prefix.is_empty() {
        "/executor".to_owned()
    } else {
        format!("{prefix}/definition")
    };
    let Some(payload) =
        plugins::available(conn, &spec.executor.key, &spec.executor.version).await?
    else {
        if !diagnostics
            .iter()
            .any(|d| d.code == "unknown_executor" && d.pointer == identity_pointer)
        {
            diagnostics.push(Diagnostic::new(
                "unknown_executor",
                identity_pointer,
                "The exact executor is missing, disabled or awaiting approval.",
            ));
        }
        return Ok(());
    };
    let manifest = serde_json::from_value(payload.manifest).map_err(ApiError::internal)?;
    let capabilities = serde_json::from_value(payload.capabilities).map_err(ApiError::internal)?;
    let plugin = RuntimePlugin::new(manifest, payload.component, capabilities)
        .await
        .map_err(plugin_error)?;
    diagnostics.retain(|d| !(d.code == "unknown_executor" && d.pointer == identity_pointer));
    let errors = plugin.validate(config).await.map_err(plugin_error)?;
    diagnostics.extend(errors.into_iter().map(|mut d| {
        let field = if prefix.is_empty() {
            "defaults"
        } else {
            "config"
        };
        d.pointer = format!("{prefix}/{field}{}", d.pointer);
        d
    }));
    Ok(())
}
