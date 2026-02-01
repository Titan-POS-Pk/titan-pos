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
//! │  INVENTORY SYNC (Milestone 2)                                          │
//! │  ────────────────────────────                                          │
//! │  SECONDARY ───► InventoryDelta { product_id, delta_qty }               │
//! │  PRIMARY   ───► InventoryUpdate { product_id, delta_qty }  (broadcast) │
//! │                                                                         │
//! │  HUB DISCOVERY & ELECTION (Milestone 2)                                │
//! │  ──────────────────────────────────────                                │
//! │  PRIMARY   ───► Heartbeat { device_id, term }                          │
//! │  ANY       ───► ElectionStart { candidate_id, priority }               │
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
//! Messages are serialized as tagged JSON using serde's adjacently tagged enum:
//! ```json
//! { "type": "Hello", "payload": { "device_id": "...", ... } }
//! ```
//!
//! Future versions may use Protobuf or MessagePack for efficiency.

use serde::{Deserialize, Serialize};

/// Current protocol version.
pub const PROTOCOL_VERSION: u32 = 2;

// =============================================================================
// Main Message Enum (Tagged Union)
// =============================================================================

/// All sync protocol messages.
///
/// Uses serde's adjacently tagged enum for clean JSON serialization:
/// `{ "type": "Hello", "payload": { ... } }`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum SyncMessage {
    // =========================================================================
    // Handshake Messages
    // =========================================================================

    /// Initial connection message from SECONDARY to PRIMARY.
    Hello(HelloPayload),

    /// Response from PRIMARY after successful handshake.
    Welcome(WelcomePayload),

    // =========================================================================
    // Outbox Sync Messages
    // =========================================================================

    /// Batch of outbox entries for upload.
    OutboxBatch(OutboxBatch),

    /// Acknowledgement for a batch upload.
    BatchAck(BatchAck),

    // =========================================================================
    // Inventory Sync Messages (Milestone 2)
    // =========================================================================

    /// Inventory delta from SECONDARY (quantity change, not absolute value).
    InventoryDelta(InventoryDelta),

    /// Inventory update broadcast from PRIMARY to all SECONDARY devices.
    InventoryUpdate(InventoryUpdate),

    // =========================================================================
    // Hub Discovery & Election Messages (Milestone 2)
    // =========================================================================

    /// Heartbeat from PRIMARY to announce its presence.
    Heartbeat(HeartbeatPayload),

    /// Election announcement from a candidate.
    ElectionStart(ElectionPayload),

    /// Vote in an election.
    ElectionVote(ElectionVotePayload),

    /// Election result announcement.
    ElectionResult(ElectionResultPayload),

    // =========================================================================
    // Entity Update Messages
    // =========================================================================

    /// Entity update pushed from PRIMARY to SECONDARY.
    EntityUpdate(EntityUpdate),

    /// Acknowledgement for an entity update.
    UpdateAck(UpdateAck),

    // =========================================================================
    // Keepalive Messages
    // =========================================================================

    /// Ping for keepalive.
    Ping { timestamp: String },

    /// Pong response for keepalive.
    Pong {
        ping_timestamp: String,
        pong_timestamp: String,
    },

    // =========================================================================
    // Error Messages
    // =========================================================================

    /// Error message.
    Error { code: String, message: String },

    // =========================================================================
    // Cursor Messages
    // =========================================================================

    /// Request current sync cursor position.
    CursorRequest { device_id: String },

    /// Response with cursor position.
    CursorResponse { cursor: i64, last_updated: String },
}

// =============================================================================
// Handshake Payloads
// =============================================================================

/// Hello message sent by SECONDARY on connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloPayload {
    /// Device identifier.
    pub device_id: String,

    /// Device name (human-readable).
    pub device_name: String,

    /// Store ID this device belongs to.
    pub store_id: String,

    /// Protocol version supported by this device.
    pub protocol_version: u32,

    /// Device priority for election.
    #[serde(default)]
    pub priority: u8,
}

impl HelloPayload {
    pub fn new(device_id: &str, device_name: &str, store_id: &str) -> Self {
        HelloPayload {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            store_id: store_id.to_string(),
            protocol_version: PROTOCOL_VERSION,
            priority: 50,
        }
    }
}

/// Welcome message sent by PRIMARY after successful handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WelcomePayload {
    /// Hub (PRIMARY) device ID.
    pub hub_device_id: String,

    /// Store ID confirmed by PRIMARY.
    pub store_id: String,

    /// Current election term (fencing token).
    pub election_term: u64,

    /// Server time for clock sync reference.
    pub server_time: String,
}

// =============================================================================
// Outbox Payloads
// =============================================================================

/// A single outbox entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxEntry {
    /// Outbox entry ID.
    pub id: String,

    /// Entity type: "SALE", "PRODUCT", "PAYMENT", etc.
    pub entity_type: String,

    /// Entity ID.
    pub entity_id: String,

    /// Full entity payload as JSON string.
    pub payload: String,

    /// When this entry was created.
    pub created_at: String,
}

/// Batch of outbox entries for upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxBatch {
    /// Device sending the batch.
    pub device_id: String,

    /// Batch entries.
    pub entities: Vec<OutboxEntry>,

    /// Batch sequence number (for ordering/deduplication).
    #[serde(default)]
    pub batch_seq: u64,
}

/// Acknowledgement for a batch upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchAck {
    /// IDs that were successfully processed.
    pub acked_ids: Vec<String>,

    /// IDs that failed with their error messages.
    #[serde(default)]
    pub failed_ids: Vec<FailedEntry>,

    /// Updated sync cursor.
    #[serde(default)]
    pub new_cursor: i64,
}

/// A failed entry in a batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedEntry {
    /// Entry ID that failed.
    pub id: String,

    /// Error message.
    pub error: String,

    /// Whether this failure is retryable.
    #[serde(default)]
    pub retryable: bool,
}

// =============================================================================
// Inventory Sync Payloads (Milestone 2)
// =============================================================================

/// Inventory delta from a POS device.
///
/// Uses CRDT-style delta updates instead of absolute values to handle
/// concurrent modifications from multiple devices.
///
/// ## Example
/// ```text
/// POS #1 sells 2 items:  InventoryDelta { delta_quantity: -2 }
/// POS #2 sells 1 item:   InventoryDelta { delta_quantity: -1 }
/// 
/// Hub aggregates: -2 + -1 = -3
/// Broadcasts:     InventoryUpdate { delta_quantity: -3 }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryDelta {
    /// Product ID (UUID).
    pub product_id: String,

    /// SKU snapshot at time of delta.
    pub sku: String,

    /// Quantity change (negative for sales, positive for restocks).
    pub delta_quantity: i32,

    /// When this delta occurred (ISO8601).
    pub timestamp: String,
}

/// Inventory update broadcast from PRIMARY to all SECONDARY devices.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryUpdate {
    /// Product ID (UUID).
    pub product_id: String,

    /// SKU for reference.
    pub sku: String,

    /// Aggregated quantity change.
    pub delta_quantity: i32,

    /// Source device ID (or "hub" if aggregated).
    pub source_device_id: String,

    /// When this update was broadcast (ISO8601).
    pub timestamp: String,
}

// =============================================================================
// Election Payloads (Milestone 2)
// =============================================================================

/// Heartbeat from PRIMARY to announce its presence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatPayload {
    /// Device ID of the PRIMARY.
    pub device_id: String,

    /// Current election term.
    pub election_term: u64,

    /// Hub WebSocket URL.
    pub hub_url: String,

    /// Hub priority (for election comparison).
    pub priority: u8,

    /// Number of connected devices.
    #[serde(default)]
    pub connected_count: usize,
}

/// Election announcement from a candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElectionPayload {
    /// Candidate device ID.
    pub candidate_id: String,

    /// Candidate priority.
    pub priority: u8,

    /// Proposed new term.
    pub proposed_term: u64,
}

/// Vote in an election.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElectionVotePayload {
    /// Voter device ID.
    pub voter_id: String,

    /// Candidate being voted for.
    pub candidate_id: String,

    /// Term being voted in.
    pub term: u64,
}

/// Election result announcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElectionResultPayload {
    /// Winner device ID.
    pub winner_id: String,

    /// Winning term.
    pub term: u64,

    /// Hub URL of the winner.
    pub hub_url: String,
}

// =============================================================================
// Entity Update Payloads
// =============================================================================

/// Entity update pushed from PRIMARY to SECONDARY.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityUpdate {
    /// Type of entity being updated.
    pub entity_type: String,

    /// Entity ID.
    pub entity_id: String,

    /// Update operation: "upsert", "patch", "delete".
    pub operation: String,

    /// Entity data as JSON.
    pub data: serde_json::Value,

    /// Version for conflict detection.
    pub version: i64,

    /// When this update was made (ISO8601).
    pub updated_at: String,
}

/// Acknowledgement for an entity update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAck {
    /// Entity ID that was updated.
    pub entity_id: String,

    /// Whether the update was applied successfully.
    pub success: bool,

    /// Applied version.
    #[serde(default)]
    pub applied_version: i64,

    /// Error message if failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// =============================================================================
// Helper Functions
// =============================================================================

impl SyncMessage {
    /// Returns the message type name as a string (for logging).
    pub fn type_name(&self) -> &'static str {
        match self {
            SyncMessage::Hello(_) => "Hello",
            SyncMessage::Welcome(_) => "Welcome",
            SyncMessage::OutboxBatch(_) => "OutboxBatch",
            SyncMessage::BatchAck(_) => "BatchAck",
            SyncMessage::InventoryDelta(_) => "InventoryDelta",
            SyncMessage::InventoryUpdate(_) => "InventoryUpdate",
            SyncMessage::Heartbeat(_) => "Heartbeat",
            SyncMessage::ElectionStart(_) => "ElectionStart",
            SyncMessage::ElectionVote(_) => "ElectionVote",
            SyncMessage::ElectionResult(_) => "ElectionResult",
            SyncMessage::EntityUpdate(_) => "EntityUpdate",
            SyncMessage::UpdateAck(_) => "UpdateAck",
            SyncMessage::Ping { .. } => "Ping",
            SyncMessage::Pong { .. } => "Pong",
            SyncMessage::Error { .. } => "Error",
            SyncMessage::CursorRequest { .. } => "CursorRequest",
            SyncMessage::CursorResponse { .. } => "CursorResponse",
        }
    }

    /// Creates a Hello message.
    pub fn hello(device_id: &str, device_name: &str, store_id: &str, priority: u8) -> Self {
        SyncMessage::Hello(HelloPayload {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            store_id: store_id.to_string(),
            protocol_version: PROTOCOL_VERSION,
            priority,
        })
    }

    /// Creates a Ping message.
    pub fn ping() -> Self {
        SyncMessage::Ping {
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Creates a Pong message.
    pub fn pong(ping_timestamp: &str) -> Self {
        SyncMessage::Pong {
            ping_timestamp: ping_timestamp.to_string(),
            pong_timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Creates an Error message.
    pub fn error(code: &str, message: &str) -> Self {
        SyncMessage::Error {
            code: code.to_string(),
            message: message.to_string(),
        }
    }

    /// Creates a Heartbeat message.
    pub fn heartbeat(device_id: &str, term: u64, hub_url: &str, priority: u8, connected_count: usize) -> Self {
        SyncMessage::Heartbeat(HeartbeatPayload {
            device_id: device_id.to_string(),
            election_term: term,
            hub_url: hub_url.to_string(),
            priority,
            connected_count,
        })
    }

    /// Creates an InventoryDelta message.
    pub fn inventory_delta(product_id: &str, sku: &str, delta_quantity: i32) -> Self {
        SyncMessage::InventoryDelta(InventoryDelta {
            product_id: product_id.to_string(),
            sku: sku.to_string(),
            delta_quantity,
            timestamp: chrono::Utc::now().to_rfc3339(),
        })
    }

    /// Serializes to JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserializes from JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization() {
        let hello = SyncMessage::hello("dev-123", "Register 1", "store-001", 50);
        let json = hello.to_json().unwrap();
        assert!(json.contains("\"type\":\"Hello\""));
        assert!(json.contains("dev-123"));

        let parsed = SyncMessage::from_json(&json).unwrap();
        if let SyncMessage::Hello(payload) = parsed {
            assert_eq!(payload.device_id, "dev-123");
        } else {
            panic!("Expected Hello message");
        }
    }

    #[test]
    fn test_inventory_delta() {
        let delta = SyncMessage::inventory_delta("prod-123", "SKU-001", -5);
        let json = delta.to_json().unwrap();
        assert!(json.contains("InventoryDelta"));
        assert!(json.contains("-5"));
    }

    #[test]
    fn test_heartbeat() {
        let hb = SyncMessage::heartbeat("hub-001", 42, "ws://192.168.1.100:8765", 100, 3);
        let json = hb.to_json().unwrap();
        assert!(json.contains("Heartbeat"));
        assert!(json.contains("42")); // term
    }

    #[test]
    fn test_error_message() {
        let error = SyncMessage::error("STORE_MISMATCH", "Store ID does not match");
        let json = error.to_json().unwrap();
        assert!(json.contains("STORE_MISMATCH"));
    }
}