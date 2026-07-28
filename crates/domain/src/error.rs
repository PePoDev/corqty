use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("email is invalid")]
    InvalidEmail,
    #[error("organization name must contain 1 to 100 characters")]
    InvalidOrganizationName,
    #[error("password must contain 12 to 128 characters")]
    InvalidPassword,
    #[error("role is invalid")]
    InvalidRole,
}
