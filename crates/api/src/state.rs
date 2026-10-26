use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use corqty_persistence::store::PgStore;
use tokio::sync::{Semaphore, broadcast};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    auth::crypto::PasswordEngine,
    config::ApiConfig,
    events::{MAX_STREAMS, Wake},
};

#[derive(Clone)]
pub struct AppState {
    pub store: PgStore,
    pub config: ApiConfig,
    pub dummy_password_hash: Arc<str>,
    /// Cancelled on ctrl-c or SIGTERM; the server and long-lived tasks stop on it.
    pub shutdown: CancellationToken,
    /// Wakes the open run event streams. `events::spawn_listener` feeds it from the database.
    pub wake: broadcast::Sender<Wake>,
    /// One permit for each run event stream that may be open in this process.
    pub streams: Arc<Semaphore>,
    /// How many run event streams each user has open in this process.
    pub streams_per_user: Arc<Mutex<HashMap<Uuid, usize>>>,
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
            streams: Arc::new(Semaphore::new(MAX_STREAMS)),
            streams_per_user: Arc::default(),
        }
    }
}
