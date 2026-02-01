//! # Sync Configuration
//!
//! Configuration management for the sync engine.
//!
//! ## Configuration Sources
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Configuration Priority                               │
//! │                                                                         │
//! │  1. Environment Variables (highest priority)                           │
//! │     TITAN_SYNC_MODE=primary                                            │
//! │     TITAN_DEVICE_ID=abc-123                                            │
//! │                                                                         │
//! │  2. SQLite Database (sync_config table)                                │
//! │     Runtime overrides stored locally                                   │
//! │                                                                         │
//! │  3. TOML Config File                                                   │
//! │     ~/.config/titan-pos/sync.toml (Linux)                              │
//! │     ~/Library/Application Support/com.titan.pos/sync.toml (macOS)      │
//! │                                                                         │
//! │  4. Default Values (lowest priority)                                   │
//! │     SyncMode::Auto, auto-generated device_id                           │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Configuration File Format
//! ```toml
//! # sync.toml
//! [device]
//! id = "550e8400-e29b-41d4-a716-446655440000"
//! name = "Register 1"
//! priority = 50  # For leader election (higher = more likely to be PRIMARY)
//!
//! [sync]
//! mode = "auto"  # auto | primary | secondary
//! hub_url = "ws://192.168.1.100:8080/sync"
//! batch_size = 100
//! poll_interval_secs = 5
//!
//! [store]
//! id = "store-001"
//! name = "Downtown Branch"
//! ```

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::error::{SyncError, SyncResult};

// =============================================================================
// Sync Mode
// =============================================================================

/// The synchronization mode for this device.
///
/// ## Mode Selection
/// ```text
/// ┌─────────────────────────────────────────────────────────────────────────┐
/// │                        Sync Mode Behavior                               │
/// │                                                                         │
/// │  AUTO (Default)                                                        │
/// │  ──────────────                                                        │
/// │  • Participates in leader election                                     │
/// │  • Can become PRIMARY if elected                                       │
/// │  • Falls back to SECONDARY if another device is PRIMARY                │
/// │  • Best for most deployments                                           │
/// │                                                                         │
/// │  PRIMARY (Forced)                                                      │
/// │  ─────────────────                                                     │
/// │  • Acts as Store Hub regardless of election                            │
/// │  • Accepts connections from SECONDARY devices                          │
/// │  • Maintains store-level database                                      │
/// │  • Use for dedicated server machines                                   │
/// │                                                                         │
/// │  SECONDARY (Forced)                                                    │
/// │  ───────────────────                                                   │
/// │  • Never becomes PRIMARY                                               │
/// │  • Always connects to discovered/configured PRIMARY                    │
/// │  • Use for devices that should never be hub                            │
/// │                                                                         │
/// │  OFFLINE                                                               │
/// │  ───────                                                               │
/// │  • Sync disabled completely                                            │
/// │  • Local operations only                                               │
/// │  • Use for testing or isolated mode                                    │
/// └─────────────────────────────────────────────────────────────────────────┘
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    /// Automatic mode - participates in leader election.
    #[default]
    Auto,

    /// Force this device to be the PRIMARY (Store Hub).
    Primary,

    /// Force this device to be SECONDARY (never becomes hub).
    Secondary,

    /// Sync disabled - offline mode only.
    Offline,
}

impl SyncMode {
    /// Returns true if this mode allows the device to become PRIMARY.
    pub fn can_be_primary(&self) -> bool {
        matches!(self, SyncMode::Auto | SyncMode::Primary)
    }

    /// Returns true if sync is enabled at all.
    pub fn is_sync_enabled(&self) -> bool {
        !matches!(self, SyncMode::Offline)
    }
}

impl std::fmt::Display for SyncMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncMode::Auto => write!(f, "auto"),
            SyncMode::Primary => write!(f, "primary"),
            SyncMode::Secondary => write!(f, "secondary"),
            SyncMode::Offline => write!(f, "offline"),
        }
    }
}

impl std::str::FromStr for SyncMode {
    type Err = SyncError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(SyncMode::Auto),
            "primary" | "hub" | "server" => Ok(SyncMode::Primary),
            "secondary" | "client" => Ok(SyncMode::Secondary),
            "offline" | "disabled" => Ok(SyncMode::Offline),
            other => Err(SyncError::InvalidConfig(format!(
                "Unknown sync mode: '{}'. Valid options: auto, primary, secondary, offline",
                other
            ))),
        }
    }
}

// =============================================================================
// Device Configuration
// =============================================================================

/// Configuration for this device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// Unique device identifier (UUID v4).
    /// Auto-generated on first run if not provided.
    pub id: String,

    /// Human-readable device name (e.g., "Register 1", "Back Office").
    #[serde(default = "default_device_name")]
    pub name: String,

    /// Priority for leader election (0-100).
    /// Higher values make this device more likely to become PRIMARY.
    /// Default: 50
    #[serde(default = "default_priority")]
    pub priority: u8,
}

fn default_device_name() -> String {
    "POS Terminal".to_string()
}

fn default_priority() -> u8 {
    50
}

impl Default for DeviceConfig {
    fn default() -> Self {
        DeviceConfig {
            id: Uuid::new_v4().to_string(),
            name: default_device_name(),
            priority: default_priority(),
        }
    }
}

// =============================================================================
// Store Configuration
// =============================================================================

/// Configuration for the store this device belongs to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreConfig {
    /// Unique store identifier.
    pub id: String,

    /// Human-readable store name.
    #[serde(default)]
    pub name: String,
}

impl Default for StoreConfig {
    fn default() -> Self {
        StoreConfig {
            id: "default-store".to_string(),
            name: "Default Store".to_string(),
        }
    }
}

// =============================================================================
// Sync Settings
// =============================================================================

/// Sync behavior settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSettings {
    /// Sync mode for this device.
    #[serde(default)]
    pub mode: SyncMode,

    /// WebSocket URL of the Store Hub (if known).
    /// Can be discovered via mDNS/UDP in Auto mode.
    #[serde(default)]
    pub hub_url: Option<String>,

    /// Number of outbox entries to send per batch.
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,

    /// Interval between outbox poll cycles (seconds).
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,

    /// Connection timeout (seconds).
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_secs: u64,

    /// Maximum reconnection attempts before giving up.
    /// Set to 0 for infinite retries.
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Initial backoff duration (milliseconds) for reconnection.
    #[serde(default = "default_initial_backoff")]
    pub initial_backoff_ms: u64,

    /// Maximum backoff duration (seconds) for reconnection.
    #[serde(default = "default_max_backoff")]
    pub max_backoff_secs: u64,
}

fn default_batch_size() -> usize {
    100
}
fn default_poll_interval() -> u64 {
    5
}
fn default_connect_timeout() -> u64 {
    10
}
fn default_max_retries() -> u32 {
    0 // Infinite
}
fn default_initial_backoff() -> u64 {
    500
}
fn default_max_backoff() -> u64 {
    60
}

impl Default for SyncSettings {
    fn default() -> Self {
        SyncSettings {
            mode: SyncMode::default(),
            hub_url: None,
            batch_size: default_batch_size(),
            poll_interval_secs: default_poll_interval(),
            connect_timeout_secs: default_connect_timeout(),
            max_retries: default_max_retries(),
            initial_backoff_ms: default_initial_backoff(),
            max_backoff_secs: default_max_backoff(),
        }
    }
}

// =============================================================================
// Main Sync Configuration
// =============================================================================

/// Complete sync configuration.
///
/// ## Example Config File
/// ```toml
/// [device]
/// id = "550e8400-e29b-41d4-a716-446655440000"
/// name = "Register 1"
/// priority = 50
///
/// [store]
/// id = "store-downtown"
/// name = "Downtown Branch"
///
/// [sync]
/// mode = "auto"
/// batch_size = 100
/// poll_interval_secs = 5
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncConfig {
    /// Device-specific configuration.
    #[serde(default)]
    pub device: DeviceConfig,

    /// Store configuration.
    #[serde(default)]
    pub store: StoreConfig,

    /// Sync behavior settings.
    #[serde(default)]
    pub sync: SyncSettings,
}

impl SyncConfig {
    /// Creates a new config with defaults and a generated device ID.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads configuration from file, environment, and defaults.
    ///
    /// ## Load Order (later overrides earlier)
    /// 1. Default values
    /// 2. Config file (sync.toml)
    /// 3. Environment variables
    pub fn load(config_path: Option<PathBuf>) -> SyncResult<Self> {
        let mut config = Self::default();

        // Try to load from config file
        if let Some(path) = config_path.or_else(Self::default_config_path) {
            if path.exists() {
                info!(?path, "Loading sync config from file");
                let contents = std::fs::read_to_string(&path)?;
                config = toml::from_str(&contents)?;
            } else {
                debug!(?path, "Config file not found, using defaults");
            }
        }

        // Override with environment variables
        config.apply_env_overrides();

        // Validate the configuration
        config.validate()?;

        Ok(config)
    }

    /// Loads config or returns default if load fails.
    pub fn load_or_default(config_path: Option<PathBuf>) -> Self {
        Self::load(config_path).unwrap_or_else(|e| {
            warn!("Failed to load sync config: {}. Using defaults.", e);
            Self::default()
        })
    }

    /// Saves configuration to file.
    pub fn save(&self, config_path: Option<PathBuf>) -> SyncResult<()> {
        let path = config_path
            .or_else(Self::default_config_path)
            .ok_or_else(|| SyncError::ConfigSaveFailed("No config path available".into()))?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let contents = toml::to_string_pretty(self)?;
        std::fs::write(&path, contents)?;

        info!(?path, "Sync config saved");
        Ok(())
    }

    /// Validates the configuration.
    pub fn validate(&self) -> SyncResult<()> {
        // Device ID must be valid
        if self.device.id.is_empty() {
            return Err(SyncError::MissingDeviceId);
        }

        // If hub_url is set, validate it
        if let Some(ref url) = self.sync.hub_url {
            if !url.starts_with("ws://") && !url.starts_with("wss://") {
                return Err(SyncError::InvalidUrl(format!(
                    "Hub URL must start with ws:// or wss://, got: {}",
                    url
                )));
            }
        }

        // Validate batch size
        if self.sync.batch_size == 0 {
            return Err(SyncError::InvalidConfig(
                "batch_size must be greater than 0".into(),
            ));
        }

        Ok(())
    }

    /// Applies environment variable overrides.
    fn apply_env_overrides(&mut self) {
        // Device ID
        if let Ok(id) = std::env::var("TITAN_DEVICE_ID") {
            debug!(device_id = %id, "Overriding device ID from environment");
            self.device.id = id;
        }

        // Device name
        if let Ok(name) = std::env::var("TITAN_DEVICE_NAME") {
            self.device.name = name;
        }

        // Sync mode
        if let Ok(mode) = std::env::var("TITAN_SYNC_MODE") {
            if let Ok(parsed) = mode.parse() {
                debug!(mode = %mode, "Overriding sync mode from environment");
                self.sync.mode = parsed;
            }
        }

        // Hub URL
        if let Ok(url) = std::env::var("TITAN_HUB_URL") {
            debug!(url = %url, "Overriding hub URL from environment");
            self.sync.hub_url = Some(url);
        }

        // Store ID
        if let Ok(id) = std::env::var("TITAN_STORE_ID") {
            self.store.id = id;
        }
    }

    /// Returns the default config file path.
    fn default_config_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "titan", "pos").map(|dirs| {
            let config_dir = dirs.config_dir();
            config_dir.join("sync.toml")
        })
    }

    // =========================================================================
    // Convenience Methods
    // =========================================================================

    /// Returns the device ID.
    pub fn device_id(&self) -> &str {
        &self.device.id
    }

    /// Returns the store ID.
    pub fn store_id(&self) -> &str {
        &self.store.id
    }

    /// Returns the sync mode.
    pub fn mode(&self) -> SyncMode {
        self.sync.mode
    }

    /// Returns true if sync is enabled.
    pub fn is_sync_enabled(&self) -> bool {
        self.sync.mode.is_sync_enabled()
    }

    /// Returns the hub URL if configured.
    pub fn hub_url(&self) -> Option<&str> {
        self.sync.hub_url.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_mode_parsing() {
        assert_eq!("auto".parse::<SyncMode>().unwrap(), SyncMode::Auto);
        assert_eq!("primary".parse::<SyncMode>().unwrap(), SyncMode::Primary);
        assert_eq!("hub".parse::<SyncMode>().unwrap(), SyncMode::Primary);
        assert_eq!("secondary".parse::<SyncMode>().unwrap(), SyncMode::Secondary);
        assert_eq!("offline".parse::<SyncMode>().unwrap(), SyncMode::Offline);
        assert!("invalid".parse::<SyncMode>().is_err());
    }

    #[test]
    fn test_default_config() {
        let config = SyncConfig::default();
        assert!(!config.device.id.is_empty()); // Auto-generated
        assert_eq!(config.sync.mode, SyncMode::Auto);
        assert_eq!(config.sync.batch_size, 100);
    }

    #[test]
    fn test_config_validation() {
        let mut config = SyncConfig::default();
        assert!(config.validate().is_ok());

        // Empty device ID should fail
        config.device.id = String::new();
        assert!(config.validate().is_err());

        // Invalid URL should fail
        config.device.id = "test".to_string();
        config.sync.hub_url = Some("http://invalid".to_string());
        assert!(config.validate().is_err());

        // Valid WebSocket URL should pass
        config.sync.hub_url = Some("ws://localhost:8080".to_string());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_mode_can_be_primary() {
        assert!(SyncMode::Auto.can_be_primary());
        assert!(SyncMode::Primary.can_be_primary());
        assert!(!SyncMode::Secondary.can_be_primary());
        assert!(!SyncMode::Offline.can_be_primary());
    }

    #[test]
    fn test_toml_serialization() {
        let config = SyncConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("[device]"));
        assert!(toml_str.contains("[sync]"));
    }
}
