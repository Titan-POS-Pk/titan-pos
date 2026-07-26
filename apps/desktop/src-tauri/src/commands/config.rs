//! # Config Commands
//!
//! Tauri commands for retrieving application configuration.
//!
//! ## Command Overview
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                        Config Commands                                  │
//! │                                                                         │
//! │  get_config()      - Returns store/app configuration                   │
//! │  get_device_info() - Returns device ID, name, and sync mode            │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::debug;

use crate::state::{ConfigState, SyncState};

/// Gets the current application configuration.
///
/// ## When Used
/// - App startup (to configure UI)
/// - Receipt printing (store name, address)
/// - Currency formatting
///
/// ## Returns
/// Complete configuration state (read-only)
#[tauri::command]
pub fn get_config(config: State<'_, ConfigState>) -> ConfigState {
    debug!("get_config command");
    (*config).clone()
}

/// Device information DTO.
///
/// Contains information about this specific POS terminal/device
/// for display in the UI header.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfoDto {
    /// Device UUID (from TITAN_DEVICE_ID or generated)
    pub device_id: String,

    /// Human-readable device name (from TITAN_DEVICE_NAME or derived)
    pub device_name: String,

    /// Current sync mode (auto, primary, secondary, offline)
    pub sync_mode: String,

    /// Store ID this device belongs to
    pub store_id: String,

    /// Whether sync is configured and running
    pub sync_enabled: bool,
}

/// Gets device information for UI display.
///
/// ## User Workflow
/// ```text
/// ┌─────────────────────────────────────────────────────────────────────────┐
/// │  Header Display                                                         │
/// │  ┌─────────────────────────────────────────────────────────────────┐   │
/// │  │ 🏪 Store Name    📱 device-pos-1 (PRIMARY)      12:34 PM   ⚙️  │   │
/// │  └─────────────────────────────────────────────────────────────────┘   │
/// │                              ▲                                          │
/// │                              │                                          │
/// │                    THIS COMMAND provides:                              │
/// │                    - device_id: "device-pos-1"                         │
/// │                    - sync_mode: "primary"                              │
/// └─────────────────────────────────────────────────────────────────────────┘
/// ```
///
/// ## Returns
/// `DeviceInfoDto` with device identification and sync status.
#[tauri::command]
pub fn get_device_info(sync: State<'_, SyncState>) -> DeviceInfoDto {
    debug!("get_device_info command");

    // Try to get device info from sync config first
    if let Some(config) = sync.get_config() {
        let sync_mode = match config.sync.mode {
            titan_sync::SyncMode::Auto => "auto",
            titan_sync::SyncMode::Primary => "primary",
            titan_sync::SyncMode::Secondary => "secondary",
            titan_sync::SyncMode::Offline => "offline",
        };

        return DeviceInfoDto {
            device_id: config.device.id.clone(),
            device_name: config.device.name.clone(),
            sync_mode: sync_mode.to_string(),
            store_id: config.store.id.clone(),
            sync_enabled: sync.is_running(),
        };
    }

    // Fall back to environment variables or defaults
    let device_id = std::env::var("TITAN_DEVICE_ID")
        .unwrap_or_else(|_| "local-dev".to_string());
    
    let device_name = std::env::var("TITAN_DEVICE_NAME")
        .unwrap_or_else(|_| format!("POS-{}", &device_id[..device_id.len().min(8)]));
    
    let sync_mode = std::env::var("TITAN_SYNC_MODE")
        .unwrap_or_else(|_| "primary".to_string());
    
    let store_id = std::env::var("TITAN_STORE_ID")
        .unwrap_or_else(|_| "default".to_string());

    DeviceInfoDto {
        device_id,
        device_name,
        sync_mode,
        store_id,
        sync_enabled: false,
    }
}
