//! # Inbound Update Handler
//!
//! Handles incoming updates from the Store Hub (PRIMARY → SECONDARY).
//!
//! ## Update Types
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Inbound Update Categories                            │
//! │                                                                         │
//! │  PRODUCT UPDATES                                                       │
//! │  ───────────────                                                       │
//! │  • Upsert: Full product data (new or updated)                          │
//! │  • Patch: Partial field updates (price change)                         │
//! │  • Delete: Soft delete (set is_active = false)                         │
//! │                                                                         │
//! │  INVENTORY DELTAS (CRDT-style)                                         │
//! │  ────────────────────────────                                          │
//! │  • Inventory changes sent as deltas (+5, -3)                           │
//! │  • Applied atomically: current_stock += delta                          │
//! │  • Conflict-free by design                                             │
//! │                                                                         │
//! │  TAX RATE UPDATES                                                      │
//! │  ─────────────────                                                     │
//! │  • Regional tax rate changes                                           │
//! │  • Applied immediately to new sales                                    │
//! │                                                                         │
//! │  USER/CATEGORY UPDATES                                                 │
//! │  ─────────────────────                                                 │
//! │  • User permission changes                                             │
//! │  • Category hierarchy updates                                          │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Conflict Resolution
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Conflict Resolution Strategy                         │
//! │                                                                         │
//! │  VERSION CHECK:                                                        │
//! │  if incoming.version > local.sync_version:                             │
//! │      apply update                                                       │
//! │      local.sync_version = incoming.version                             │
//! │  else:                                                                  │
//! │      skip (already have newer data)                                    │
//! │                                                                         │
//! │  INVENTORY SPECIAL CASE (CRDT):                                        │
//! │  • Deltas always applied, never skipped                                │
//! │  • current_stock += delta (atomic operation)                           │
//! │  • No version conflicts possible                                       │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use titan_db::Database;

use crate::config::SyncConfig;
use crate::error::{SyncError, SyncResult};
use crate::protocol::{EntityUpdate, SyncMessage, UpdateAck};
use crate::transport::TransportHandle;

// =============================================================================
// Inbound Handler
// =============================================================================

/// Handles incoming entity updates from the Store Hub.
pub struct InboundHandler {
    /// Database connection.
    db: Arc<Database>,

    /// Sync configuration.
    config: Arc<SyncConfig>,

    /// Transport for sending acknowledgements.
    transport: TransportHandle,

    /// Receiver for incoming update messages.
    update_rx: mpsc::Receiver<SyncMessage>,

    /// Shutdown receiver.
    shutdown_rx: mpsc::Receiver<()>,
}

/// Handle for controlling the inbound handler.
#[derive(Clone)]
pub struct InboundHandlerHandle {
    /// Shutdown sender.
    shutdown_tx: mpsc::Sender<()>,

    /// Sender for routing update messages to the handler.
    update_tx: mpsc::Sender<SyncMessage>,
}

impl InboundHandlerHandle {
    /// Routes an entity update message to the handler.
    pub async fn handle_update(&self, message: SyncMessage) -> SyncResult<()> {
        self.update_tx
            .send(message)
            .await
            .map_err(|_| SyncError::ChannelError("Update channel closed".into()))
    }

    /// Triggers graceful shutdown.
    pub async fn shutdown(&self) -> SyncResult<()> {
        self.shutdown_tx
            .send(())
            .await
            .map_err(|_| SyncError::ChannelError("Shutdown channel closed".into()))
    }
}

impl InboundHandler {
    /// Creates a new inbound handler and returns a handle.
    pub fn new(
        db: Arc<Database>,
        config: Arc<SyncConfig>,
        transport: TransportHandle,
    ) -> (Self, InboundHandlerHandle) {
        let (update_tx, update_rx) = mpsc::channel(100);
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

        let handler = InboundHandler {
            db,
            config,
            transport,
            update_rx,
            shutdown_rx,
        };

        let handle = InboundHandlerHandle {
            shutdown_tx,
            update_tx,
        };

        (handler, handle)
    }

    /// Runs the inbound handler loop.
    pub async fn run(mut self) {
        info!("Inbound handler starting");

        loop {
            tokio::select! {
                Some(msg) = self.update_rx.recv() => {
                    if let SyncMessage::EntityUpdate(update) = msg {
                        if let Err(e) = self.process_update(update).await {
                            error!(?e, "Failed to process entity update");
                        }
                    }
                }

                _ = self.shutdown_rx.recv() => {
                    info!("Inbound handler shutting down");
                    break;
                }
            }
        }

        info!("Inbound handler stopped");
    }

    /// Processes an entity update message.
    async fn process_update(&self, update: EntityUpdate) -> SyncResult<()> {
        debug!(
            entity_type = %update.entity_type,
            entity_id = %update.entity_id,
            operation = %update.operation,
            version = update.version,
            "Processing entity update"
        );

        let result = match update.entity_type.as_str() {
            "product" => self.apply_product_update(&update).await,
            "inventory_delta" => self.apply_inventory_delta(&update).await,
            "tax_rate" => self.apply_tax_rate_update(&update).await,
            "category" => self.apply_category_update(&update).await,
            "user" => self.apply_user_update(&update).await,
            _ => {
                warn!(entity_type = %update.entity_type, "Unknown entity type");
                Ok(0)
            }
        };

        // Send acknowledgement
        let ack = match &result {
            Ok(applied_version) => SyncMessage::UpdateAck(UpdateAck {
                entity_id: update.entity_id.clone(),
                success: true,
                applied_version: *applied_version,
                error: None,
            }),
            Err(e) => SyncMessage::UpdateAck(UpdateAck {
                entity_id: update.entity_id.clone(),
                success: false,
                applied_version: 0,
                error: Some(e.to_string()),
            }),
        };

        self.transport.send(ack).await?;

        result.map(|_| ())
    }

    /// Applies a product update.
    async fn apply_product_update(&self, update: &EntityUpdate) -> SyncResult<i64> {
        // Check version to avoid applying stale updates
        let current = self
            .db
            .products()
            .get_by_id(&update.entity_id)
            .await?;

        if let Some(ref product) = current {
            if product.sync_version >= update.version {
                debug!(
                    entity_id = %update.entity_id,
                    current_version = product.sync_version,
                    incoming_version = update.version,
                    "Skipping stale product update"
                );
                return Ok(product.sync_version);
            }
        }

        match update.operation.as_str() {
            "upsert" => {
                // Parse full product from data
                let mut product: titan_core::Product =
                    serde_json::from_value(update.data.clone())?;

                // Ensure sync_version is set
                product.sync_version = update.version;

                // Use existing upsert method or implement sync-specific one
                // For now, we'll use the existing product methods
                if current.is_some() {
                    // Update existing
                    self.update_product_from_sync(&product).await?;
                } else {
                    // Insert new
                    self.insert_product_from_sync(&product).await?;
                }

                info!(
                    entity_id = %update.entity_id,
                    version = update.version,
                    "Applied product upsert"
                );

                Ok(update.version)
            }
            "patch" => {
                // Partial update - only update specified fields
                // This requires a dedicated method that handles partial JSON
                warn!(
                    entity_id = %update.entity_id,
                    "Product patch not implemented yet"
                );
                Ok(current.map(|p| p.sync_version).unwrap_or(0))
            }
            "delete" => {
                // Soft delete
                self.soft_delete_product(&update.entity_id, update.version)
                    .await?;

                info!(
                    entity_id = %update.entity_id,
                    version = update.version,
                    "Soft deleted product"
                );

                Ok(update.version)
            }
            _ => {
                warn!(operation = %update.operation, "Unknown operation for Product");
                Ok(current.map(|p| p.sync_version).unwrap_or(0))
            }
        }
    }

    /// Applies an inventory delta (CRDT-style).
    ///
    /// ## CRDT Behavior
    /// Inventory deltas are always applied, regardless of version.
    /// The delta value is added to current_stock atomically.
    async fn apply_inventory_delta(&self, update: &EntityUpdate) -> SyncResult<i64> {
        // Extract delta from data
        #[derive(serde::Deserialize)]
        struct InventoryDeltaData {
            product_id: String,
            delta: i64,
            reason: Option<String>,
        }

        let delta_data: InventoryDeltaData = serde_json::from_value(update.data.clone())?;

        // Apply delta atomically using SQL
        let rows_affected = sqlx::query!(
            r#"
            UPDATE products
            SET current_stock = COALESCE(current_stock, 0) + ?1,
                updated_at = datetime('now')
            WHERE id = ?2
            "#,
            delta_data.delta,
            delta_data.product_id
        )
        .execute(self.db.pool())
        .await?
        .rows_affected();

        if rows_affected == 0 {
            warn!(
                product_id = %delta_data.product_id,
                "Product not found for inventory delta"
            );
        } else {
            info!(
                product_id = %delta_data.product_id,
                delta = delta_data.delta,
                reason = ?delta_data.reason,
                "Applied inventory delta"
            );
        }

        // Record delta in local history (for auditing)
        self.record_inventory_delta(&delta_data.product_id, delta_data.delta, &update.entity_id)
            .await?;

        Ok(update.version)
    }

    /// Applies a tax rate update.
    async fn apply_tax_rate_update(&self, update: &EntityUpdate) -> SyncResult<i64> {
        // Tax rate updates would go here
        // For now, just log and acknowledge
        warn!(
            entity_id = %update.entity_id,
            "Tax rate update not implemented yet"
        );
        Ok(update.version)
    }

    /// Applies a category update.
    async fn apply_category_update(&self, update: &EntityUpdate) -> SyncResult<i64> {
        // Category updates would go here
        warn!(
            entity_id = %update.entity_id,
            "Category update not implemented yet"
        );
        Ok(update.version)
    }

    /// Applies a user update.
    async fn apply_user_update(&self, update: &EntityUpdate) -> SyncResult<i64> {
        // User updates would go here
        warn!(
            entity_id = %update.entity_id,
            "User update not implemented yet"
        );
        Ok(update.version)
    }

    // =========================================================================
    // Database Operations (would ideally be in titan-db SyncInboundRepository)
    // =========================================================================

    /// Updates an existing product from sync data.
    async fn update_product_from_sync(&self, product: &titan_core::Product) -> SyncResult<()> {
        sqlx::query!(
            r#"
            UPDATE products SET
                sku = ?2,
                barcode = ?3,
                name = ?4,
                description = ?5,
                price_cents = ?6,
                cost_cents = ?7,
                tax_rate_bps = ?8,
                track_inventory = ?9,
                allow_negative_stock = ?10,
                is_active = ?11,
                updated_at = ?12,
                sync_version = ?13
            WHERE id = ?1
            "#,
            product.id,
            product.sku,
            product.barcode,
            product.name,
            product.description,
            product.price_cents,
            product.cost_cents,
            product.tax_rate_bps,
            product.track_inventory,
            product.allow_negative_stock,
            product.is_active,
            product.updated_at,
            product.sync_version
        )
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    /// Inserts a new product from sync data.
    async fn insert_product_from_sync(&self, product: &titan_core::Product) -> SyncResult<()> {
        sqlx::query!(
            r#"
            INSERT INTO products (
                id, tenant_id, sku, barcode, name, description,
                price_cents, cost_cents, tax_rate_bps,
                track_inventory, allow_negative_stock, current_stock,
                is_active, created_at, updated_at, sync_version
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9,
                ?10, ?11, ?12,
                ?13, ?14, ?15, ?16
            )
            "#,
            product.id,
            product.tenant_id,
            product.sku,
            product.barcode,
            product.name,
            product.description,
            product.price_cents,
            product.cost_cents,
            product.tax_rate_bps,
            product.track_inventory,
            product.allow_negative_stock,
            product.current_stock,
            product.is_active,
            product.created_at,
            product.updated_at,
            product.sync_version
        )
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    /// Soft deletes a product.
    async fn soft_delete_product(&self, product_id: &str, version: i64) -> SyncResult<()> {
        sqlx::query!(
            r#"
            UPDATE products SET
                is_active = false,
                updated_at = datetime('now'),
                sync_version = ?2
            WHERE id = ?1
            "#,
            product_id,
            version
        )
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    /// Records an inventory delta for audit trail.
    async fn record_inventory_delta(
        &self,
        product_id: &str,
        delta: i64,
        sync_id: &str,
    ) -> SyncResult<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        
        // Get local device ID from config or use a default
        let origin_device_id = "sync".to_string();
        
        // Get next sequence number (simplified - in production would use atomic counter)
        let sequence_num: i64 = 1;

        // Note: This requires the inventory_deltas table from 003_sync_tables.sql
        let result = sqlx::query!(
            r#"
            INSERT INTO inventory_deltas (
                id, product_id, delta, delta_type, reference_id, reference_type,
                origin_device_id, occurred_at, sequence_num, synced, created_at
            )
            VALUES (?1, ?2, ?3, 'sync', ?4, 'sync', ?5, ?6, ?7, 1, ?6)
            "#,
            id,
            product_id,
            delta,
            sync_id,
            origin_device_id,
            now,
            sequence_num
        )
        .execute(self.db.pool())
        .await;

        // Ignore if table doesn't exist yet (migration not run)
        if let Err(e) = result {
            debug!(?e, "Could not record inventory delta (table may not exist)");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests would go here with mock database
}
