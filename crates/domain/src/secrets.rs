//! Encryption and exact-value leak prevention. Keys never enter a database record.

use std::collections::HashSet;

use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use hkdf::Hkdf;
use serde_json::Value;
use sha2::Sha256;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::{diagnostic::Diagnostic, graph::GraphSnapshot};

pub const MAX_SECRET_BYTES: usize = 65_536;
const KEY_VERSION: i32 = 1;

#[derive(Clone)]
pub struct MasterKey(Zeroizing<[u8; 32]>);

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MasterKey([REDACTED])")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncryptedSecret {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub key_version: i32,
}

#[derive(Debug, thiserror::Error)]
pub enum SecretCryptoError {
    #[error("CORQTY_MASTER_KEY is required and must be a base64-encoded 32-byte key")]
    InvalidMasterKey,
    #[error("secure random generation failed")]
    Random,
    #[error("secret encryption or authentication failed")]
    Authentication,
    #[error("unsupported secret key version")]
    KeyVersion,
}

impl MasterKey {
    #[must_use]
    pub fn from_bytes(mut bytes: [u8; 32]) -> Self {
        let key = Self(Zeroizing::new(bytes));
        bytes.zeroize();
        key
    }

    pub fn from_env() -> Result<Self, SecretCryptoError> {
        let text = Zeroizing::new(
            std::env::var("CORQTY_MASTER_KEY").map_err(|_| SecretCryptoError::InvalidMasterKey)?,
        );
        Self::from_base64(&text)
    }

    pub fn from_base64(text: &str) -> Result<Self, SecretCryptoError> {
        let bytes = Zeroizing::new(
            STANDARD
                .decode(text.trim())
                .map_err(|_| SecretCryptoError::InvalidMasterKey)?,
        );
        if bytes.len() != 32 {
            return Err(SecretCryptoError::InvalidMasterKey);
        }
        let mut key = Zeroizing::new([0u8; 32]);
        key.copy_from_slice(bytes.as_slice());
        Ok(Self(key))
    }

    fn cipher(&self, organization_id: Uuid) -> Result<Aes256Gcm, SecretCryptoError> {
        let hkdf = Hkdf::<Sha256>::new(Some(b"corqty.secret.encryption.v1"), self.0.as_ref());
        let mut key = Zeroizing::new([0u8; 32]);
        hkdf.expand(organization_id.as_bytes(), key.as_mut())
            .map_err(|_| SecretCryptoError::Authentication)?;
        Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| SecretCryptoError::Authentication)
    }

    pub fn encrypt(
        &self,
        org: Uuid,
        id: Uuid,
        plaintext: &str,
    ) -> Result<EncryptedSecret, SecretCryptoError> {
        let mut nonce = [0u8; 12];
        getrandom::fill(&mut nonce).map_err(|_| SecretCryptoError::Random)?;
        let aad = associated_data(org, id);
        let ciphertext = self
            .cipher(org)?
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: &aad,
                },
            )
            .map_err(|_| SecretCryptoError::Authentication)?;
        Ok(EncryptedSecret {
            ciphertext,
            nonce: nonce.to_vec(),
            key_version: KEY_VERSION,
        })
    }

    pub fn decrypt(
        &self,
        org: Uuid,
        id: Uuid,
        encrypted: &EncryptedSecret,
    ) -> Result<String, SecretCryptoError> {
        if encrypted.key_version != KEY_VERSION {
            return Err(SecretCryptoError::KeyVersion);
        }
        if encrypted.nonce.len() != 12 {
            return Err(SecretCryptoError::Authentication);
        }
        let aad = associated_data(org, id);
        let plaintext = self
            .cipher(org)?
            .decrypt(
                Nonce::from_slice(&encrypted.nonce),
                Payload {
                    msg: &encrypted.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| SecretCryptoError::Authentication)?;
        String::from_utf8(plaintext).map_err(|error| {
            // Never put decrypted bytes into an error or a log.
            let mut bytes = error.into_bytes();
            bytes.zeroize();
            SecretCryptoError::Authentication
        })
    }
}

fn associated_data(org: Uuid, id: Uuid) -> [u8; 36] {
    let mut aad = [0u8; 36];
    aad[..16].copy_from_slice(org.as_bytes());
    aad[16..32].copy_from_slice(id.as_bytes());
    aad[32..].copy_from_slice(&KEY_VERSION.to_be_bytes());
    aad
}

/// Removes only a missing-secret diagnostic whose precise reference exists in this tenant.
/// Other diagnostics are never suppressed, and an empty tenant set rejects every reference.
pub fn filter_secret_diagnostics(
    graph: &GraphSnapshot,
    known: &HashSet<Uuid>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let graph = serde_json::to_value(graph).expect("graph serializes");
    filter_value_secret_diagnostics(&graph, known, diagnostics);
}

pub fn filter_value_secret_diagnostics(
    value: &Value,
    known: &HashSet<Uuid>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.retain(|diagnostic| {
        diagnostic.code != "secret_not_found"
            || !value
                .pointer(&diagnostic.pointer)
                .and_then(Value::as_str)
                .and_then(|id| id.parse::<Uuid>().ok())
                .is_some_and(|id| known.contains(&id))
    });
}

/// All explicit opaque bindings, including secret-reference configuration widgets.
#[must_use]
pub fn referenced_secret_ids(value: &Value) -> HashSet<Uuid> {
    fn collect(value: &Value, ids: &mut HashSet<Uuid>) {
        match value {
            Value::Object(object) => {
                if object.get("kind").and_then(Value::as_str) == Some("secret")
                    && let Some(id) = object
                        .get("secretId")
                        .and_then(Value::as_str)
                        .and_then(|id| id.parse().ok())
                {
                    ids.insert(id);
                }
                object.values().for_each(|value| collect(value, ids));
            }
            Value::Array(values) => values.iter().for_each(|value| collect(value, ids)),
            _ => {}
        }
    }
    let mut ids = HashSet::new();
    collect(value, &mut ids);
    ids
}

/// Longest matches first so one secret that is a prefix of another cannot expose a suffix.
#[derive(Clone, Default)]
pub struct Redactor {
    values: Vec<String>,
}

impl std::fmt::Debug for Redactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Redactor([REDACTED])")
    }
}

impl Redactor {
    pub fn new(values: impl IntoIterator<Item = String>) -> Self {
        let mut values: Vec<_> = values
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect();
        values.sort_by_key(|value| std::cmp::Reverse(value.len()));
        values.dedup();
        Self { values }
    }

    #[must_use]
    pub fn contains(&self, text: &str) -> bool {
        self.values.iter().any(|value| text.contains(value))
    }

    #[must_use]
    pub fn text(&self, text: &str) -> String {
        let mut result = String::new();
        let mut remaining = text;
        while !remaining.is_empty() {
            let found = self
                .values
                .iter()
                .filter_map(|secret| remaining.find(secret).map(|index| (index, secret.len())))
                .min_by_key(|(index, length)| (*index, std::cmp::Reverse(*length)));
            if let Some((index, length)) = found {
                result.push_str(&remaining[..index]);
                result.push_str("[REDACTED]");
                remaining = &remaining[index + length..];
            } else {
                result.push_str(remaining);
                break;
            }
        }
        result
    }

    #[must_use]
    pub fn contains_json(&self, value: &Value) -> bool {
        match value {
            Value::String(text) => self.contains(text),
            Value::Object(object) => object
                .iter()
                .any(|(key, value)| self.contains(key) || self.contains_json(value)),
            Value::Array(values) => values.iter().any(|value| self.contains_json(value)),
            _ => self.contains(&value.to_string()),
        }
    }

    #[must_use]
    pub fn json(&self, value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.text(text)),
            Value::Object(object) => Value::Object(
                object
                    .iter()
                    .map(|(key, value)| (self.text(key), self.json(value)))
                    .collect(),
            ),
            Value::Array(values) => {
                Value::Array(values.iter().map(|value| self.json(value)).collect())
            }
            _ if self.contains(&value.to_string()) => Value::String("[REDACTED]".to_owned()),
            _ => value.clone(),
        }
    }

    /// Safe prefix of a stream: retain a suffix which might be the start of a secret.
    /// Every flush uses this, so chunk/flush boundaries cannot reveal a split secret.
    pub fn stream_prefix(&self, pending: &mut String, final_flush: bool) -> String {
        let mut cut = pending.len();
        if !final_flush {
            for secret in &self.values {
                let mut from = pending.len().saturating_sub(secret.len().saturating_sub(1));
                while !pending.is_char_boundary(from) {
                    from += 1;
                }
                for (offset, _) in pending[from..].char_indices() {
                    let index = from + offset;
                    let tail = &pending[index..];
                    if tail.len() < secret.len() && secret.starts_with(tail) {
                        cut = cut.min(index);
                        break;
                    }
                }
            }
            // Do not split a full match intersecting the candidate boundary.
            loop {
                let previous = cut;
                for secret in &self.values {
                    for (start, _) in pending.match_indices(secret) {
                        if start < cut && start + secret.len() > cut {
                            cut = start;
                        }
                    }
                }
                if previous == cut {
                    break;
                }
            }
        }
        let tail = pending.split_off(cut);
        let ready = std::mem::replace(pending, tail);
        self.text(&ready)
    }
}

impl Drop for Redactor {
    fn drop(&mut self) {
        self.values.zeroize();
    }
}
