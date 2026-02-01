//! # Sync Error Types
//!
//! Error types for sync operations.
//!
//! ## Error Hierarchy
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                       Sync Error Categories                             │
//! │                                                                         │
//! │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────┐ │
//! │  │  Configuration  │  │   Transport     │  │     Protocol            │ │
//! │  │                 │  │                 │  │                         │ │
//! │  │  InvalidConfig  │  │  Connection     │  │  InvalidMessage         │ │
//! │  │  MissingDeviceId│  │  Disconnected   │  │  UnsupportedVersion     │ │
//! │  │  InvalidUrl     │  │  Timeout        │  │  DeserializationFailed  │ │
//! │  └─────────────────┘  └─────────────────┘  └─────────────────────────┘ │
//! │                                                                         │
//! │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────┐ │
//! │  │    Database     │  │     Outbox      │  │      Inbound            │ │
//! │  │                 │  │                 │  │                         │ │
//! │  │  QueryFailed    │  │  BatchFailed    │  │  ApplyFailed            │ │
//! │  │  MigrationError │  │  EmptyPayload   │  │  ConflictDetected       │ │
//! │  └─────────────────┘  └─────────────────┘  └─────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use thiserror::Error;

/// Result type alias for sync operations.
pub type SyncResult<T> = Result<T, SyncError>;

/// Sync error type covering all possible sync failures.
///
/// ## Design Principles
/// - Each variant includes enough context for debugging
/// - Errors are categorized for different handling strategies
/// - All errors are `Send + Sync` for async compatibility
#[derive(Debug, Error)]
pub enum SyncError {
    // =========================================================================
    // Configuration Errors
    // =========================================================================
    /// Invalid sync configuration.
    #[error("Invalid sync configuration: {0}")]
    InvalidConfig(String),

    /// Missing device ID (required for sync).
    #[error("Device ID not configured. Run initial setup first.")]
    MissingDeviceId,

    /// Invalid hub URL.
    #[error("Invalid hub URL: {0}")]
    InvalidUrl(String),

    /// Failed to load config file.
    #[error("Failed to load config: {0}")]
    ConfigLoadFailed(String),

    /// Failed to save config file.
    #[error("Failed to save config: {0}")]
    ConfigSaveFailed(String),

    // =========================================================================
    // Transport Errors
    // =========================================================================
    /// Failed to establish WebSocket connection.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// WebSocket disconnected unexpectedly.
    #[error("Disconnected from sync hub")]
    Disconnected,

    /// Connection timeout.
    #[error("Connection timeout after {0} seconds")]
    Timeout(u64),

    /// TLS/SSL error.
    #[error("TLS error: {0}")]
    TlsError(String),

    /// WebSocket protocol error.
    #[error("WebSocket error: {0}")]
    WebSocketError(String),

    // =========================================================================
    // Protocol Errors
    // =========================================================================
    /// Invalid message received.
    #[error("Invalid message: {0}")]
    InvalidMessage(String),

    /// Unsupported protocol version.
    #[error("Unsupported protocol version: {0}")]
    UnsupportedVersion(u32),

    /// Failed to serialize message.
    #[error("Serialization failed: {0}")]
    SerializationFailed(String),

    /// Failed to deserialize message.
    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),

    /// Unexpected message type.
    #[error("Unexpected message type: expected {expected}, got {actual}")]
    UnexpectedMessageType { expected: String, actual: String },

    // =========================================================================
    // Database Errors
    // =========================================================================
    /// Database query failed.
    #[error("Database error: {0}")]
    DatabaseError(String),

    /// Database migration failed.
    #[error("Migration failed: {0}")]
    MigrationFailed(String),

    // =========================================================================
    // Outbox Errors
    // =========================================================================
    /// Failed to process outbox batch.
    #[error("Outbox batch failed: {0}")]
    OutboxBatchFailed(String),

    /// Outbox entry has empty payload.
    #[error("Outbox entry {id} has empty payload")]
    EmptyPayload { id: String },

    /// Maximum retry attempts exceeded.
    #[error("Max retries exceeded for outbox entry {id}: {last_error}")]
    MaxRetriesExceeded { id: String, last_error: String },

    // =========================================================================
    // Inbound Errors
    // =========================================================================
    /// Failed to apply inbound update.
    #[error("Failed to apply update: {0}")]
    ApplyFailed(String),

    /// Conflict detected during update.
    #[error("Conflict detected for {entity_type}/{entity_id}: local version {local_version}, remote version {remote_version}")]
    ConflictDetected {
        entity_type: String,
        entity_id: String,
        local_version: i64,
        remote_version: i64,
    },

    // =========================================================================
    // Internal Errors
    // =========================================================================
    /// Internal sync agent error.
    #[error("Internal error: {0}")]
    Internal(String),

    /// Agent is shutting down.
    #[error("Sync agent is shutting down")]
    ShuttingDown,

    /// Channel send/receive failed.
    #[error("Channel error: {0}")]
    ChannelError(String),
}

// =============================================================================
// Error Conversions
// =============================================================================

impl From<titan_db::DbError> for SyncError {
    fn from(err: titan_db::DbError) -> Self {
        SyncError::DatabaseError(err.to_string())
    }
}

impl From<serde_json::Error> for SyncError {
    fn from(err: serde_json::Error) -> Self {
        SyncError::SerializationFailed(err.to_string())
    }
}

impl From<url::ParseError> for SyncError {
    fn from(err: url::ParseError) -> Self {
        SyncError::InvalidUrl(err.to_string())
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for SyncError {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        use tokio_tungstenite::tungstenite::Error as WsError;
        match err {
            WsError::ConnectionClosed => SyncError::Disconnected,
            WsError::AlreadyClosed => SyncError::Disconnected,
            WsError::Protocol(p) => SyncError::WebSocketError(p.to_string()),
            WsError::Io(io) => SyncError::ConnectionFailed(io.to_string()),
            WsError::Tls(tls) => SyncError::TlsError(tls.to_string()),
            other => SyncError::WebSocketError(other.to_string()),
        }
    }
}

impl From<std::io::Error> for SyncError {
    fn from(err: std::io::Error) -> Self {
        SyncError::ConfigLoadFailed(err.to_string())
    }
}

impl From<toml::de::Error> for SyncError {
    fn from(err: toml::de::Error) -> Self {
        SyncError::ConfigLoadFailed(err.to_string())
    }
}

impl From<toml::ser::Error> for SyncError {
    fn from(err: toml::ser::Error) -> Self {
        SyncError::ConfigSaveFailed(err.to_string())
    }
}

impl From<sqlx::Error> for SyncError {
    fn from(err: sqlx::Error) -> Self {
        SyncError::DatabaseError(err.to_string())
    }
}

// =============================================================================
// Error Categorization (for retry logic)
// =============================================================================

impl SyncError {
    /// Returns true if this error is recoverable and the operation can be retried.
    ///
    /// ## Retryable Errors
    /// - Connection failures (network issues)
    /// - Timeouts
    /// - Temporary disconnections
    ///
    /// ## Non-Retryable Errors
    /// - Configuration errors
    /// - Protocol/version mismatches
    /// - Permanent conflicts
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            SyncError::ConnectionFailed(_)
                | SyncError::Disconnected
                | SyncError::Timeout(_)
                | SyncError::WebSocketError(_)
                | SyncError::OutboxBatchFailed(_)
        )
    }

    /// Returns true if this error indicates a configuration problem.
    pub fn is_config_error(&self) -> bool {
        matches!(
            self,
            SyncError::InvalidConfig(_)
                | SyncError::MissingDeviceId
                | SyncError::InvalidUrl(_)
                | SyncError::ConfigLoadFailed(_)
                | SyncError::ConfigSaveFailed(_)
        )
    }

    /// Returns true if this error indicates a protocol mismatch.
    pub fn is_protocol_error(&self) -> bool {
        matches!(
            self,
            SyncError::InvalidMessage(_)
                | SyncError::UnsupportedVersion(_)
                | SyncError::SerializationFailed(_)
                | SyncError::DeserializationFailed(_)
                | SyncError::UnexpectedMessageType { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retryable_errors() {
        assert!(SyncError::ConnectionFailed("network error".into()).is_retryable());
        assert!(SyncError::Disconnected.is_retryable());
        assert!(SyncError::Timeout(30).is_retryable());

        assert!(!SyncError::InvalidConfig("bad config".into()).is_retryable());
        assert!(!SyncError::MissingDeviceId.is_retryable());
        assert!(!SyncError::UnsupportedVersion(99).is_retryable());
    }

    #[test]
    fn test_error_display() {
        let err = SyncError::ConflictDetected {
            entity_type: "Product".into(),
            entity_id: "abc-123".into(),
            local_version: 5,
            remote_version: 7,
        };
        assert!(err.to_string().contains("Product"));
        assert!(err.to_string().contains("abc-123"));
    }
}
