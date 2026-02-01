//! # Sync Commands
//!
//! Tauri commands for managing sync functionality.
//!
//! ## Command Overview
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                        Sync Commands                                    │
//! │                                                                         │
//! │  get_sync_status()   - Returns current sync status                     │
//! │  get_sync_config()   - Returns current sync configuration              │
//! │  set_sync_mode()     - Changes the sync mode                           │
//! │  get_pending_sync()  - Returns pending outbox count                    │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::ApiError;
use crate::state::{SyncState, SyncStatusDto};

/// Gets the current sync status.
///
/// # Returns
/// `SyncStatusDto` containing connection state, mode, pending count, etc.
#[tauri::command]
pub async fn get_sync_status(
    sync: State<'_, SyncState>,
) -> Result<SyncStatusDto, ApiError> {
    Ok(sync.get_status())
}

/// Response DTO for sync configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConfigDto {
    /// Device UUID
    pub device_id: String,

    /// Human-readable device name
    pub device_name: String,

    /// Store ID this device belongs to
    pub store_id: String,

    /// Store name
    pub store_name: String,

    /// Current sync mode
    pub sync_mode: String,

    /// Whether the sync agent is running
    pub is_running: bool,
}

/// Gets the current sync configuration.
///
/// # Returns
/// `SyncConfigDto` containing the current sync configuration.
#[tauri::command]
pub async fn get_sync_config(
    sync: State<'_, SyncState>,
) -> Result<SyncConfigDto, ApiError> {
    let config = sync.get_config();
    let is_running = sync.is_running();

    match config {
        Some(cfg) => {
            let sync_mode = match cfg.sync.mode {
                titan_sync::SyncMode::Auto => "auto",
                titan_sync::SyncMode::Primary => "primary",
                titan_sync::SyncMode::Secondary => "secondary",
                titan_sync::SyncMode::Offline => "offline",
            };

            Ok(SyncConfigDto {
                device_id: cfg.device.id.clone(),
                device_name: cfg.device.name.clone(),
                store_id: cfg.store.id.clone(),
                store_name: cfg.store.name.clone(),
                sync_mode: sync_mode.to_string(),
                is_running,
            })
        }
        None => {
            // Return default config when not configured
            Ok(SyncConfigDto {
                device_id: "unconfigured".to_string(),
                device_name: "Unconfigured Device".to_string(),
                store_id: "".to_string(),
                store_name: "".to_string(),
                sync_mode: "offline".to_string(),
                is_running: false,
            })
        }
    }
}

/// Sets the sync mode.
///
/// # Arguments
/// * `mode` - New sync mode: "auto", "primary", "secondary", or "offline"
///
/// # Returns
/// Updated `SyncStatusDto` reflecting the new mode.
#[tauri::command]
pub async fn set_sync_mode(
    sync: State<'_, SyncState>,
    mode: String,
) -> Result<SyncStatusDto, ApiError> {
    let _sync_mode = match mode.as_str() {
        "auto" => titan_sync::SyncMode::Auto,
        "primary" => titan_sync::SyncMode::Primary,
        "secondary" => titan_sync::SyncMode::Secondary,
        "offline" => titan_sync::SyncMode::Offline,
        _ => {
            return Err(ApiError::validation(format!(
                "Invalid sync mode: {}. Must be 'auto', 'primary', 'secondary', or 'offline'",
                mode
            )));
        }
    };

    // TODO: Implement mode change when SyncAgent supports runtime mode changes
    // For now, this just validates the mode and returns current status
    tracing::info!(mode = %mode, "Sync mode change requested (not yet implemented)");

    Ok(sync.get_status())
}

/// Gets the pending outbox count.
///
/// # Returns
/// Number of pending outbox entries.
#[tauri::command]
pub async fn get_pending_sync_count(
    sync: State<'_, SyncState>,
) -> Result<i64, ApiError> {
    Ok(sync.get_status().pending_outbox_count)
}
