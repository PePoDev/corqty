#![forbid(unsafe_code)]

//! The executors a worker can run. Milestone 4 adds the Wasmtime host for runtime plugins here.

use std::sync::Arc;

use corqty_plugin_sdk::{Capability, Executor, ExecutorMetadata, HOST_CONTRACT};
use thiserror::Error;

/// Why a worker refuses to start with a set of executors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("executor key {0:?} must match ^[a-z][a-z0-9-]{{0,62}}$")]
    InvalidKey(String),
    #[error("executor {0} must have a MAJOR.MINOR.PATCH version")]
    InvalidVersion(String),
    #[error("executor {executor} targets host contract {host_contract:?}, not {HOST_CONTRACT:?}")]
    UnsupportedHostContract {
        executor: String,
        host_contract: String,
    },
    #[error("executor {executor} declares {capability:?}, which this host grants from Milestone 4")]
    UnsupportedCapability {
        executor: String,
        capability: Capability,
    },
    #[error("executor {0} is registered twice")]
    DuplicateExecutor(String),
}

/// The executors compiled into this worker, checked once at startup.
pub struct Registry(Vec<Arc<dyn Executor>>);

impl Registry {
    /// Accepts executors with a lowercase slug key, a `MAJOR.MINOR.PATCH` version, this host's
    /// contract, only the capabilities Milestone 3 grants, and a unique `key@version`.
    pub fn new(executors: Vec<Arc<dyn Executor>>) -> Result<Self, RegistryError> {
        let mut identities = Vec::new();
        for executor in &executors {
            let metadata = executor.metadata();
            let identity = identity(&metadata);
            if !is_valid_key(metadata.key) {
                return Err(RegistryError::InvalidKey(metadata.key.to_owned()));
            }
            if !is_valid_version(metadata.version) {
                return Err(RegistryError::InvalidVersion(identity));
            }
            if metadata.host_contract != HOST_CONTRACT {
                return Err(RegistryError::UnsupportedHostContract {
                    executor: identity,
                    host_contract: metadata.host_contract.to_owned(),
                });
            }
            // ponytail: Milestone 3 grants log, scratch-fs and command-exec to every compiled
            // executor; secret-read, network and per-plugin approval arrive in Milestone 4.
            if let Some(capability) = metadata.capabilities.into_iter().find(|capability| {
                matches!(
                    capability,
                    Capability::SecretRead | Capability::Network { .. }
                )
            }) {
                return Err(RegistryError::UnsupportedCapability {
                    executor: identity,
                    capability,
                });
            }
            if identities.contains(&identity) {
                return Err(RegistryError::DuplicateExecutor(identity));
            }
            identities.push(identity);
        }
        Ok(Self(executors))
    }

    /// The executor whose key and version are exactly these.
    #[must_use]
    pub fn find(&self, key: &str, version: &str) -> Option<Arc<dyn Executor>> {
        self.0
            .iter()
            .find(|executor| {
                let metadata = executor.metadata();
                metadata.key == key && metadata.version == version
            })
            .cloned()
    }

    /// Every `key@version` this worker runs, sorted: what it advertises when it claims jobs.
    #[must_use]
    pub fn advertised(&self) -> Vec<String> {
        let mut identities: Vec<String> = self
            .0
            .iter()
            .map(|executor| identity(&executor.metadata()))
            .collect();
        identities.sort();
        identities
    }
}

fn identity(metadata: &ExecutorMetadata) -> String {
    format!("{}@{}", metadata.key, metadata.version)
}

fn is_valid_key(key: &str) -> bool {
    key.len() <= 63
        && key.starts_with(|character: char| character.is_ascii_lowercase())
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn is_valid_version(version: &str) -> bool {
    version.split('.').count() == 3
        && version
            .split('.')
            .all(|number| !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
}
