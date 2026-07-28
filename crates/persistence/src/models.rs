use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct SessionRecord {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub csrf_hash: Vec<u8>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryRecord {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationMembershipRecord {
    pub organization_id: Uuid,
    pub name: String,
    pub slug: String,
    pub role: String,
}

#[derive(Debug)]
pub struct RegistrationRecord {
    pub user_id: Uuid,
    pub organization_id: Uuid,
    pub role: String,
}
