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
use tracing::{debug, error, info};

/// Sync state managed by Tauri.
///
/// This wraps the sync agent and provides thread-safe access to sync status.
/// The sync agent runs as a background task, and this state allows commands
/// to query status and control the sync process.
pub struct SyncState {
    /// Current sync status (thread-safe for reads)
    status: Arc<RwLock<SyncStatusDto>>,

    /// Handle to control the running sync agent
    agent_handle: Arc<RwLock<Option<SyncAgentHandle>>>,

    /// Current sync configuration
    config: Arc<RwLock<Option<SyncConfig>>>,
}

impl SyncState {
    /// Creates a new SyncState with default (offline) status.
    pub fn new() -> Self {
        Self {
            status: Arc::new(RwLock::new(SyncStatusDto::default())),
            agent_handle: Arc::new(RwLock::new(None)),
            config: Arc::new(RwLock::new(None)),
        }
    }

    /// Gets the current sync status.
    pub fn get_status(&self) -> SyncStatusDto {
        self.status
            .read()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// Updates the sync status.
    pub fn update_status(&self, status: SyncStatusDto) {
        if let Ok(mut s) = self.status.write() {
            *s = status;
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
        self.config
            .read()
            .ok()
            .and_then(|c| c.clone())
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

    /// Stops the sync agent.
    pub async fn stop_agent(&self) {
        let handle = {
            self.agent_handle
                .write()
                .ok()
                .and_then(|mut h| h.take())
        };

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
            sync_mode: "offline".to_string(),
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
            ConnectionState::Backoff { .. } => "backoff",
            ConnectionState::Reconnecting { .. } => "reconnecting",
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

        if let Err(e) = self.app_handle.emit("sync:progress", ProgressEvent { pending, synced }) {
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
}
