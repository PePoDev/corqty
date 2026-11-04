//! Organization automation, authenticated external actors, and durable dispatch.
use axum::{
    Extension, Json,
    body::Bytes,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::Next,
    response::Response,
};
use chrono::{DateTime, Utc};
use corqty_domain::{
    rbac::{Capability, Role},
    secrets::EncryptedSecret,
};
use corqty_persistence::{
    automation::{self, TriggerRecord},
    models::SessionRecord,
    runs::{self, RunOrigin},
    workflows,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    auth::crypto::SecretToken,
    authz::{OrgAccess, authorize},
    catalog::Items,
    error::ApiError,
    extract::{ApiJson, ApiPath},
    schedule::CronSchedule,
    state::AppState,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewToken {
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedToken {
    #[serde(flatten)]
    record: automation::ServiceTokenRecord,
    token: String,
}

pub async fn list_tokens(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(org): ApiPath<Uuid>,
) -> Result<Json<Items<automation::ServiceTokenRecord>>, ApiError> {
    let mut access = authorize(&state, &session, org, Capability::ManageServiceTokens).await?;
    let items = automation::tokens(&mut access.tx).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}

pub async fn create_token(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(org): ApiPath<Uuid>,
    ApiJson(body): ApiJson<NewToken>,
) -> Result<(StatusCode, Json<CreatedToken>), ApiError> {
    let mut access = authorize(&state, &session, org, Capability::ManageServiceTokens).await?;
    let name = name(&body.name)?;
    let token = SecretToken::generate()?;
    let record =
        automation::create_token(&mut access.tx, org, session.user_id, name, &token.hash).await?;
    automation::audit_actor(
        &mut access.tx,
        org,
        Some(session.user_id),
        "service_token.created",
        "service_token",
        record.id,
        json!({"scope":"workflow:run"}),
    )
    .await?;
    access.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedToken {
            record,
            token: token.plaintext,
        }),
    ))
}

pub async fn revoke_token(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let mut access = authorize(&state, &session, org, Capability::ManageServiceTokens).await?;
    if !automation::revoke_token(&mut access.tx, id).await? {
        return Err(ApiError::NotFound);
    }
    automation::audit_actor(
        &mut access.tx,
        org,
        Some(session.user_id),
        "service_token.revoked",
        "service_token",
        id,
        json!({}),
    )
    .await?;
    access.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TriggerRequest {
    name: String,
    workflow_id: Uuid,
    version: i32,
    kind: String,
    #[serde(default)]
    input: Map<String, Value>,
    #[serde(default = "yes")]
    enabled: bool,
    cron: Option<String>,
    timezone: Option<String>,
    #[serde(default = "skip")]
    overlap_policy: String,
}
fn yes() -> bool {
    true
}
fn skip() -> String {
    "skip".to_owned()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerResponse {
    #[serde(flatten)]
    record: TriggerRecord,
    #[serde(skip_serializing_if = "Option::is_none")]
    webhook_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    webhook_path: Option<String>,
}

pub async fn list_triggers(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(org): ApiPath<Uuid>,
) -> Result<Json<Items<TriggerRecord>>, ApiError> {
    let mut access = authorize(&state, &session, org, Capability::ViewOrganization).await?;
    let items = automation::triggers(&mut access.tx).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}

pub async fn create_trigger(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(org): ApiPath<Uuid>,
    ApiJson(body): ApiJson<TriggerRequest>,
) -> Result<(StatusCode, Json<TriggerResponse>), ApiError> {
    let mut access = authorize(&state, &session, org, Capability::ManageTriggers).await?;
    let (version_id, next) = validate_trigger(&mut access, &body).await?;
    let id = Uuid::now_v7();
    let plaintext = if body.kind == "webhook" {
        Some(SecretToken::generate()?.plaintext)
    } else {
        None
    };
    let encrypted = plaintext
        .as_ref()
        .map(|secret| state.config.master_key.encrypt(org, id, secret))
        .transpose()
        .map_err(ApiError::internal)?;
    let record: TriggerRecord = sqlx::query_as("INSERT INTO triggers(id,organization_id,name,workflow_id,workflow_version_id,version,kind,input,enabled,cron,timezone,overlap_policy,next_scheduled_at,webhook_ciphertext,webhook_nonce,webhook_key_version,created_by)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17) RETURNING *")
        .bind(id).bind(org).bind(body.name.trim()).bind(body.workflow_id).bind(version_id).bind(body.version).bind(&body.kind)
        .bind(Value::Object(body.input)).bind(body.enabled).bind(body.cron).bind(body.timezone).bind(body.overlap_policy).bind(next)
        .bind(encrypted.as_ref().map(|value| &value.ciphertext)).bind(encrypted.as_ref().map(|value| &value.nonce))
        .bind(encrypted.as_ref().map(|value| value.key_version)).bind(session.user_id).fetch_one(&mut *access.tx).await?;
    automation::audit_actor(
        &mut access.tx,
        org,
        Some(session.user_id),
        "trigger.created",
        "trigger",
        id,
        json!({"kind":body.kind,"version":body.version}),
    )
    .await?;
    access.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(TriggerResponse {
            record,
            webhook_path: plaintext.as_ref().map(|_| format!("/hooks/v1/{id}")),
            webhook_secret: plaintext,
        }),
    ))
}

pub async fn update_trigger(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(body): ApiJson<TriggerRequest>,
) -> Result<Json<TriggerRecord>, ApiError> {
    let mut access = authorize(&state, &session, org, Capability::ManageTriggers).await?;
    let existing = automation::trigger(&mut access.tx, id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if existing.kind != body.kind {
        return Err(ApiError::Validation("trigger_kind_immutable"));
    }
    let (version_id, next) = validate_trigger(&mut access, &body).await?;
    let record = sqlx::query_as("UPDATE triggers SET name=$2,workflow_id=$3,workflow_version_id=$4,version=$5,input=$6,enabled=$7,cron=$8,timezone=$9,overlap_policy=$10,next_scheduled_at=$11,updated_at=now() WHERE id=$1 RETURNING *")
        .bind(id).bind(body.name.trim()).bind(body.workflow_id).bind(version_id).bind(body.version).bind(Value::Object(body.input)).bind(body.enabled)
        .bind(body.cron).bind(body.timezone).bind(body.overlap_policy).bind(next).fetch_one(&mut *access.tx).await?;
    automation::audit_actor(
        &mut access.tx,
        org,
        Some(session.user_id),
        "trigger.updated",
        "trigger",
        id,
        json!({"version":body.version,"enabled":body.enabled}),
    )
    .await?;
    access.commit().await?;
    Ok(Json(record))
}

pub async fn delete_trigger(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let mut access = authorize(&state, &session, org, Capability::ManageTriggers).await?;
    if sqlx::query("DELETE FROM triggers WHERE id=$1")
        .bind(id)
        .execute(&mut *access.tx)
        .await?
        .rows_affected()
        == 0
    {
        return Err(ApiError::NotFound);
    }
    automation::audit_actor(
        &mut access.tx,
        org,
        Some(session.user_id),
        "trigger.deleted",
        "trigger",
        id,
        json!({}),
    )
    .await?;
    access.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn validate_trigger(
    access: &mut OrgAccess,
    body: &TriggerRequest,
) -> Result<(Uuid, Option<DateTime<Utc>>), ApiError> {
    name(&body.name)?;
    if !matches!(body.overlap_policy.as_str(), "skip" | "allow") {
        return Err(ApiError::Validation("invalid_overlap_policy"));
    }
    let next = match body.kind.as_str() {
        "cron" => Some(
            CronSchedule::parse(
                body.cron
                    .as_deref()
                    .ok_or(ApiError::Validation("invalid_cron"))?,
                body.timezone
                    .as_deref()
                    .ok_or(ApiError::Validation("invalid_timezone"))?,
            )?
            .next_after(Utc::now())?,
        ),
        "webhook" if body.cron.is_none() && body.timezone.is_none() => None,
        _ => return Err(ApiError::Validation("invalid_trigger_kind")),
    };
    let version = workflows::get_version(&mut access.tx, body.workflow_id, body.version)
        .await?
        .ok_or(ApiError::NotFound)?;
    let input = Value::Object(body.input.clone());
    if input.to_string().len() > crate::limits::run_input_bytes() {
        return Err(ApiError::Validation("run_input_too_large"));
    }
    let diagnostics = crate::runs::input_diagnostics(&version.graph.workflow_input_schema, &input)
        .map_err(ApiError::internal)?;
    if !diagnostics.is_empty() {
        return Err(ApiError::Invalid {
            code: "run_input_invalid",
            diagnostics,
        });
    }
    Ok((version.summary.id, next))
}

fn name(value: &str) -> Result<&str, ApiError> {
    let value = value.trim();
    if !(1..=100).contains(&value.chars().count()) {
        return Err(ApiError::Validation("invalid_name"));
    }
    Ok(value)
}

pub(crate) fn idempotency_key(
    headers: &HeaderMap,
    required: bool,
) -> Result<Option<String>, ApiError> {
    match headers.get("idempotency-key") {
        None if required => Err(ApiError::BadRequest("idempotency_key_required")),
        None => Ok(None),
        Some(value) => value
            .to_str()
            .ok()
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 200
                    && value.bytes().all(|byte| (33..=126).contains(&byte))
            })
            .map(|value| Some(value.to_owned()))
            .ok_or(ApiError::BadRequest("invalid_idempotency_key")),
    }
}

/// Signature = lowercase hex HMAC-SHA256(secret, timestamp + "." + raw body).
/// The timestamp is Unix seconds; signed bodies are JSON workflow-input objects.
pub async fn webhook(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<runs::RunRecord>), ApiError> {
    if body.len() > crate::limits::webhook_bytes() {
        return Err(ApiError::PayloadTooLarge);
    }
    let trigger = automation::webhook(state.store.pool(), id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let encrypted = EncryptedSecret {
        ciphertext: trigger
            .webhook_ciphertext
            .clone()
            .ok_or_else(|| ApiError::internal("missing webhook ciphertext"))?,
        nonce: trigger
            .webhook_nonce
            .clone()
            .ok_or_else(|| ApiError::internal("missing webhook nonce"))?,
        key_version: trigger
            .webhook_key_version
            .ok_or_else(|| ApiError::internal("missing webhook key version"))?,
    };
    let secret = state
        .config
        .master_key
        .decrypt(trigger.organization_id, id, &encrypted)
        .map_err(ApiError::internal)?;
    verify_signature(&headers, &body, secret.as_bytes(), Utc::now())?;
    check_rate(
        &state,
        &format!("hook:{id}"),
        crate::limits::bounded("CORQTY_WEBHOOK_RATE_PER_MINUTE", 120, 10000),
        60,
    )
    .await?;
    let key = idempotency_key(&headers, true)?.expect("required key");
    let input: Map<String, Value> =
        serde_json::from_slice(&body).map_err(|_| ApiError::BadRequest("invalid_json"))?;
    let org = trigger.organization_id;
    let mut tx = state.store.pool().begin().await?;
    automation::actor_context(&mut tx, org, None, Some(id)).await?;
    let mut access = OrgAccess {
        tx,
        organization_id: org,
        user_id: Uuid::nil(),
        role: Role::Runner,
    };
    // Serialize with edits/disabling and use the configuration current at dispatch time.
    let trigger: TriggerRecord = sqlx::query_as("SELECT * FROM corqty.lock_automation_trigger($1)")
        .bind(id)
        .fetch_optional(&mut *access.tx)
        .await?
        .ok_or(ApiError::NotFound)?;
    let route = format!("/hooks/v1/{id}");
    let actor = format!("trigger:{id}");
    let hash = Sha256::digest(&body);
    match automation::check_idempotency(&mut access.tx, org, &route, &actor, &key, &hash).await? {
        automation::Idempotency::Fresh => {}
        automation::Idempotency::Replay(run) => {
            let run = runs::get_run(&mut access.tx, run)
                .await?
                .ok_or(ApiError::NotFound)?;
            access.commit().await?;
            return Ok((StatusCode::OK, Json(run)));
        }
        automation::Idempotency::Conflict => {
            return Err(ApiError::Conflict("idempotency_conflict"));
        }
    }
    let origin = RunOrigin {
        user_id: None,
        service_token_id: None,
        trigger_id: Some(id),
        source: "webhook",
    };
    let run = crate::runs::create_with_origin(
        &state,
        &mut access,
        trigger.workflow_id,
        trigger.version,
        Value::Object(input),
        None,
        &origin,
    )
    .await?;
    automation::record_idempotency(
        &mut access.tx,
        org,
        &route,
        &actor,
        &key,
        &hash,
        run.summary.id,
    )
    .await?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(run)))
}

fn verify_signature(
    headers: &HeaderMap,
    body: &[u8],
    secret: &[u8],
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    let timestamp = headers
        .get("x-corqty-timestamp")
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;
    let seconds = timestamp
        .parse::<i64>()
        .map_err(|_| ApiError::Unauthorized)?;
    if now.timestamp().abs_diff(seconds) > 300 {
        return Err(ApiError::Unauthorized);
    }
    let signature = headers
        .get("x-corqty-signature")
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;
    let signature = signature.strip_prefix("sha256=").unwrap_or(signature);
    if signature.len() != 64 || !signature.is_ascii() {
        return Err(ApiError::Unauthorized);
    }
    let signature = (0..64)
        .step_by(2)
        .map(|index| u8::from_str_radix(&signature[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ApiError::Unauthorized)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).map_err(ApiError::internal)?;
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    mac.verify_slice(&signature)
        .map_err(|_| ApiError::Unauthorized)
}

pub async fn check_rate(
    state: &AppState,
    key: &str,
    limit: usize,
    seconds: i32,
) -> Result<(), ApiError> {
    let allowed: bool = sqlx::query_scalar("SELECT corqty.rate_limit($1,$2,$3)")
        .bind(Sha256::digest(key.as_bytes()).to_vec())
        .bind(i32::try_from(limit).unwrap_or(10000))
        .bind(seconds)
        .fetch_one(state.store.pool())
        .await?;
    if allowed {
        Ok(())
    } else {
        Err(ApiError::TooManyRequests("rate_limited"))
    }
}

/// Uses the actual socket peer, never caller-supplied forwarding headers. Proxy
/// deployments should also apply edge rate limits; database buckets span API replicas.
pub async fn rate_limit_requests(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let path = request.uri().path();
    let bucket = if request.method() == Method::POST
        && (path.ends_with("/auth/login") || path.ends_with("/auth/register"))
    {
        Some((
            "auth",
            crate::limits::bounded("CORQTY_AUTH_RATE_PER_MINUTE", 30, 1000),
        ))
    } else if path.ends_with("/invitations/accept") {
        Some(("invitation", 30))
    } else if path.starts_with("/hooks/v1/") || request.headers().contains_key("authorization") {
        Some(("external", 300))
    } else {
        None
    };
    if let Some((bucket, limit)) = bucket {
        let peer = request
            .extensions()
            .get::<ConnectInfo<std::net::SocketAddr>>()
            .map(|peer| peer.0.ip().to_string())
            .unwrap_or_else(|| "in-process".to_owned());
        check_rate(&state, &format!("{bucket}:{peer}"), limit, 60).await?;
    }
    Ok(next.run(request).await)
}

/// One leader handles at most 100 triggers per pass. Misses older than five minutes
/// are summarized in an audit event, then only the five-minute grace window is replayed.
pub async fn dispatch_due(state: &AppState, now: DateTime<Utc>) -> Result<usize, ApiError> {
    let mut tx = state.store.pool().begin().await?;
    let triggers = automation::due_triggers(&mut tx, now).await?;
    let count = triggers.len();
    let mut access = OrgAccess {
        tx,
        organization_id: Uuid::nil(),
        user_id: Uuid::nil(),
        role: Role::Runner,
    };
    for trigger in triggers {
        let org = trigger.organization_id;
        access.organization_id = org;
        automation::actor_context(&mut access.tx, org, None, Some(trigger.id)).await?;
        let schedule = CronSchedule::parse(
            trigger.cron.as_deref().unwrap_or_default(),
            trigger.timezone.as_deref().unwrap_or_default(),
        )?;
        let mut scheduled = trigger
            .next_scheduled_at
            .ok_or_else(|| ApiError::internal("cron without schedule"))?;
        let grace = now - chrono::Duration::minutes(5);
        if scheduled < grace {
            occurrence(&mut access, &trigger, scheduled, None, "misfire_skipped").await?;
            automation::audit_actor(
                &mut access.tx,
                org,
                None,
                "trigger.misfire_skipped",
                "trigger",
                trigger.id,
                json!({"from":scheduled,"before":grace}),
            )
            .await?;
            scheduled = schedule.next_after(grace - chrono::Duration::seconds(1))?;
        }
        while scheduled <= now {
            let overlap: bool = sqlx::query_scalar("SELECT EXISTS(SELECT FROM runs WHERE trigger_id=$1 AND status IN ('queued','running'))")
                .bind(trigger.id).fetch_one(&mut *access.tx).await?;
            if trigger.overlap_policy == "skip" && overlap {
                occurrence(&mut access, &trigger, scheduled, None, "overlap_skipped").await?;
                automation::audit_actor(
                    &mut access.tx,
                    org,
                    None,
                    "trigger.overlap_skipped",
                    "trigger",
                    trigger.id,
                    json!({"scheduledFor":scheduled}),
                )
                .await?;
            } else {
                sqlx::query("SAVEPOINT cron_run")
                    .execute(&mut *access.tx)
                    .await?;
                let origin = RunOrigin {
                    user_id: None,
                    service_token_id: None,
                    trigger_id: Some(trigger.id),
                    source: "cron",
                };
                match crate::runs::create_with_origin(
                    state,
                    &mut access,
                    trigger.workflow_id,
                    trigger.version,
                    trigger.input.clone(),
                    None,
                    &origin,
                )
                .await
                {
                    Ok(run) => {
                        occurrence(
                            &mut access,
                            &trigger,
                            scheduled,
                            Some(run.summary.id),
                            "started",
                        )
                        .await?;
                    }
                    Err(error) => {
                        sqlx::query("ROLLBACK TO SAVEPOINT cron_run")
                            .execute(&mut *access.tx)
                            .await?;
                        occurrence(&mut access, &trigger, scheduled, None, "rejected").await?;
                        // No configuration, input, or source error reaches the audit record.
                        automation::audit_actor(
                            &mut access.tx,
                            org,
                            None,
                            "trigger.dispatch_rejected",
                            "trigger",
                            trigger.id,
                            json!({"scheduledFor":scheduled}),
                        )
                        .await?;
                        tracing::warn!(trigger_id=%trigger.id, error=%error, "cron dispatch rejected");
                    }
                }
                sqlx::query("RELEASE SAVEPOINT cron_run")
                    .execute(&mut *access.tx)
                    .await?;
            }
            scheduled = schedule.next_after(scheduled)?;
        }
        sqlx::query("SELECT corqty.advance_trigger($1,$2)")
            .bind(trigger.id)
            .bind(scheduled)
            .execute(&mut *access.tx)
            .await?;
    }
    access.commit().await?;
    Ok(count)
}

async fn occurrence(
    access: &mut OrgAccess,
    trigger: &TriggerRecord,
    time: DateTime<Utc>,
    run: Option<Uuid>,
    outcome: &str,
) -> Result<(), ApiError> {
    sqlx::query("INSERT INTO trigger_occurrences(trigger_id,organization_id,scheduled_for,run_id,outcome) VALUES ($1,$2,$3,$4,$5)")
        .bind(trigger.id).bind(trigger.organization_id).bind(time).bind(run).bind(outcome).execute(&mut *access.tx).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signatures_bind_raw_bytes_and_timestamp_with_a_five_minute_window() {
        let now = Utc::now();
        let timestamp = now.timestamp().to_string();
        let secret = b"a long webhook secret";
        let body = br#"{"a":1}"#;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(timestamp.as_bytes());
        mac.update(b".");
        mac.update(body);
        let signature = mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let mut headers = HeaderMap::new();
        headers.insert("x-corqty-timestamp", timestamp.parse().unwrap());
        headers.insert("x-corqty-signature", signature.parse().unwrap());
        assert!(verify_signature(&headers, body, secret, now).is_ok());
        assert!(verify_signature(&headers, br#"{"a":2}"#, secret, now).is_err());
        assert!(
            verify_signature(&headers, body, secret, now + chrono::Duration::seconds(301)).is_err()
        );
        assert!(verify_signature(&headers, body, b"wrong", now).is_err());
    }
}
