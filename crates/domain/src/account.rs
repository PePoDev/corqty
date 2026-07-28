use serde::{Deserialize, Serialize};

use crate::error::DomainError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NormalizedEmail(String);

impl NormalizedEmail {
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        let normalized = value.trim().to_lowercase();
        let mut parts = normalized.split('@');
        let local = parts.next().unwrap_or_default();
        let domain = parts.next().unwrap_or_default();
        let valid = !local.is_empty()
            && domain.contains('.')
            && !domain.starts_with('.')
            && !domain.ends_with('.')
            && parts.next().is_none()
            && normalized.len() <= 320;
        valid
            .then_some(Self(normalized))
            .ok_or(DomainError::InvalidEmail)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OrganizationName(String);

impl OrganizationName {
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        let trimmed = value.trim();
        (1..=100)
            .contains(&trimmed.chars().count())
            .then(|| Self(trimmed.to_owned()))
            .ok_or(DomainError::InvalidOrganizationName)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn validate_password(value: &str) -> Result<(), DomainError> {
    (12..=128)
        .contains(&value.chars().count())
        .then_some(())
        .ok_or(DomainError::InvalidPassword)
}
