//! # Cloud Uplink Adapter
//!
//! Adapts the CloudUplink gRPC client to the CloudUplinkCallback trait
//! used by HubCore for cloud synchronization.
//!
//! ## Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                     Cloud Uplink Adapter Flow                           │
//! │                                                                         │
//! │  ┌───────────────┐     ┌──────────────────┐     ┌──────────────────┐   │
//! │  │   HubCore     │     │ CloudUplinkAdapter│     │   CloudUplink    │   │
//! │  │               │────▶│                  │────▶│   (gRPC client)  │   │
//! │  │ Calls trait   │     │ Implements       │     │                  │   │
//! │  │ methods       │     │ CloudUplinkCallback    │ upload_batch()   │   │
//! │  └───────────────┘     └──────────────────┘     └────────┬─────────┘   │
//! │                                                           │             │
//! │                                                           ▼             │
//! │                                              ┌──────────────────────┐   │
//! │                                              │   Cloud API (gRPC)   │   │
//! │                                              │   PostgreSQL backend │   │
//! │                                              └──────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//! ```rust,ignore
//! use titan_sync::{CloudUplink, CloudUplinkConfig, CloudUplinkAdapter, HubCore};
//!
//! // Create the gRPC client
//! let config = CloudUplinkConfig { /* ... */ };
//! let mut uplink = CloudUplink::new(config)?;
//! uplink.connect().await?;
//!
//! // Wrap it in the adapter
//! let adapter = CloudUplinkAdapter::new(uplink);
//!
//! // Use with HubCore
//! let hub = HubCore::with_cloud_uplink("store-id".into(), Arc::new(adapter));
//! ```

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::cloud_uplink::CloudUplink;
use crate::error::SyncResult;
use crate::hub_core::CloudUplinkCallback;
use crate::proto::{sync_entity, InventoryDelta as ProtoInventoryDelta, SyncEntity, Timestamp};

/// Adapter that wraps CloudUplink (gRPC client) and implements CloudUplinkCallback.
///
/// This allows HubCore to sync to the cloud without knowing the gRPC details.
pub struct CloudUplinkAdapter {
    /// The underlying gRPC client.
    uplink: Arc<RwLock<CloudUplink>>,
    /// Whether to buffer entities before upload.
    buffer_enabled: bool,
    /// Buffer of pending entities (when buffering is enabled).
    buffer: RwLock<Vec<SyncEntity>>,
    /// Buffer flush threshold.
    buffer_threshold: usize,
}

impl CloudUplinkAdapter {
    /// Create a new adapter wrapping a CloudUplink client.
    pub fn new(uplink: CloudUplink) -> Self {
        Self {
            uplink: Arc::new(RwLock::new(uplink)),
            buffer_enabled: false,
            buffer: RwLock::new(Vec::new()),
            buffer_threshold: 50,
        }
    }

    /// Create a new adapter with buffering enabled.
    ///
    /// Entities will be buffered and flushed when the threshold is reached,
    /// or when `flush()` is called explicitly.
    pub fn with_buffering(uplink: CloudUplink, buffer_threshold: usize) -> Self {
        Self {
            uplink: Arc::new(RwLock::new(uplink)),
            buffer_enabled: true,
            buffer: RwLock::new(Vec::new()),
            buffer_threshold,
        }
    }

    /// Manually flush the buffer to the cloud.
    pub async fn flush(&self) -> SyncResult<()> {
        let mut buffer = self.buffer.write().await;
        if buffer.is_empty() {
            return Ok(());
        }

        let entities: Vec<_> = buffer.drain(..).collect();
        drop(buffer);

        let uplink = self.uplink.read().await;
        match uplink.upload_batch(entities).await {
            Ok(response) => {
                info!(
                    synced = response.synced_ids.len(),
                    errors = response.errors.len(),
                    "Flushed buffer to cloud"
                );
                Ok(())
            }
            Err(e) => {
                warn!(?e, "Failed to flush buffer to cloud");
                Err(e)
            }
        }
    }

    /// Check if the uplink is connected.
    pub async fn is_connected(&self) -> bool {
        let uplink = self.uplink.read().await;
        uplink.is_connected().await
    }
}

#[async_trait]
impl CloudUplinkCallback for CloudUplinkAdapter {
    /// Sync an entity to the cloud.
    ///
    /// Creates a SyncEntity from the provided data and either buffers it
    /// or uploads immediately depending on configuration.
    async fn sync_entity(
        &self,
        entity_type: &str,
        entity_id: &str,
        _payload: &str,
    ) -> SyncResult<()> {
        info!(
            entity_type = %entity_type,
            entity_id = %entity_id,
            "Syncing entity to cloud"
        );

        // Create the SyncEntity
        // Note: For generic JSON payloads, we'd need to add a GenericJson variant to the proto.
        // For now, we'll skip entities that don't have a specific proto type.
        // Sales, Payments, and InventoryDeltas have their own types.

        // This is a simplified implementation - in production, you'd parse the payload
        // and create the appropriate proto type based on entity_type
        let entity = SyncEntity {
            entity_id: entity_id.to_string(),
            entity_type: entity_type.to_string(),
            created_at: Some(Timestamp {
                value: chrono::Utc::now().to_rfc3339(),
            }),
            device_sequence: 0, // Would be tracked per-device in production
            data: None,         // Generic JSON not supported in current proto
        };

        if self.buffer_enabled {
            let mut buffer = self.buffer.write().await;
            buffer.push(entity);

            if buffer.len() >= self.buffer_threshold {
                drop(buffer);
                self.flush().await?;
            }
        } else {
            // Immediate upload
            let uplink = self.uplink.read().await;
            uplink.upload_batch(vec![entity]).await?;
        }

        Ok(())
    }

    /// Record an inventory delta to the cloud.
    ///
    /// Creates an InventoryDelta proto and uploads it.
    /// Note: `sku` is not used in the proto but kept for logging/debugging.
    async fn record_inventory_delta(
        &self,
        product_id: &str,
        _sku: &str, // SKU can be looked up from product_id in cloud
        delta: i32,
        source: &str,
    ) -> SyncResult<()> {
        info!(
            product_id = %product_id,
            delta = delta,
            source = %source,
            "Recording inventory delta to cloud"
        );

        let delta_id = uuid::Uuid::new_v4().to_string();

        let entity = SyncEntity {
            entity_id: delta_id.clone(),
            entity_type: "INVENTORY_DELTA".to_string(),
            created_at: Some(Timestamp {
                value: chrono::Utc::now().to_rfc3339(),
            }),
            device_sequence: 0,
            data: Some(sync_entity::Data::InventoryDelta(ProtoInventoryDelta {
                id: delta_id,
                store_id: String::new(), // Filled by cloud API from JWT
                device_id: source.to_string(),
                product_id: product_id.to_string(),
                delta,
                reason: "SALE".to_string(),
                reference_id: String::new(),
                created_at: Some(Timestamp {
                    value: chrono::Utc::now().to_rfc3339(),
                }),
            })),
        };

        if self.buffer_enabled {
            let mut buffer = self.buffer.write().await;
            buffer.push(entity);

            if buffer.len() >= self.buffer_threshold {
                drop(buffer);
                self.flush().await?;
            }
        } else {
            let uplink = self.uplink.read().await;
            uplink.upload_batch(vec![entity]).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_uplink::CloudUplinkConfig;

    // Note: These tests require a running cloud-api server
    // Use `docker compose --profile cloud up -d` to start it

    #[tokio::test]
    #[ignore = "requires running cloud-api server"]
    async fn test_adapter_sync_entity() {
        let config = CloudUplinkConfig {
            cloud_url: "http://localhost:50051".to_string(),
            store_id: "test-store".to_string(),
            tenant_id: "test-tenant".to_string(),
            api_key: "test-key".to_string(),
            device_id: "test-device".to_string(),
            device_name: Some("Test Device".to_string()),
            verify_tls: false,
            ..Default::default()
        };

        let mut uplink = CloudUplink::new(config).expect("create uplink");
        uplink.connect().await.expect("connect");

        let adapter = CloudUplinkAdapter::new(uplink);

        let result = adapter
            .sync_entity(
                "SALE",
                "test-sale-123",
                r#"{"id": "test-sale-123", "total": 1000}"#,
            )
            .await;

        assert!(result.is_ok());
    }
}
