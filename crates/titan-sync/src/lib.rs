//! # titan-sync: Sync Engine for Titan POS
//!
//! This crate provides the synchronization layer for Titan POS, enabling
//! offline-first operation with background sync to Store Hub and Cloud.
//!
//! ## Architecture Overview (v0.2+)
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
//! │  MILESTONE 2 ADDITIONS:                                                │
//! │  ────────────────────                                                  │
//! │  ┌────────────────┐  ┌────────────────┐  ┌────────────────────────┐    │
//! │  │   Discovery    │  │   Election     │  │     Hub Server         │    │
//! │  │                │  │                │  │                        │    │
//! │  │ mDNS + UDP     │  │ Leader elect   │  │ Axum WebSocket for     │    │
//! │  │ broadcast for  │  │ with fencing   │  │ accepting SECONDARY    │    │
//! │  │ hub discovery  │  │ tokens         │  │ connections            │    │
//! │  └────────────────┘  └────────────────┘  └────────────────────────┘    │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                    Inventory Aggregator                          │   │
//! │  │                                                                 │   │
//! │  │ Receives inventory deltas from SECONDARY devices                │   │
//! │  │ Aggregates using CRDT principles (additive deltas)              │   │
//! │  │ Broadcasts updates to all connected devices                     │   │
//! │  │ Supports immediate or coalesced (50ms window) broadcasting      │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                                                                         │
//! │  MILESTONE 3 ADDITIONS:                                                │
//! │  ────────────────────                                                  │
//! │  ┌────────────────┐  ┌────────────────┐                                │
//! │  │  CloudAuth     │  │  CloudUplink   │                                │
//! │  │                │  │                │                                │
//! │  │ JWT token mgmt │  │ gRPC client    │                                │
//! │  │ Auto-refresh   │  │ for cloud API  │                                │
//! │  │ API key exch.  │  │ Upload/Download│                                │
//! │  └────────────────┘  └────────────────┘                                │
//! │                                                                         │
//! │  STATUS EVENTS (to Frontend via Tauri):                                │
//! │  • "sync://status" - Connection state changes                          │
//! │  • "sync://progress" - Upload/download progress                        │
//! │  • "sync://error" - Sync failures                                      │
//! │  • "sync://role" - Role changes (PRIMARY/SECONDARY)                    │
//! │  • "sync://election" - Election events                                 │
//! │  • "sync://cloud" - Cloud connection events                            │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Module Organization
//!
//! ### Core Modules (Milestone 1)
//! - [`agent`] - Main `SyncAgent` orchestrator
//! - [`config`] - Sync configuration (mode, device ID, hub URL)
//! - [`error`] - Sync error types
//! - [`inbound`] - Handler for incoming updates
//! - [`outbox`] - Outbox processor for uploads
//! - [`protocol`] - Message types for sync communication
//! - [`transport`] - WebSocket client with reconnection
//!
//! ### Store Hub Modules (Milestone 2)
//! - [`discovery`] - mDNS + UDP broadcast hub discovery
//! - [`election`] - Leader election with fencing tokens
//! - [`hub`] - WebSocket server for PRIMARY mode
//! - [`aggregator`] - Inventory delta aggregation and broadcasting
//!
//! ### Cloud Uplink Modules (Milestone 3)
//! - [`proto`] - Generated gRPC client stubs from proto/titan_sync.proto
//! - [`cloud_auth`] - JWT token management and API key exchange
//! - [`cloud_uplink`] - gRPC client for cloud sync (PRIMARY → Cloud)
//!
//! ## Usage
//!
//! ```rust,ignore
//! use titan_sync::{SyncAgent, SyncConfig};
//! use titan_db::Database;
//!
//! // Create sync configuration
//! let config = SyncConfig::load_or_default(None);
//!
//! // Create and start sync agent
//! let agent = SyncAgent::new(config, database);
//! agent.start().await?;
//!
//! // Query sync status
//! let status = agent.status().await;
//! println!("Connected: {}", status.is_connected);
//! println!("Role: {:?}", status.role);
//! ```

// =============================================================================
// Module Declarations
// =============================================================================

// Core sync modules (Milestone 1)
pub mod agent;
pub mod config;
pub mod error;
pub mod inbound;
pub mod outbox;
pub mod protocol;
pub mod transport;

// Store Hub modules (Milestone 2)
pub mod aggregator;
pub mod discovery;
pub mod election;
pub mod hub;

// Cloud Uplink modules (Milestone 3)
pub mod proto;
pub mod cloud_auth;
pub mod cloud_uplink;

// =============================================================================
// Re-exports
// =============================================================================

// Core types
pub use agent::{SyncAgent, SyncAgentHandle, SyncEventEmitter, SyncStatus};
pub use config::{BroadcastMode, HubSettings, SyncConfig, SyncMode};
pub use error::{SyncError, SyncResult};
pub use protocol::SyncMessage;
pub use transport::ConnectionState;

// Milestone 2 types
pub use aggregator::{AggregatorConfig, AggregatorHandle, InventoryAggregator};
pub use discovery::{DiscoveredHub, DiscoveryConfig, DiscoveryHandle, DiscoveryService};
pub use election::{ElectionConfig, ElectionHandle, ElectionService, ElectionState, NodeRole};
pub use hub::{HubConfig, HubHandle, HubServer};

// Milestone 3 types
pub use cloud_auth::{CloudAuth, CloudAuthConfig, TokenInfo};
pub use cloud_uplink::{CloudUplink, CloudUplinkConfig};
