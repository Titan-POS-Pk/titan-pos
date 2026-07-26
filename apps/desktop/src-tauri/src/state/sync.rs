//! # Sync State Module
//!
//! Manages sync agent state for the Tauri desktop app.
//!
//! ## Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                       Sync State Architecture                           │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                      Tauri Runtime                              │   │
//! │  │  app.manage(sync_state);  // SyncState                          │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                              │                                          │
//! │                              ▼                                          │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                      SyncState                                  │   │
//! │  │                                                                 │   │
//! │  │  ┌─────────────────┐  ┌─────────────────────────────────────┐  │   │
//! │  │  │  SyncAgent      │  │  SyncStatus                        │  │   │
//! │  │  │  (Background    │  │                                     │  │   │
//! │  │  │   Task)         │  │  • connection_state (Connected/...)│  │   │
//! │  │  │                 │  │  • last_sync                       │  │   │
//! │  │  │  - WebSocket    │  │  • pending_count                   │  │   │
//! │  │  │  - Outbox       │  │  • mode (Auto/Primary/...)         │  │   │
//! │  │  │  - Inbound      │  │                                     │  │   │
//! │  │  └─────────────────┘  └─────────────────────────────────────┘  │   │
//! │  │                                                                 │   │
//! │  │  Emits events:                                                  │   │
//! │  │  • sync:status         (SyncStatus)                            │   │
//! │  │  • sync:progress       (pending, synced)                       │   │
//! │  │  • sync:error          (message, retryable)                    │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Event Flow
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────────┐
//! │  Frontend (SolidJS)                                                      │
//! │  ───────────────────                                                     │
//! │                                                                          │
//! │  import { listen } from '@tauri-apps/api/event';                         │
//! │                                                                          │
//! │  listen('sync:status', (event) => {                                      │
//! │    setSyncStatus(event.payload);                                         │
//! │  });                                                                     │
//! │                                                                          │
//! │  listen('sync:error', (event) => {                                       │
//! │    toast.error(event.payload.message);                                   │
//! │  });                                                                     │
//! └──────────────────────────────────────────────────────────────────────────┘
//! ```

use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Emitter};
use titan_sync::{
    ConnectionState, SyncAgentHandle, SyncConfig, SyncEventEmitter, SyncMode, SyncStatus,
};
use tokio::sync::broadcast;
use tracing::{debug, error, info};

/// Sync state managed by Tauri.
///
/// This wraps the sync agent and provides thread-safe access to sync status.
/// The sync agent runs as a background task, and this state allows commands
/// to query status and control the sync process.
#[derive(Clone)]
pub struct SyncState {
    /// Current sync status (thread-safe for reads)
    status: Arc<RwLock<SyncStatusDto>>,

    /// Handle to control the running sync agent
    agent_handle: Arc<RwLock<Option<SyncAgentHandle>>>,

    /// Current sync configuration
    config: Arc<RwLock<Option<SyncConfig>>>,

    /// App handle for emitting events to frontend
    app_handle: Arc<RwLock<Option<AppHandle>>>,

    /// Broadcast channel for hub to broadcast messages to connected secondaries.
    /// PRIMARY mode sets this when starting the hub server.
    hub_broadcast_tx: Arc<RwLock<Option<broadcast::Sender<String>>>>,
}

impl SyncState {
    /// Creates a new SyncState with default (offline) status.
    pub fn new() -> Self {
        Self {
            status: Arc::new(RwLock::new(SyncStatusDto::default())),
            agent_handle: Arc::new(RwLock::new(None)),
            config: Arc::new(RwLock::new(None)),
            app_handle: Arc::new(RwLock::new(None)),
            hub_broadcast_tx: Arc::new(RwLock::new(None)),
        }
    }

    /// Sets the app handle for emitting events.
    pub fn set_app_handle(&self, handle: AppHandle) {
        if let Ok(mut h) = self.app_handle.write() {
            *h = Some(handle);
        }
    }

    /// Gets the current sync status.
    pub fn get_status(&self) -> SyncStatusDto {
        self.status.read().map(|s| s.clone()).unwrap_or_default()
    }

    /// Updates the sync status and emits event to frontend.
    pub fn update_status(&self, status: SyncStatusDto) {
        debug!(
            connection_state = %status.connection_state,
            sync_mode = %status.sync_mode,
            "Updating sync status"
        );

        // Update internal state
        if let Ok(mut s) = self.status.write() {
            *s = status.clone();
        }

        // Emit event to frontend
        if let Ok(handle) = self.app_handle.read() {
            if let Some(ref app) = *handle {
                if let Err(e) = app.emit("sync:status", &status) {
                    error!(?e, "Failed to emit sync:status event");
                }
            }
        }
    }

    /// Checks if the sync agent is currently running.
    pub fn is_running(&self) -> bool {
        self.agent_handle
            .read()
            .map(|h| h.is_some())
            .unwrap_or(false)
    }

    /// Gets the current sync configuration.
    pub fn get_config(&self) -> Option<SyncConfig> {
        self.config.read().ok().and_then(|c| c.clone())
    }

    /// Sets the sync agent handle (called when agent starts).
    pub fn set_agent_handle(&self, handle: SyncAgentHandle) {
        if let Ok(mut h) = self.agent_handle.write() {
            *h = Some(handle);
        }
    }

    /// Sets the sync configuration.
    pub fn set_config(&self, config: SyncConfig) {
        if let Ok(mut c) = self.config.write() {
            *c = Some(config);
        }
    }

    /// Sets the hub broadcast channel (PRIMARY mode only).
    ///
    /// When running as PRIMARY, the hub server creates a broadcast channel
    /// for sending messages to connected secondaries. This method stores
    /// a reference to that channel so other parts of the app (like sale
    /// finalization) can broadcast inventory updates.
    pub fn set_hub_broadcast_tx(&self, tx: broadcast::Sender<String>) {
        if let Ok(mut t) = self.hub_broadcast_tx.write() {
            *t = Some(tx);
        }
    }

    /// Broadcasts an inventory update to connected secondaries (PRIMARY mode).
    ///
    /// This is called when PRIMARY makes a local sale and needs to notify
    /// connected SECONDARY devices about the stock change.
    ///
    /// # Arguments
    /// * `product_id` - The product whose stock changed
    /// * `sku` - The SKU for reference
    /// * `delta_qty` - The change in stock (negative for sales)
    ///
    /// # Returns
    /// `true` if broadcast was sent (PRIMARY mode), `false` otherwise.
    pub fn broadcast_inventory_update(&self, product_id: &str, sku: &str, delta_qty: i32) -> bool {
        if let Ok(tx_guard) = self.hub_broadcast_tx.read() {
            if let Some(ref tx) = *tx_guard {
                // Use the exact field names expected by SyncMessage::InventoryUpdate
                // which uses #[serde(rename_all = "camelCase")]
                let update = serde_json::json!({
                    "type": "InventoryUpdate",
                    "payload": {
                        "productId": product_id,
                        "sku": sku,
                        "deltaQuantity": delta_qty,
                        "sourceDeviceId": "primary",
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }
                });

                match tx.send(update.to_string()) {
                    Ok(receivers) => {
                        debug!(
                            product_id = %product_id,
                            delta_qty = delta_qty,
                            receivers = receivers,
                            "Broadcast InventoryUpdate to secondaries"
                        );
                        return true;
                    }
                    Err(_) => {
                        // No receivers - this is OK if no secondaries are connected
                        debug!("No secondaries connected to receive InventoryUpdate");
                        return false;
                    }
                }
            }
        }
        false
    }

    /// Sends an inventory delta to the hub (SECONDARY mode).
    ///
    /// This is called when SECONDARY makes a sale and needs to notify PRIMARY
    /// about the stock change so it can broadcast to other secondaries.
    ///
    /// # Arguments
    /// * `product_id` - The product whose stock changed
    /// * `sku` - The SKU for reference
    /// * `delta_qty` - The change in stock (negative for sales)
    ///
    /// # Returns
    /// `true` if delta was sent (SECONDARY mode with agent), `false` otherwise.
    pub async fn send_inventory_delta(&self, product_id: &str, sku: &str, delta_qty: i32) -> bool {
        // Clone the handle to avoid holding the lock across await
        let handle = {
            let guard = match self.agent_handle.read() {
                Ok(g) => g,
                Err(_) => return false,
            };
            guard.clone()
        };

        if let Some(ref h) = handle {
            match h.send_inventory_delta(product_id, sku, delta_qty).await {
                Ok(sent) => {
                    if sent {
                        debug!(
                            product_id = %product_id,
                            sku = %sku,
                            delta_qty = delta_qty,
                            "Sent InventoryDelta to hub"
                        );
                    }
                    return sent;
                }
                Err(e) => {
                    error!(?e, "Failed to send InventoryDelta");
                    return false;
                }
            }
        }
        false
    }

    /// Emits an inventory update event to the frontend.
    ///
    /// This should be called when SECONDARY receives an InventoryUpdate from PRIMARY
    /// so the frontend can refresh product displays.
    pub fn emit_inventory_update(&self, product_ids: Vec<String>, reason: &str) {
        if let Ok(handle) = self.app_handle.read() {
            if let Some(ref app) = *handle {
                #[derive(Clone, serde::Serialize)]
                #[serde(rename_all = "camelCase")]
                struct InventoryUpdateEvent {
                    product_ids: Vec<String>,
                    reason: String,
                    timestamp: String,
                }

                let event = InventoryUpdateEvent {
                    product_ids: product_ids.clone(),
                    reason: reason.to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                };

                if let Err(e) = app.emit("inventory:update", &event) {
                    error!(?e, "Failed to emit inventory:update event");
                } else {
                    debug!(
                        count = product_ids.len(),
                        reason = reason,
                        "Emitted inventory:update event from sync"
                    );
                }
            }
        }
    }

    /// Stops the sync agent.
    pub async fn stop_agent(&self) {
        let handle = { self.agent_handle.write().ok().and_then(|mut h| h.take()) };

        if let Some(h) = handle {
            info!("Stopping sync agent...");
            h.shutdown().await;
            info!("Sync agent stopped");
        }
    }
}

impl Default for SyncState {
    fn default() -> Self {
        Self::new()
    }
}

/// DTO for sync status that can be serialized to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatusDto {
    /// Current connection state
    pub connection_state: String,

    /// Current sync mode
    pub sync_mode: String,

    /// Last successful sync timestamp (ISO8601)
    pub last_sync_at: Option<String>,

    /// Number of pending outbox entries
    pub pending_outbox_count: i64,

    /// Whether sync is healthy (connected and no errors)
    pub is_healthy: bool,

    /// Last error message if any
    pub error_message: Option<String>,

    /// Hub URL if connected
    pub hub_url: Option<String>,
}

impl Default for SyncStatusDto {
    fn default() -> Self {
        Self {
            connection_state: "disconnected".to_string(),
            sync_mode: "primary".to_string(),
            last_sync_at: None,
            pending_outbox_count: 0,
            is_healthy: false,
            error_message: None,
            hub_url: None,
        }
    }
}

impl From<SyncStatus> for SyncStatusDto {
    fn from(status: SyncStatus) -> Self {
        let connection_state = match status.connection_state {
            ConnectionState::Disconnected => "disconnected",
            ConnectionState::Connecting => "connecting",
            ConnectionState::Connected => "connected",
            ConnectionState::Backoff => "backoff",
            ConnectionState::Reconnecting => "reconnecting",
        };

        let sync_mode = match status.mode {
            SyncMode::Auto => "auto",
            SyncMode::Primary => "primary",
            SyncMode::Secondary => "secondary",
            SyncMode::Offline => "offline",
        };

        Self {
            connection_state: connection_state.to_string(),
            sync_mode: sync_mode.to_string(),
            last_sync_at: status.last_sync,
            pending_outbox_count: status.pending_count,
            is_healthy: status.is_connected,
            error_message: status.last_error,
            hub_url: status.hub_url,
        }
    }
}

/// Tauri-based sync event emitter.
///
/// Implements the SyncEventEmitter trait from titan-sync to emit events
/// that the SolidJS frontend can listen to.
#[derive(Clone)]
pub struct TauriSyncEventEmitter {
    app_handle: AppHandle,
    sync_state: Arc<RwLock<SyncStatusDto>>,
}

impl TauriSyncEventEmitter {
    /// Creates a new TauriSyncEventEmitter.
    pub fn new(app_handle: AppHandle, sync_state: Arc<RwLock<SyncStatusDto>>) -> Self {
        Self {
            app_handle,
            sync_state,
        }
    }
}

impl SyncEventEmitter for TauriSyncEventEmitter {
    fn emit_status(&self, status: &SyncStatus) {
        let dto = SyncStatusDto::from(status.clone());

        // Update local state
        if let Ok(mut s) = self.sync_state.write() {
            *s = dto.clone();
        }

        // Emit to frontend
        if let Err(e) = self.app_handle.emit("sync:status", &dto) {
            error!(?e, "Failed to emit sync:status event");
        }

        debug!(?dto, "Emitted sync:status");
    }

    fn emit_progress(&self, pending: i64, synced: i64) {
        #[derive(Serialize, Clone)]
        struct ProgressEvent {
            pending: i64,
            synced: i64,
        }

        if let Err(e) = self
            .app_handle
            .emit("sync:progress", ProgressEvent { pending, synced })
        {
            error!(?e, "Failed to emit sync:progress event");
        }

        debug!(pending, synced, "Emitted sync:progress");
    }

    fn emit_error(&self, message: &str, retryable: bool) {
        #[derive(Serialize, Clone)]
        struct ErrorEvent {
            message: String,
            retryable: bool,
        }

        let event = ErrorEvent {
            message: message.to_string(),
            retryable,
        };

        if let Err(e) = self.app_handle.emit("sync:error", &event) {
            error!(?e, "Failed to emit sync:error event");
        }

        error!(message, retryable, "Emitted sync:error");
    }

    fn emit_inventory_update(&self, product_ids: Vec<String>, reason: &str) {
        #[derive(Serialize, Clone)]
        #[serde(rename_all = "camelCase")]
        struct InventoryUpdateEvent {
            product_ids: Vec<String>,
            reason: String,
            timestamp: String,
        }

        let event = InventoryUpdateEvent {
            product_ids: product_ids.clone(),
            reason: reason.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        if let Err(e) = self.app_handle.emit("inventory:update", &event) {
            error!(?e, "Failed to emit inventory:update event");
        }

        debug!(
            count = product_ids.len(),
            reason = reason,
            "Emitted inventory:update from sync agent"
        );
    }
}

/// Simple sync event emitter that calls a callback on connection state changes.
///
/// Used for AUTO mode where we need to update the main SyncState when
/// connection status changes.
pub struct SimpleSyncEmitter<F>
where
    F: Fn(bool) + Send + Sync,
{
    on_connected_change: F,
}

impl<F> SimpleSyncEmitter<F>
where
    F: Fn(bool) + Send + Sync,
{
    /// Creates a new SimpleSyncEmitter with the given callback.
    ///
    /// The callback receives `true` when connected, `false` otherwise.
    pub fn new(on_connected_change: F) -> Self {
        Self {
            on_connected_change,
        }
    }
}

impl<F> SyncEventEmitter for SimpleSyncEmitter<F>
where
    F: Fn(bool) + Send + Sync,
{
    fn emit_status(&self, status: &SyncStatus) {
        let connected = matches!(status.connection_state, ConnectionState::Connected);
        info!(connected, "SimpleSyncEmitter: connection state changed");
        (self.on_connected_change)(connected);
    }

    fn emit_progress(&self, _pending: i64, _synced: i64) {
        // No-op for simple emitter
    }

    fn emit_error(&self, message: &str, _retryable: bool) {
        error!(message, "SimpleSyncEmitter: error");
        (self.on_connected_change)(false);
    }

    fn emit_inventory_update(&self, product_ids: Vec<String>, reason: &str) {
        // No-op for simple emitter - it doesn't have an AppHandle to emit to
        debug!(
            count = product_ids.len(),
            reason = reason,
            "SimpleSyncEmitter: inventory update (no-op)"
        );
    }
}
