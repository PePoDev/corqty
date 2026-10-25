use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Clone)]
pub struct PgStore {
    pub(crate) pool: PgPool,
}

impl PgStore {
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn begin_tenant(
        &self,
        user_id: Uuid,
        organization_id: Uuid,
    ) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
        crate::tenant::begin_tenant(&self.pool, user_id, organization_id).await
    }
}
