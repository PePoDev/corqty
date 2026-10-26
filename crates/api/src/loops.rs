//! Background loops of the API process. Every replica runs them, and each pass elects its
//! leader in the database, so replicas never repeat each other's work.

use std::time::Duration;

use corqty_persistence::worker_queue::recover_expired_leases;
use tokio::task::JoinSet;

use crate::state::AppState;

/// How long the recovery loop waits between passes.
const RECOVERY_INTERVAL: Duration = Duration::from_secs(5);

/// Starts the background loops. Each ends once `state.shutdown` is cancelled.
#[must_use = "dropping the JoinSet aborts the loops"]
pub fn spawn(state: &AppState) -> JoinSet<()> {
    let mut loops = JoinSet::new();
    let pool = state.store.pool().clone();
    let shutdown = state.shutdown.clone();
    loops.spawn(async move {
        loop {
            match recover_expired_leases(&pool).await {
                Ok(0) => {}
                Ok(recovered) => tracing::info!(recovered, "recovered expired worker leases"),
                // A deadlock with a heartbeat, or a lost connection: the next pass tries again.
                Err(error) => tracing::warn!(error = %error, "lease recovery failed"),
            }
            tokio::select! {
                () = shutdown.cancelled() => break,
                () = tokio::time::sleep(RECOVERY_INTERVAL) => {}
            }
        }
    });
    loops
}
