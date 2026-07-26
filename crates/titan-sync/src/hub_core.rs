//! # Hub Core Module
//!
//! Shared hub logic that can be used both in embedded mode (PRIMARY POS)
//! and standalone mode (dedicated hub server).
//!
//! ## Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         Hub Core Architecture                           │
//! │                                                                         │
//! │  This module provides the shared "brain" of the hub that handles:       │
//! │  • Client connection management                                         │
//! │  • Message routing (InventoryDelta → InventoryUpdate broadcast)        │
//! │  • Outbox processing (EntityUpdate broadcast)                          │
//! │  • Store ID validation                                                  │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                        HubCore                                  │   │
//! │  │                                                                 │   │
//! │  │  ┌─────────────────┐  ┌─────────────────┐                      │   │
//! │  │  │ ClientRegistry  │  │ MessageRouter   │                      │   │
//! │  │  │ • Add/remove    │  │ • InventoryDelta│                      │   │
//! │  │  │ • Lookup        │  │ • OutboxBatch   │                      │   │
//! │  │  │ • Broadcast     │  │ • EntityUpdate  │                      │   │
//! │  │  └─────────────────┘  └─────────────────┘                      │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                               │                                         │
//! │        ┌──────────────────────┼──────────────────────┐                 │
//! │        ▼                      ▼                      ▼                 │
//! │  ┌──────────┐          ┌──────────┐          ┌──────────┐             │
//! │  │ Embedded │          │Standalone│          │  Cloud   │             │
//! │  │   Hub    │          │   Hub    │          │  Uplink  │             │
//! │  │(PRIMARY) │          │ (binary) │          │ (gRPC)   │             │
//! │  └──────────┘          └──────────┘          └──────────┘             │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//! ```rust,ignore
//! // Create hub core
//! let hub = HubCore::new("demo-store".to_string());
//!
//! // Register a client
//! let client_tx = hub.register_client("device-1", "Demo Device", addr).await?;
//!
//! // Process incoming message
//! hub.handle_message("device-1", message).await?;
//!
//! // Get stats
//! let stats = hub.stats().await;
//! ```

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, info, warn};

use crate::error::{SyncError, SyncResult};
use crate::protocol::{
    BatchAck, EntityUpdate, InventoryDelta, InventoryUpdate, OutboxBatch, SyncMessage,
};

// =============================================================================
// Hub Core Types
// =============================================================================

/// Connected client information.
#[derive(Debug, Clone)]
pub struct ConnectedClient {
    /// Device identifier.
    pub device_id: String,
    /// Human-readable device name.
    pub device_name: String,
    /// Store ID this device belongs to.
    pub store_id: String,
    /// Client's remote address.
    pub addr: SocketAddr,
    /// When the client connected.
    pub connected_at: Instant,
    /// Channel to send messages to this specific client.
    pub tx: mpsc::Sender<String>,
}

/// Hub statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubStats {
    /// Number of connected clients.
    pub connected_clients: usize,
    /// List of connected device IDs.
    pub clients: Vec<String>,
    /// Store ID this hub serves.
    pub store_id: String,
    /// Hub status.
    pub status: String,
}

/// Callback trait for cloud uplink integration.
/// Implement this to sync messages to the cloud database.
///
/// Uses `#[async_trait]` to be dyn-compatible (required for `Arc<dyn CloudUplinkCallback>`).
#[async_trait]
pub trait CloudUplinkCallback: Send + Sync {
    /// Called when an entity needs to be synced to cloud.
    async fn sync_entity(
        &self,
        entity_type: &str,
        entity_id: &str,
        payload: &str,
    ) -> SyncResult<()>;

    /// Called when an inventory delta needs to be recorded.
    async fn record_inventory_delta(
        &self,
        product_id: &str,
        sku: &str,
        delta: i32,
        source: &str,
    ) -> SyncResult<()>;
}

/// No-op cloud uplink (for standalone hub without cloud connection).
pub struct NoOpCloudUplink;

#[async_trait]
impl CloudUplinkCallback for NoOpCloudUplink {
    async fn sync_entity(
        &self,
        _entity_type: &str,
        _entity_id: &str,
        _payload: &str,
    ) -> SyncResult<()> {
        Ok(())
    }

    async fn record_inventory_delta(
        &self,
        _product_id: &str,
        _sku: &str,
        _delta: i32,
        _source: &str,
    ) -> SyncResult<()> {
        Ok(())
    }
}

// =============================================================================
// Hub Core
// =============================================================================

/// The core hub logic shared between embedded and standalone modes.
pub struct HubCore {
    /// Store ID this hub serves.
    store_id: String,
    /// Connected clients indexed by device_id.
    clients: RwLock<HashMap<String, ConnectedClient>>,
    /// Broadcast channel for sending to all clients.
    broadcast_tx: broadcast::Sender<String>,
    /// Optional cloud uplink callback.
    cloud_uplink: Option<Arc<dyn CloudUplinkCallback>>,
}

impl HubCore {
    /// Creates a new hub core.
    pub fn new(store_id: String) -> Self {
        let (broadcast_tx, _) = broadcast::channel(256);

        HubCore {
            store_id,
            clients: RwLock::new(HashMap::new()),
            broadcast_tx,
            cloud_uplink: None,
        }
    }

    /// Creates a new hub core with cloud uplink.
    pub fn with_cloud_uplink(store_id: String, uplink: Arc<dyn CloudUplinkCallback>) -> Self {
        let (broadcast_tx, _) = broadcast::channel(256);

        HubCore {
            store_id,
            clients: RwLock::new(HashMap::new()),
            broadcast_tx,
            cloud_uplink: Some(uplink),
        }
    }

    /// Returns the store ID.
    pub fn store_id(&self) -> &str {
        &self.store_id
    }

    /// Validates that a client's store ID matches.
    pub fn validate_store_id(&self, client_store_id: &str) -> SyncResult<()> {
        if client_store_id != self.store_id {
            return Err(SyncError::StoreMismatch {
                expected: self.store_id.clone(),
                actual: client_store_id.to_string(),
            });
        }
        Ok(())
    }

    /// Registers a new client and returns a channel for sending messages to them.
    pub async fn register_client(
        &self,
        device_id: &str,
        device_name: &str,
        store_id: &str,
        addr: SocketAddr,
    ) -> SyncResult<(
        mpsc::Sender<String>,
        mpsc::Receiver<String>,
        broadcast::Receiver<String>,
    )> {
        // Validate store ID
        self.validate_store_id(store_id)?;

        // Create channels for this client
        let (tx, rx) = mpsc::channel(100);
        let broadcast_rx = self.broadcast_tx.subscribe();

        // Register the client
        let client = ConnectedClient {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            store_id: store_id.to_string(),
            addr,
            connected_at: Instant::now(),
            tx: tx.clone(),
        };

        // Capture values for logging before await
        let device_id_owned = device_id.to_string();
        let device_name_owned = device_name.to_string();

        let client_count = {
            let mut clients = self.clients.write().await;
            clients.insert(device_id.to_string(), client);
            clients.len()
        };

        info!(
            device_id = %device_id_owned,
            device_name = %device_name_owned,
            addr = %addr,
            clients = client_count,
            "Client registered"
        );

        Ok((tx, rx, broadcast_rx))
    }

    /// Removes a client.
    pub async fn remove_client(&self, device_id: &str) {
        let mut clients = self.clients.write().await;
        if clients.remove(device_id).is_some() {
            info!(
                device_id = %device_id,
                remaining = clients.len(),
                "Client disconnected"
            );
        }
    }

    /// Returns hub statistics.
    pub async fn stats(&self) -> HubStats {
        let clients = self.clients.read().await;
        HubStats {
            connected_clients: clients.len(),
            clients: clients.keys().cloned().collect(),
            store_id: self.store_id.clone(),
            status: "running".to_string(),
        }
    }

    /// Broadcasts a message to all connected clients.
    pub async fn broadcast(&self, message: &str) {
        let _ = self.broadcast_tx.send(message.to_string());
    }

    /// Sends a message to a specific client.
    pub async fn send_to_client(&self, device_id: &str, message: &str) -> SyncResult<()> {
        let clients = self.clients.read().await;
        if let Some(client) = clients.get(device_id) {
            client
                .tx
                .send(message.to_string())
                .await
                .map_err(|_| SyncError::ChannelError("Client channel closed".into()))?;
        }
        Ok(())
    }

    /// Handles an incoming message from a client.
    /// Returns an optional response to send back to the client.
    pub async fn handle_message(
        &self,
        device_id: &str,
        message: SyncMessage,
    ) -> SyncResult<Option<String>> {
        match message {
            SyncMessage::InventoryDelta(delta) => {
                self.handle_inventory_delta(device_id, delta).await
            }

            SyncMessage::OutboxBatch(batch) => self.handle_outbox_batch(device_id, batch).await,

            SyncMessage::UpdateAck(ack) => {
                debug!(
                    device_id = %device_id,
                    entity_id = %ack.entity_id,
                    success = ack.success,
                    "Received UpdateAck"
                );
                Ok(None)
            }

            _ => {
                debug!(device_id = %device_id, ?message, "Ignoring message type");
                Ok(None)
            }
        }
    }

    /// Handles an inventory delta from a client.
    async fn handle_inventory_delta(
        &self,
        device_id: &str,
        delta: InventoryDelta,
    ) -> SyncResult<Option<String>> {
        info!(
            device_id = %device_id,
            product_id = %delta.product_id,
            sku = %delta.sku,
            delta = delta.delta_quantity,
            "Received InventoryDelta"
        );

        // Record to cloud if uplink is configured
        if let Some(ref uplink) = self.cloud_uplink {
            if let Err(e) = uplink
                .record_inventory_delta(
                    &delta.product_id,
                    &delta.sku,
                    delta.delta_quantity,
                    device_id,
                )
                .await
            {
                warn!(?e, "Failed to record inventory delta to cloud");
            }
        }

        // Convert to InventoryUpdate and broadcast to ALL clients
        let update = SyncMessage::InventoryUpdate(InventoryUpdate {
            product_id: delta.product_id,
            sku: delta.sku,
            delta_quantity: delta.delta_quantity,
            source_device_id: device_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        });

        if let Ok(json) = serde_json::to_string(&update) {
            info!("Broadcasting InventoryUpdate to all clients");
            self.broadcast(&json).await;
        }

        Ok(None)
    }

    /// Handles an outbox batch from a client.
    async fn handle_outbox_batch(
        &self,
        device_id: &str,
        batch: OutboxBatch,
    ) -> SyncResult<Option<String>> {
        info!(
            device_id = %device_id,
            entities = batch.entities.len(),
            batch_seq = batch.batch_seq,
            "Received OutboxBatch"
        );

        let mut acked_ids = Vec::new();

        for entity in &batch.entities {
            // Sync to cloud if uplink is configured
            if let Some(ref uplink) = self.cloud_uplink {
                if let Err(e) = uplink
                    .sync_entity(&entity.entity_type, &entity.entity_id, &entity.payload)
                    .await
                {
                    warn!(
                        entity_id = %entity.entity_id,
                        ?e,
                        "Failed to sync entity to cloud"
                    );
                }
            }

            // Broadcast entity update to all OTHER clients
            let update = EntityUpdate {
                entity_type: entity.entity_type.clone(),
                entity_id: entity.entity_id.clone(),
                operation: "upsert".to_string(),
                data: serde_json::from_str(&entity.payload).unwrap_or(serde_json::Value::Null),
                version: 1,
                // Use created_at as the updated_at timestamp
                updated_at: entity.created_at.clone(),
            };

            if let Ok(json) = serde_json::to_string(&SyncMessage::EntityUpdate(update)) {
                // Broadcast to all clients (source will filter self-echo)
                self.broadcast(&json).await;
            }

            acked_ids.push(entity.id.clone());
        }

        // Send batch acknowledgement
        let ack = SyncMessage::BatchAck(BatchAck {
            acked_ids,
            failed_ids: vec![],
            new_cursor: 0,
        });

        Ok(serde_json::to_string(&ack).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[tokio::test]
    async fn test_hub_core_creation() {
        let hub = HubCore::new("test-store".to_string());
        assert_eq!(hub.store_id(), "test-store");

        let stats = hub.stats().await;
        assert_eq!(stats.connected_clients, 0);
    }

    #[tokio::test]
    async fn test_client_registration() {
        let hub = HubCore::new("test-store".to_string());
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);

        let result = hub
            .register_client("device-1", "Test Device", "test-store", addr)
            .await;
        assert!(result.is_ok());

        let stats = hub.stats().await;
        assert_eq!(stats.connected_clients, 1);
        assert!(stats.clients.contains(&"device-1".to_string()));
    }

    #[tokio::test]
    async fn test_store_id_validation() {
        let hub = HubCore::new("correct-store".to_string());

        // Valid store ID
        assert!(hub.validate_store_id("correct-store").is_ok());

        // Invalid store ID
        let result = hub.validate_store_id("wrong-store");
        assert!(result.is_err());
    }
}
