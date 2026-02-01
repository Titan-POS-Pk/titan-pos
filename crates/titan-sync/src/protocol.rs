//! # Sync Protocol Messages
//!
//! Message types for sync communication between devices.
//!
//! ## Protocol Overview
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                      Sync Protocol Messages                             │
//! │                                                                         │
//! │  HANDSHAKE FLOW                                                        │
//! │  ──────────────                                                        │
//! │  SECONDARY ───► Hello { device_id, protocol_version }                  │
//! │  PRIMARY   ◄─── Welcome { store_id, sync_cursor }                      │
//! │                                                                         │
//! │  OUTBOX UPLOAD (SECONDARY → PRIMARY)                                   │
//! │  ───────────────────────────────────                                   │
//! │  SECONDARY ───► OutboxBatch { entries: [...] }                         │
//! │  PRIMARY   ◄─── BatchAck { acked_ids: [...], failed_ids: [...] }       │
//! │                                                                         │
//! │  INBOUND UPDATES (PRIMARY → SECONDARY)                                 │
//! │  ─────────────────────────────────────                                 │
//! │  PRIMARY   ───► EntityUpdate { entity_type, entity_id, data, version } │
//! │  SECONDARY ◄─── UpdateAck { entity_id }                                │
//! │                                                                         │
//! │  KEEPALIVE                                                             │
//! │  ─────────                                                             │
//! │  Both      ◄──► Ping { timestamp }                                     │
//! │  Both      ◄──► Pong { timestamp }                                     │
//! │                                                                         │
//! │  ERROR                                                                 │
//! │  ─────                                                                 │
//! │  Both      ◄──► Error { code, message }                                │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Wire Format (v0.2: JSON)
//! Messages are serialized as JSON with a `type` discriminator:
//! ```json
//! {
//!   "type": "hello",
//!   "payload": {
//!     "device_id": "...",
//!     "protocol_version": 1
//!   }
//! }
//! ```
//!
//! Future versions may use Protobuf or MessagePack for efficiency.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{SyncError, SyncResult};

/// Current protocol version.
pub const PROTOCOL_VERSION: u32 = 1;

// =============================================================================
// Message Envelope
// =============================================================================

/// Sync message envelope with type discriminator.
///
/// All messages are wrapped in this envelope for serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMessage {
    /// Message type discriminator.
    #[serde(rename = "type")]
    pub kind: SyncMessageKind,

    /// Message payload (type-specific).
    pub payload: serde_json::Value,

    /// Optional message ID for request-response correlation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,

    /// Timestamp when message was created.
    #[serde(default = "Utc::now")]
    pub timestamp: DateTime<Utc>,
}

impl SyncMessage {
    /// Creates a new message with a random ID.
    pub fn new<T: Serialize>(kind: SyncMessageKind, payload: T) -> SyncResult<Self> {
        Ok(SyncMessage {
            kind,
            payload: serde_json::to_value(payload)?,
            message_id: Some(uuid::Uuid::new_v4().to_string()),
            timestamp: Utc::now(),
        })
    }

    /// Creates a new message without an ID.
    pub fn new_without_id<T: Serialize>(kind: SyncMessageKind, payload: T) -> SyncResult<Self> {
        Ok(SyncMessage {
            kind,
            payload: serde_json::to_value(payload)?,
            message_id: None,
            timestamp: Utc::now(),
        })
    }

    /// Serializes to JSON string.
    pub fn to_json(&self) -> SyncResult<String> {
        Ok(serde_json::to_string(self)?)
    }

    /// Deserializes from JSON string.
    pub fn from_json(json: &str) -> SyncResult<Self> {
        serde_json::from_str(json).map_err(|e| SyncError::DeserializationFailed(e.to_string()))
    }

    /// Extracts the typed payload.
    pub fn extract_payload<T: for<'de> Deserialize<'de>>(&self) -> SyncResult<T> {
        serde_json::from_value(self.payload.clone())
            .map_err(|e| SyncError::DeserializationFailed(e.to_string()))
    }
}

// =============================================================================
// Message Types
// =============================================================================

/// Discriminator for message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMessageKind {
    // Handshake
    Hello,
    Welcome,

    // Outbox upload
    OutboxBatch,
    BatchAck,

    // Inbound updates
    EntityUpdate,
    UpdateAck,

    // Keepalive
    Ping,
    Pong,

    // Error
    Error,

    // Cursor sync
    CursorRequest,
    CursorResponse,
}

// =============================================================================
// Handshake Messages
// =============================================================================

/// Hello message sent by SECONDARY on connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloPayload {
    /// Device identifier.
    pub device_id: String,

    /// Device name (human-readable).
    pub device_name: String,

    /// Store ID this device belongs to.
    pub store_id: String,

    /// Protocol version supported by this device.
    pub protocol_version: u32,

    /// Capabilities this device supports.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl HelloPayload {
    pub fn new(device_id: &str, device_name: &str, store_id: &str) -> Self {
        HelloPayload {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            store_id: store_id.to_string(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: vec!["outbox_sync".into(), "entity_updates".into()],
        }
    }
}

/// Welcome message sent by PRIMARY after successful handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WelcomePayload {
    /// Store ID confirmed by PRIMARY.
    pub store_id: String,

    /// Current sync cursor for this device (where to resume).
    pub sync_cursor: i64,

    /// Server (PRIMARY) device ID.
    pub server_device_id: String,

    /// Server device name.
    pub server_device_name: String,

    /// Protocol version negotiated.
    pub protocol_version: u32,
}

// =============================================================================
// Outbox Messages
// =============================================================================

/// A single outbox entry in a batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxEntry {
    /// Outbox entry ID.
    pub id: String,

    /// Entity type: "SALE", "PRODUCT", "PAYMENT", etc.
    pub entity_type: String,

    /// Entity ID.
    pub entity_id: String,

    /// Full entity payload as JSON.
    pub payload: String,

    /// When this entry was created.
    pub created_at: DateTime<Utc>,
}

/// Batch of outbox entries for upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxBatchPayload {
    /// Device sending the batch.
    pub device_id: String,

    /// Batch entries.
    pub entries: Vec<OutboxEntry>,

    /// Batch sequence number (for ordering/deduplication).
    pub batch_seq: u64,
}

/// Acknowledgement for a batch upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchAckPayload {
    /// IDs that were successfully processed.
    pub acked_ids: Vec<String>,

    /// IDs that failed with their error messages.
    pub failed_ids: Vec<FailedEntry>,

    /// Updated sync cursor.
    pub new_cursor: i64,
}

/// A failed entry in a batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedEntry {
    /// Entry ID that failed.
    pub id: String,

    /// Error message.
    pub error: String,

    /// Whether this failure is retryable.
    pub retryable: bool,
}

// =============================================================================
// Entity Update Messages
// =============================================================================

/// Entity update pushed from PRIMARY to SECONDARY.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityUpdatePayload {
    /// Type of entity being updated.
    pub entity_type: EntityType,

    /// Entity ID.
    pub entity_id: String,

    /// Update operation.
    pub operation: UpdateOperation,

    /// Entity data (full or partial depending on operation).
    pub data: serde_json::Value,

    /// Version for conflict detection.
    pub version: i64,

    /// When this update was made.
    pub updated_at: DateTime<Utc>,
}

/// Types of entities that can be synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Product,
    TaxRate,
    Category,
    User,
    /// Inventory delta (CRDT-style).
    InventoryDelta,
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntityType::Product => write!(f, "product"),
            EntityType::TaxRate => write!(f, "tax_rate"),
            EntityType::Category => write!(f, "category"),
            EntityType::User => write!(f, "user"),
            EntityType::InventoryDelta => write!(f, "inventory_delta"),
        }
    }
}

/// Type of update operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateOperation {
    /// Full entity upsert.
    Upsert,
    /// Partial field update.
    Patch,
    /// Soft delete.
    Delete,
    /// Inventory delta adjustment.
    InventoryAdjust,
}

/// Acknowledgement for an entity update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAckPayload {
    /// Entity ID that was updated.
    pub entity_id: String,

    /// Whether the update was applied successfully.
    pub success: bool,

    /// Applied version (may differ if conflict was resolved).
    pub applied_version: i64,

    /// Error message if failed.
    pub error: Option<String>,
}

// =============================================================================
// Keepalive Messages
// =============================================================================

/// Ping message for keepalive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingPayload {
    /// Timestamp when ping was sent.
    pub timestamp: DateTime<Utc>,
}

/// Pong response for keepalive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PongPayload {
    /// Original ping timestamp.
    pub ping_timestamp: DateTime<Utc>,

    /// Timestamp when pong was sent.
    pub pong_timestamp: DateTime<Utc>,
}

// =============================================================================
// Error Messages
// =============================================================================

/// Error message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    /// Error code.
    pub code: ErrorCode,

    /// Human-readable error message.
    pub message: String,

    /// Whether the client should retry.
    pub retryable: bool,

    /// Reference to the message that caused the error (if applicable).
    pub reference_message_id: Option<String>,
}

/// Protocol error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// Protocol version mismatch.
    UnsupportedVersion,

    /// Invalid message format.
    InvalidMessage,

    /// Store ID mismatch.
    StoreMismatch,

    /// Authentication failed.
    AuthFailed,

    /// Rate limited.
    RateLimited,

    /// Internal server error.
    InternalError,

    /// Entity not found.
    NotFound,

    /// Conflict detected.
    Conflict,
}

// =============================================================================
// Cursor Messages
// =============================================================================

/// Request current sync cursor position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorRequestPayload {
    /// Device requesting cursor.
    pub device_id: String,

    /// Entity type to get cursor for (optional, all if not specified).
    pub entity_type: Option<EntityType>,
}

/// Response with cursor position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorResponsePayload {
    /// Current cursor position.
    pub cursor: i64,

    /// When cursor was last updated.
    pub last_updated: DateTime<Utc>,

    /// Entity type this cursor is for (optional).
    pub entity_type: Option<EntityType>,
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Creates a Hello message.
pub fn make_hello(device_id: &str, device_name: &str, store_id: &str) -> SyncResult<SyncMessage> {
    let payload = HelloPayload::new(device_id, device_name, store_id);
    SyncMessage::new(SyncMessageKind::Hello, payload)
}

/// Creates a Ping message.
pub fn make_ping() -> SyncResult<SyncMessage> {
    let payload = PingPayload {
        timestamp: Utc::now(),
    };
    SyncMessage::new_without_id(SyncMessageKind::Ping, payload)
}

/// Creates an Error message.
pub fn make_error(
    code: ErrorCode,
    message: &str,
    retryable: bool,
    reference: Option<String>,
) -> SyncResult<SyncMessage> {
    let payload = ErrorPayload {
        code,
        message: message.to_string(),
        retryable,
        reference_message_id: reference,
    };
    SyncMessage::new(SyncMessageKind::Error, payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization() {
        let hello = make_hello("dev-123", "Register 1", "store-001").unwrap();
        let json = hello.to_json().unwrap();
        assert!(json.contains("\"type\":\"hello\""));
        assert!(json.contains("dev-123"));

        let parsed = SyncMessage::from_json(&json).unwrap();
        assert_eq!(parsed.kind, SyncMessageKind::Hello);
    }

    #[test]
    fn test_extract_payload() {
        let hello = make_hello("dev-123", "Register 1", "store-001").unwrap();
        let payload: HelloPayload = hello.extract_payload().unwrap();
        assert_eq!(payload.device_id, "dev-123");
        assert_eq!(payload.protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn test_ping_pong() {
        let ping = make_ping().unwrap();
        assert_eq!(ping.kind, SyncMessageKind::Ping);
        assert!(ping.message_id.is_none()); // Pings don't need IDs
    }

    #[test]
    fn test_error_message() {
        let error = make_error(
            ErrorCode::UnsupportedVersion,
            "Version 99 not supported",
            false,
            None,
        )
        .unwrap();

        let payload: ErrorPayload = error.extract_payload().unwrap();
        assert_eq!(payload.code, ErrorCode::UnsupportedVersion);
        assert!(!payload.retryable);
    }

    #[test]
    fn test_entity_type_display() {
        assert_eq!(EntityType::Product.to_string(), "product");
        assert_eq!(EntityType::InventoryDelta.to_string(), "inventory_delta");
    }
}
