//! Error types for Cloud API.

use tonic::Status;

/// Cloud API errors.
#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Migration error: {0}")]
    Migration(String),

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("Authorization failed: {0}")]
    Unauthorized(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Rate limited: {0}")]
    RateLimited(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Service unavailable: {0}")]
    Unavailable(String),
}

impl From<CloudError> for Status {
    fn from(error: CloudError) -> Self {
        match error {
            CloudError::Database(msg) => Status::internal(msg),
            CloudError::Migration(msg) => Status::internal(msg),
            CloudError::AuthFailed(msg) => Status::unauthenticated(msg),
            CloudError::Unauthorized(msg) => Status::permission_denied(msg),
            CloudError::InvalidRequest(msg) => Status::invalid_argument(msg),
            CloudError::NotFound(msg) => Status::not_found(msg),
            CloudError::Conflict(msg) => Status::already_exists(msg),
            CloudError::RateLimited(msg) => Status::resource_exhausted(msg),
            CloudError::Internal(msg) => Status::internal(msg),
            CloudError::Unavailable(msg) => Status::unavailable(msg),
        }
    }
}
