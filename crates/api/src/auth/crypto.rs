use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

use crate::error::ApiError;

pub struct PasswordEngine;

impl PasswordEngine {
    pub fn hash(password: &str) -> Result<String, ApiError> {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|_| ApiError::Internal)
    }

    #[must_use]
    pub fn verify(password: &str, encoded_hash: &str) -> bool {
        let Ok(parsed) = PasswordHash::new(encoded_hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    }
}

#[derive(Debug)]
pub struct SecretToken {
    pub plaintext: String,
    pub hash: Vec<u8>,
}

impl SecretToken {
    pub fn generate() -> Result<Self, ApiError> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| ApiError::Internal)?;
        let plaintext = URL_SAFE_NO_PAD.encode(bytes);
        Ok(Self {
            hash: Self::hash(&plaintext),
            plaintext,
        })
    }

    #[must_use]
    pub fn hash(plaintext: &str) -> Vec<u8> {
        Sha256::digest(plaintext.as_bytes()).to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::{PasswordEngine, SecretToken};

    #[test]
    fn password_hash_verifies_only_the_original_password() {
        let hash = PasswordEngine::hash("correct horse battery staple").unwrap();
        assert!(PasswordEngine::verify(
            "correct horse battery staple",
            &hash
        ));
        assert!(!PasswordEngine::verify("wrong password", &hash));
    }

    #[test]
    fn generated_tokens_are_opaque_and_hashable() {
        let first = SecretToken::generate().unwrap();
        let second = SecretToken::generate().unwrap();

        assert_ne!(first.plaintext, second.plaintext);
        assert_eq!(first.hash.len(), 32);
        assert_eq!(second.hash.len(), 32);
    }
}
