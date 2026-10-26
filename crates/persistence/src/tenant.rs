use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub async fn begin_tenant(
    pool: &PgPool,
    user_id: Uuid,
    organization_id: Uuid,
) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "SELECT
            set_config('corqty.user_id', $1, true),
            set_config('corqty.organization_id', $2, true)",
    )
    .bind(user_id.to_string())
    .bind(organization_id.to_string())
    .execute(&mut *tx)
    .await?;
    Ok(tx)
}

pub async fn begin_user(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('corqty.user_id', $1, true)")
        .bind(user_id.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}
