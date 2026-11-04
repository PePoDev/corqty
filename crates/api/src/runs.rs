use axum::{Extension, Json, extract::State, http::StatusCode};
use corqty_domain::{
    diagnostic::Diagnostic,
    publish::shell_diagnostics,
    rbac::Capability,
    run::plan_run,
    runtime::MAX_RUN_INPUT_BYTES,
    schema::{compile, instance_errors},
};
use corqty_persistence::{
    catalog,
    models::SessionRecord,
    runs::{self, CancelError, NewRun, RunRecord, RunSummaryRecord},
    workflows,
};
use serde::Deserialize;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{
    authz::{OrgAccess, authorize},
    catalog::{Items, blocking},
    error::ApiError,
    extract::{ApiJson, ApiPath},
    organizations::settings::shell_allowed,
    state::AppState,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRun {
    #[serde(default)]
    input: Map<String, Value>,
    version: Option<i32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rerun {
    version: Option<i32>,
}

pub async fn list_runs(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id)): ApiPath<(Uuid, Uuid)>,
) -> Result<Json<Items<RunSummaryRecord>>, ApiError> {
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
    let items = runs::list_runs(&mut access.tx, workflow_id).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}

/// Starts the published version, or the version the request names, with an input that is
/// at most 1 MiB of JSON and matches that version's workflow input schema.
pub async fn start_run(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(request): ApiJson<StartRun>,
) -> Result<(StatusCode, Json<RunRecord>), ApiError> {
    let mut access = authorize(&state, &session, organization_id, Capability::RunWorkflows).await?;
    let input = Value::Object(request.input);
    if input.to_string().len() > MAX_RUN_INPUT_BYTES {
        return Err(ApiError::Validation("run_input_too_large"));
    }
    let workflow = workflows::get_workflow(&mut access.tx, workflow_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let version = request
        .version
        .or(workflow.published_version)
        .ok_or(ApiError::Conflict("workflow_not_published"))?;
    let run = create(&state, &mut access, workflow_id, version, input, None).await?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(run)))
}

pub async fn get_run(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, run_id)): ApiPath<(Uuid, Uuid)>,
) -> Result<Json<RunRecord>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let run = runs::get_run(&mut access.tx, run_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    access.commit().await?;
    Ok(Json(run))
}

/// Starts the run's workflow again with the run's input, on the run's version unless the
/// request names another one, whose schema the input must then match.
pub async fn rerun(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, run_id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(request): ApiJson<Rerun>,
) -> Result<(StatusCode, Json<RunRecord>), ApiError> {
    let mut access = authorize(&state, &session, organization_id, Capability::RunWorkflows).await?;
    let source = runs::get_run(&mut access.tx, run_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let version = request.version.unwrap_or(source.summary.version);
    let run = create(
        &state,
        &mut access,
        source.summary.workflow_id,
        version,
        source.input,
        Some(run_id),
    )
    .await?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(run)))
}

/// Cancels the run's waiting nodes now; running nodes stop when their workers see the
/// request. The cancel has no body. A deadlock or serialization failure rolls the attempt
/// back, and the cancel is tried again up to three times.
pub async fn cancel_run(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, run_id)): ApiPath<(Uuid, Uuid)>,
) -> Result<Json<RunSummaryRecord>, ApiError> {
    let mut retries = 0;
    loop {
        let mut access =
            authorize(&state, &session, organization_id, Capability::RunWorkflows).await?;
        match runs::cancel_run(&mut access.tx, organization_id, access.user_id, run_id).await {
            Ok(run) => {
                access.commit().await?;
                return Ok(Json(run));
            }
            Err(CancelError::NotFound) => return Err(ApiError::NotFound),
            Err(CancelError::Finished) => return Err(ApiError::Conflict("run_finished")),
            Err(CancelError::Database(error)) if retries < 3 && is_transient(&error) => {
                retries += 1;
            }
            Err(CancelError::Database(error)) => return Err(error.into()),
        }
    }
}

/// Creates a run of the workflow's `version` from its plan and reads it back. `input` must
/// match that version's workflow input schema, and a version with shell nodes runs only while
/// the organization may use shell (see `shell_allowed`).
async fn create(
    state: &AppState,
    access: &mut OrgAccess,
    workflow_id: Uuid,
    version: i32,
    input: Value,
    rerun_of: Option<Uuid>,
) -> Result<RunRecord, ApiError> {
    let version = workflows::get_version(&mut access.tx, workflow_id, version)
        .await?
        .ok_or(ApiError::NotFound)?;
    let ((version, input), diagnostics) = blocking((version, input), |(version, input)| {
        input_diagnostics(&version.graph.workflow_input_schema, input)
    })
    .await?;
    // Publish compiled this schema, so a failure here is the server's.
    let diagnostics = diagnostics.map_err(ApiError::internal)?;
    if !diagnostics.is_empty() {
        return Err(ApiError::Invalid {
            code: "run_input_invalid",
            diagnostics,
        });
    }
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    // ponytail: no row lock, so a run that starts while an admin disables shell counts as
    // started just before; disabling never touches existing runs anyway.
    if !shell_allowed(state, access).await? {
        let diagnostics = shell_diagnostics(&version.graph, &definitions);
        if !diagnostics.is_empty() {
            return Err(ApiError::Invalid {
                code: "executor_disabled",
                diagnostics,
            });
        }
    }
    let nodes = plan_run(&version.graph, &definitions)
        .ok_or_else(|| ApiError::internal("a published version has no run plan"))?;
    let run = runs::create_run(
        &mut access.tx,
        &NewRun {
            organization_id: access.organization_id,
            actor_user_id: access.user_id,
            workflow_id,
            workflow_version_id: version.summary.id,
            version: version.summary.version,
            input: &input,
            nodes: &nodes,
            rerun_of,
        },
    )
    .await?;
    runs::get_run(&mut access.tx, run.id)
        .await?
        .ok_or_else(|| ApiError::internal("a new run is not visible"))
}

/// Where and how `input` breaks `schema`, one diagnostic per error.
fn input_diagnostics(schema: &Value, input: &Value) -> Result<Vec<Diagnostic>, String> {
    let validator = compile(schema)?;
    Ok(instance_errors(&validator, input)
        .into_iter()
        .map(|(pointer, message)| Diagnostic::new("invalid_input", pointer, message))
        .collect())
}

/// A deadlock or serialization failure: the transaction failed only because of its timing.
fn is_transient(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| code == "40P01" || code == "40001")
}
