use std::sync::Arc;

use corqty_persistence::store::PgStore;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::{auth::crypto::PasswordEngine, config::ApiConfig, events::Wake};

#[derive(Clone)]
pub struct AppState {
    pub store: PgStore,
    pub config: ApiConfig,
    pub dummy_password_hash: Arc<str>,
    /// Cancelled on ctrl-c or SIGTERM; the server and long-lived tasks stop on it.
    pub shutdown: CancellationToken,
    /// Wakes the open run event streams. `events::spawn_listener` feeds it from the database.
    pub wake: broadcast::Sender<Wake>,
}

impl AppState {
    #[must_use]
    pub fn new(store: PgStore, config: ApiConfig) -> Self {
        Self {
            store,
            config,
            dummy_password_hash: PasswordEngine::hash("corqty dummy password value")
                .expect("dummy password hash")
                .into(),
            shutdown: CancellationToken::new(),
            // A stream that falls 1024 wake-ups behind reads again.
            wake: broadcast::channel(1024).0,
        }
    }
}
