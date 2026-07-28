use std::sync::Arc;

use corqty_persistence::store::PgStore;

use crate::{auth::crypto::PasswordEngine, config::ApiConfig};

#[derive(Clone)]
pub struct AppState {
    pub store: PgStore,
    pub config: ApiConfig,
    pub dummy_password_hash: Arc<str>,
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
        }
    }
}
