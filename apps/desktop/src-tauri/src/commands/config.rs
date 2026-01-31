//! # Config Commands
//!
//! Tauri commands for retrieving application configuration.

use tauri::State;
use tracing::debug;

use crate::state::ConfigState;

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
