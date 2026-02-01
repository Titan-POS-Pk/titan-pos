//! # Inventory Aggregator Module
//!
//! Aggregates inventory deltas from SECONDARY devices and broadcasts updates.
//! Supports two broadcast modes:
//! - **Immediate**: Broadcasts each delta as it arrives
//! - **Coalesced**: Batches deltas over a time window (default: 50ms) for efficiency
//!
//! ## Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                     Inventory Aggregator                                │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                    Message Flow                                  │   │
//! │  │                                                                 │   │
//! │  │  POS #1 ──┐                                                     │   │
//! │  │           │ InventoryDelta                                      │   │
//! │  │  POS #2 ──┼────────────────▶ ┌─────────────────┐                │   │
//! │  │           │                  │                 │                │   │
//! │  │  POS #3 ──┘                  │   Aggregator    │                │   │
//! │  │                              │                 │                │   │
//! │  │                              │  ┌───────────┐  │                │   │
//! │  │                              │  │ Pending   │  │                │   │
//! │  │                              │  │ Deltas    │  │                │   │
//! │  │                              │  │           │  │                │   │
//! │  │                              │  │ SKU: -2   │  │                │   │
//! │  │                              │  │ SKU: +5   │  │                │   │
//! │  │                              │  └───────────┘  │                │   │
//! │  │                              │                 │                │   │
//! │  │                              └────────┬────────┘                │   │
//! │  │                                       │                         │   │
//! │  │           ┌───────────────────────────┴───────────────────┐    │   │
//! │  │           │                                               │    │   │
//! │  │           ▼ IMMEDIATE MODE              ▼ COALESCED MODE  │    │   │
//! │  │  ┌─────────────────┐           ┌─────────────────────┐    │   │
//! │  │  │ Broadcast each  │           │ Wait 50ms, then     │    │   │
//! │  │  │ delta as it     │           │ broadcast merged    │    │   │
//! │  │  │ arrives         │           │ deltas              │    │   │
//! │  │  └─────────────────┘           └─────────────────────┘    │   │
//! │  │                                                            │   │
//! │  └─────────────────────────────────────────────────────────────┘   │
//! │                                                                     │
//! │  CRDT Delta Merging:                                               │
//! │  ────────────────────                                              │
//! │  • Deltas are ADDITIVE: delta(-2) + delta(+5) = delta(+3)         │
//! │  • Never overwrite absolute values                                 │
//! │  • Handles concurrent updates from multiple POS devices            │
//! │                                                                     │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Instant};
use tracing::{debug, error, info, warn};

use crate::error::{SyncError, SyncResult};
use crate::hub::HubHandle;
use crate::protocol::{InventoryDelta, InventoryUpdate, SyncMessage};

// =============================================================================
// Constants
// =============================================================================

/// Default coalesce window in milliseconds.
pub const DEFAULT_COALESCE_WINDOW_MS: u64 = 50;

/// Maximum pending deltas before force flush.
const MAX_PENDING_DELTAS: usize = 1000;

// =============================================================================
// Broadcast Mode
// =============================================================================

/// Inventory broadcast mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BroadcastMode {
    /// Broadcast each delta immediately as it arrives.
    /// Lower latency, higher network traffic.
    Immediate,

    /// Coalesce deltas over a time window before broadcasting.
    /// Higher latency, lower network traffic.
    /// Default mode.
    #[default]
    Coalesced,
}

impl std::fmt::Display for BroadcastMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BroadcastMode::Immediate => write!(f, "immediate"),
            BroadcastMode::Coalesced => write!(f, "coalesced"),
        }
    }
}

// =============================================================================
// Aggregator Configuration
// =============================================================================

/// Configuration for the inventory aggregator.
#[derive(Debug, Clone)]
pub struct AggregatorConfig {
    /// Broadcast mode.
    pub mode: BroadcastMode,
    /// Coalesce window (only used in Coalesced mode).
    pub coalesce_window: Duration,
}

impl Default for AggregatorConfig {
    fn default() -> Self {
        AggregatorConfig {
            mode: BroadcastMode::Coalesced,
            coalesce_window: Duration::from_millis(DEFAULT_COALESCE_WINDOW_MS),
        }
    }
}

impl AggregatorConfig {
    /// Creates a config for immediate mode.
    pub fn immediate() -> Self {
        AggregatorConfig {
            mode: BroadcastMode::Immediate,
            coalesce_window: Duration::ZERO,
        }
    }

    /// Creates a config for coalesced mode with custom window.
    pub fn coalesced(window_ms: u64) -> Self {
        AggregatorConfig {
            mode: BroadcastMode::Coalesced,
            coalesce_window: Duration::from_millis(window_ms),
        }
    }
}

// =============================================================================
// Pending Delta
// =============================================================================

/// A pending delta waiting to be broadcast.
#[derive(Debug, Clone)]
struct PendingDelta {
    /// Product ID (UUID).
    product_id: String,
    /// SKU snapshot at time of delta.
    sku: String,
    /// Cumulative delta quantity.
    delta_quantity: i32,
    /// Source device ID.
    source_device: String,
    /// Timestamp of first delta.
    first_seen: Instant,
    /// Timestamp of most recent delta.
    last_seen: Instant,
}

// =============================================================================
// Inventory Aggregator
// =============================================================================

/// Aggregates inventory deltas and broadcasts updates.
pub struct InventoryAggregator {
    /// Configuration.
    config: AggregatorConfig,
    /// Hub handle for broadcasting.
    hub: HubHandle,
    /// Pending deltas keyed by product_id.
    pending: Arc<RwLock<HashMap<String, PendingDelta>>>,
}

/// Handle for controlling the aggregator.
#[derive(Clone)]
pub struct AggregatorHandle {
    /// Command sender.
    cmd_tx: mpsc::Sender<AggregatorCommand>,
}

/// Commands for the aggregator.
#[derive(Debug)]
enum AggregatorCommand {
    /// Process an incoming delta.
    ProcessDelta {
        source_device: String,
        delta: InventoryDelta,
    },
    /// Force flush all pending deltas.
    Flush,
    /// Shutdown the aggregator.
    Shutdown,
}

impl AggregatorHandle {
    /// Processes an inventory delta.
    pub async fn process_delta(&self, source_device: String, delta: InventoryDelta) -> SyncResult<()> {
        self.cmd_tx
            .send(AggregatorCommand::ProcessDelta { source_device, delta })
            .await
            .map_err(|_| SyncError::ChannelError("Aggregator channel closed".into()))
    }

    /// Forces a flush of all pending deltas.
    pub async fn flush(&self) -> SyncResult<()> {
        self.cmd_tx
            .send(AggregatorCommand::Flush)
            .await
            .map_err(|_| SyncError::ChannelError("Aggregator channel closed".into()))
    }

    /// Shuts down the aggregator.
    pub async fn shutdown(&self) -> SyncResult<()> {
        self.cmd_tx
            .send(AggregatorCommand::Shutdown)
            .await
            .map_err(|_| SyncError::ChannelError("Aggregator channel closed".into()))
    }
}

impl InventoryAggregator {
    /// Creates a new aggregator.
    pub fn new(config: AggregatorConfig, hub: HubHandle) -> Self {
        InventoryAggregator {
            config,
            hub,
            pending: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Starts the aggregator and returns a handle.
    pub fn start(self) -> AggregatorHandle {
        let (cmd_tx, cmd_rx) = mpsc::channel(256);

        tokio::spawn(async move {
            self.run(cmd_rx).await;
        });

        AggregatorHandle { cmd_tx }
    }

    /// Main aggregator loop.
    async fn run(self, mut cmd_rx: mpsc::Receiver<AggregatorCommand>) {
        info!(mode = %self.config.mode, "Inventory aggregator started");

        // Coalesce timer (only active in Coalesced mode)
        let mut coalesce_interval = interval(self.config.coalesce_window);

        loop {
            tokio::select! {
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        AggregatorCommand::Shutdown => {
                            info!("Inventory aggregator shutting down");
                            // Flush remaining deltas
                            self.flush_pending().await;
                            break;
                        }
                        AggregatorCommand::Flush => {
                            self.flush_pending().await;
                        }
                        AggregatorCommand::ProcessDelta { source_device, delta } => {
                            self.handle_delta(source_device, delta).await;
                        }
                    }
                }
                _ = coalesce_interval.tick(), if self.config.mode == BroadcastMode::Coalesced => {
                    self.flush_pending().await;
                }
            }
        }
    }

    /// Handles an incoming delta.
    async fn handle_delta(&self, source_device: String, delta: InventoryDelta) {
        debug!(
            source = %source_device,
            product_id = %delta.product_id,
            sku = %delta.sku,
            delta = delta.delta_quantity,
            "Received inventory delta"
        );

        match self.config.mode {
            BroadcastMode::Immediate => {
                // Broadcast immediately
                self.broadcast_delta(&delta, &source_device).await;
            }
            BroadcastMode::Coalesced => {
                // Add to pending deltas
                self.add_pending_delta(source_device, delta).await;

                // Force flush if too many pending
                let pending_count = self.pending.read().await.len();
                if pending_count >= MAX_PENDING_DELTAS {
                    warn!(count = pending_count, "Too many pending deltas - forcing flush");
                    self.flush_pending().await;
                }
            }
        }
    }

    /// Adds a delta to the pending map (coalescing with existing deltas).
    async fn add_pending_delta(&self, source_device: String, delta: InventoryDelta) {
        let mut pending = self.pending.write().await;
        let now = Instant::now();

        match pending.get_mut(&delta.product_id) {
            Some(existing) => {
                // Merge with existing delta (CRDT: additive)
                existing.delta_quantity += delta.delta_quantity;
                existing.last_seen = now;
                debug!(
                    product_id = %delta.product_id,
                    merged_delta = existing.delta_quantity,
                    "Coalesced delta"
                );
            }
            None => {
                // Insert new pending delta
                pending.insert(
                    delta.product_id.clone(),
                    PendingDelta {
                        product_id: delta.product_id,
                        sku: delta.sku,
                        delta_quantity: delta.delta_quantity,
                        source_device,
                        first_seen: now,
                        last_seen: now,
                    },
                );
            }
        }
    }

    /// Flushes all pending deltas.
    async fn flush_pending(&self) {
        let deltas: Vec<PendingDelta> = {
            let mut pending = self.pending.write().await;
            if pending.is_empty() {
                return;
            }
            pending.drain().map(|(_, v)| v).collect()
        };

        debug!(count = deltas.len(), "Flushing pending deltas");

        // Broadcast each coalesced delta
        for pending_delta in deltas {
            // Skip if delta_quantity is 0 (no net change)
            if pending_delta.delta_quantity == 0 {
                debug!(
                    product_id = %pending_delta.product_id,
                    "Skipping zero-sum delta"
                );
                continue;
            }

            let delta = InventoryDelta {
                product_id: pending_delta.product_id,
                sku: pending_delta.sku,
                delta_quantity: pending_delta.delta_quantity,
                timestamp: chrono::Utc::now().to_rfc3339(),
            };

            self.broadcast_delta(&delta, &pending_delta.source_device).await;
        }
    }

    /// Broadcasts a single delta as an InventoryUpdate.
    async fn broadcast_delta(&self, delta: &InventoryDelta, source_device: &str) {
        let update = SyncMessage::InventoryUpdate(InventoryUpdate {
            product_id: delta.product_id.clone(),
            sku: delta.sku.clone(),
            delta_quantity: delta.delta_quantity,
            source_device_id: source_device.to_string(),
            timestamp: delta.timestamp.clone(),
        });

        if let Err(e) = self.hub.broadcast(update) {
            error!(?e, "Failed to broadcast inventory update");
        }
    }
}

// =============================================================================
// Delta Processor
// =============================================================================

/// Processes incoming messages from the hub and routes them to the aggregator.
pub struct DeltaProcessor {
    /// Aggregator handle.
    aggregator: AggregatorHandle,
}

impl DeltaProcessor {
    /// Creates a new delta processor.
    pub fn new(aggregator: AggregatorHandle) -> Self {
        DeltaProcessor { aggregator }
    }

    /// Starts processing messages from the given receiver.
    pub async fn start(self, mut delta_rx: mpsc::Receiver<(String, SyncMessage)>) {
        info!("Delta processor started");

        while let Some((device_id, msg)) = delta_rx.recv().await {
            match msg {
                SyncMessage::InventoryDelta(delta) => {
                    if let Err(e) = self.aggregator.process_delta(device_id, delta).await {
                        error!(?e, "Failed to process inventory delta");
                    }
                }
                SyncMessage::OutboxBatch(batch) => {
                    // Process each entity in the batch
                    for entity in batch.entities {
                        if entity.entity_type == "InventoryDelta" {
                            if let Ok(delta) = serde_json::from_str::<InventoryDelta>(&entity.payload) {
                                if let Err(e) = self.aggregator.process_delta(device_id.clone(), delta).await {
                                    error!(?e, "Failed to process delta from batch");
                                }
                            }
                        }
                    }
                }
                other => {
                    debug!(?other, "Ignoring non-delta message");
                }
            }
        }

        info!("Delta processor stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broadcast_mode_display() {
        assert_eq!(BroadcastMode::Immediate.to_string(), "immediate");
        assert_eq!(BroadcastMode::Coalesced.to_string(), "coalesced");
    }

    #[test]
    fn test_aggregator_config_default() {
        let config = AggregatorConfig::default();
        assert_eq!(config.mode, BroadcastMode::Coalesced);
        assert_eq!(config.coalesce_window, Duration::from_millis(50));
    }

    #[test]
    fn test_aggregator_config_immediate() {
        let config = AggregatorConfig::immediate();
        assert_eq!(config.mode, BroadcastMode::Immediate);
    }

    #[test]
    fn test_aggregator_config_custom_window() {
        let config = AggregatorConfig::coalesced(100);
        assert_eq!(config.mode, BroadcastMode::Coalesced);
        assert_eq!(config.coalesce_window, Duration::from_millis(100));
    }
}
