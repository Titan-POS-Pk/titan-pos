//! # titan-sync: Sync Engine for Titan POS
//!
//! This crate provides the synchronization layer for Titan POS, enabling
//! offline-first operation with background sync to Store Hub and Cloud.
//!
//! ## Architecture Overview
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                        Sync Agent Architecture                          │
//! │                                                                         │
//! │  ┌──────────────────────────────────────────────────────────────────┐  │
//! │  │                      SyncAgent (Main Orchestrator)               │  │
//! │  │                                                                  │  │
//! │  │  Spawned as Tokio task in Tauri setup                           │  │
//! │  │  Coordinates all sync operations                                 │  │
//! │  └────────────────────────────┬─────────────────────────────────────┘  │
//! │                               │                                         │
//! │         ┌─────────────────────┼─────────────────────┐                  │
//! │         ▼                     ▼                     ▼                   │
//! │  ┌────────────────┐  ┌────────────────┐  ┌────────────────────────┐    │
//! │  │ OutboxProcessor│  │   Transport    │  │  InboundHandler        │    │
//! │  │                │  │                │  │                        │    │
//! │  │ Reads pending  │  │ WebSocket with │  │ Applies updates from   │    │
//! │  │ sync_outbox    │  │ auto-reconnect │  │ Store Hub/Cloud        │    │
//! │  │ Batches entries│  │ & backoff      │  │ Products/Prices/Stock  │    │
//! │  │ Sends to hub   │  │                │  │                        │    │
//! │  └────────────────┘  └────────────────┘  └────────────────────────┘    │
//! │                                                                         │
//! │  STATUS EVENTS (to Frontend via Tauri):                                │
//! │  • "sync://status" - Connection state changes                          │
//! │  • "sync://progress" - Upload/download progress                        │
//! │  • "sync://error" - Sync failures                                      │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Module Organization
//!
//! - [`agent`] - Main `SyncAgent` orchestrator
//! - [`config`] - Sync configuration (mode, device ID, hub URL)
//! - [`error`] - Sync error types
//! - [`inbound`] - Handler for incoming updates
//! - [`outbox`] - Outbox processor for uploads
//! - [`protocol`] - Message types for sync communication
//! - [`transport`] - WebSocket client with reconnection
//!
//! ## Usage
//!
//! ```rust,ignore
//! use titan_sync::{SyncAgent, SyncConfig};
//! use titan_db::Database;
//!
//! // Create sync configuration
//! let config = SyncConfig::load_or_default()?;
//!
//! // Create and start sync agent
//! let agent = SyncAgent::new(config, database);
//! agent.start().await?;
//!
//! // Query sync status
//! let status = agent.status().await;
//! println!("Connected: {}", status.is_connected);
//! ```

// =============================================================================
// Module Declarations
// =============================================================================

pub mod agent;
pub mod config;
pub mod error;
pub mod inbound;
pub mod outbox;
pub mod protocol;
pub mod transport;

// =============================================================================
// Re-exports
// =============================================================================

pub use agent::{SyncAgent, SyncAgentHandle, SyncEventEmitter, SyncStatus};
pub use config::{SyncConfig, SyncMode};
pub use error::{SyncError, SyncResult};
pub use protocol::{SyncMessage, SyncMessageKind};
pub use transport::ConnectionState;
