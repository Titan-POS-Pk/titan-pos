//! # Titan Desktop Library
//!
//! Core library for the Titan POS desktop application.
//! This is the main entry point that configures and runs the Tauri app.
//!
//! ## Module Organization
//! ```text
//! titan_desktop_lib/
//! ├── lib.rs          ◄─── You are here (Tauri setup & run)
//! ├── state/
//! │   ├── mod.rs      ◄─── State type exports
//! │   ├── db.rs       ◄─── Database state wrapper
//! │   ├── cart.rs     ◄─── Cart state management
//! │   ├── config.rs   ◄─── Configuration state
//! │   └── sync.rs     ◄─── Sync agent state
//! ├── commands/
//! │   ├── mod.rs      ◄─── Command exports
//! │   ├── product.rs  ◄─── Product search/CRUD commands
//! │   ├── sale.rs     ◄─── Sale/transaction commands
//! │   ├── cart.rs     ◄─── Cart manipulation commands
//! │   └── sync.rs     ◄─── Sync status/control commands
//! └── error.rs        ◄─── API error type for commands
//! ```
//!
//! ## State Management (Option B: Multiple State Types)
//! Instead of a single `AppState` struct, we use multiple focused state types:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Tauri State Management                               │
//! │                                                                         │
//! │  Option B: Multiple State Types (CHOSEN)                               │
//! │  ─────────────────────────────────────────                             │
//! │                                                                         │
//! │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐      │
//! │  │   DbState   │ │  CartState  │ │ ConfigState │ │  SyncState  │      │
//! │  │             │ │             │ │             │ │             │      │
//! │  │ • Database  │ │ • Cart      │ │ • Tenant ID │ │ • SyncAgent │      │
//! │  │   pool      │ │   items     │ │ • Tax rates │ │ • Status    │      │
//! │  │ • Repos     │ │ • Totals    │ │ • Store     │ │ • Events    │      │
//! │  └─────────────┘ └─────────────┘ └─────────────┘ └─────────────┘      │
//! │                                                                         │
//! │  WHY: Each command only requests the state it needs.                   │
//! │       Better separation of concerns and testability.                   │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

pub mod commands;
pub mod error;
pub mod state;

use directories::ProjectDirs;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;
use tracing::{debug, info, warn, error, Level};
use tracing_subscriber::EnvFilter;

use state::{CartState, ConfigState, DbState, SimpleSyncEmitter, SyncState, TauriSyncEventEmitter, SyncStatusDto};
use titan_db::{Database, DbConfig};
use titan_sync::{SyncConfig, SyncMode, SyncAgent};

/// Runs the Tauri application.
///
/// ## Startup Sequence
/// ```text
/// ┌─────────────────────────────────────────────────────────────────────────┐
/// │                       Application Startup                               │
/// │                                                                         │
/// │  1. Initialize Logging ───────────────────────────────────────────────► │
/// │     • tracing-subscriber with env filter                                │
/// │     • Default: INFO, can be overridden with RUST_LOG                    │
/// │                                                                         │
/// │  2. Determine Database Path ──────────────────────────────────────────► │
/// │     • macOS: ~/Library/Application Support/com.titan.pos/titan.db       │
/// │     • Windows: %APPDATA%/titan/pos/titan.db                             │
/// │     • Linux: ~/.local/share/titan-pos/titan.db                          │
/// │                                                                         │
/// │  3. Connect to Database ──────────────────────────────────────────────► │
/// │     • SQLite with WAL mode                                              │
/// │     • Run pending migrations                                            │
/// │                                                                         │
/// │  4. Initialize State Objects ─────────────────────────────────────────► │
/// │     • DbState: Wraps Database connection                                │
/// │     • CartState: Empty cart with Mutex for thread-safe updates          │
/// │     • ConfigState: Default configuration                                │
/// │                                                                         │
/// │  5. Build & Run Tauri App ────────────────────────────────────────────► │
/// │     • Register all commands                                             │
/// │     • Manage state                                                      │
/// │     • Launch window                                                     │
/// └─────────────────────────────────────────────────────────────────────────┘
/// ```
pub fn run() {
    // Load .env file if present (for development configuration)
    // This allows setting TITAN_SYNC_MODE, TITAN_DEVICE_ID, etc. in a file
    // rather than on the command line for each invocation.
    if let Err(e) = dotenvy::dotenv() {
        // Not an error - .env file is optional
        eprintln!("Note: No .env file loaded ({e}). Using environment variables or defaults.");
    }

    // Initialize tracing (logging)
    init_tracing();

    info!("Starting Titan POS Desktop Application");

    // Build and run the Tauri app
    tauri::Builder::default()
        // Setup hook runs before the app starts
        .setup(|app| {
            // Determine database path
            let db_path = get_database_path(app)?;
            info!(?db_path, "Database path determined");

            // Initialize database (blocking in setup, async in runtime)
            let db = tauri::async_runtime::block_on(async {
                let config = DbConfig::new(db_path);
                Database::new(config).await
            })?;

            info!("Database connected and migrations applied");

            // Initialize state objects
            let db_state = DbState::new(db.clone());
            let cart_state = CartState::new();
            let config_state = ConfigState::default();
            let sync_state = SyncState::new();
            
            // Set app handle for event emission
            sync_state.set_app_handle(app.handle().clone());

            // Register state with Tauri
            app.manage(db_state);
            app.manage(cart_state);
            app.manage(config_state);
            app.manage(sync_state.clone());

            // Start sync agent if configured via environment variables
            let app_handle = app.handle().clone();
            let sync_state_clone = sync_state.clone();
            let db_arc = Arc::new(db);
            
            tauri::async_runtime::spawn(async move {
                start_sync_agent_if_configured(app_handle, sync_state_clone, db_arc).await;
            });

            info!("State initialized, sync agent startup scheduled");
            Ok(())
        })
        // Register all commands
        .invoke_handler(tauri::generate_handler![
            // Product commands
            commands::product::search_products,
            commands::product::get_product_by_id,
            commands::product::get_product_by_sku,
            // Cart commands
            commands::cart::get_cart,
            commands::cart::add_to_cart,
            commands::cart::update_cart_item,
            commands::cart::remove_from_cart,
            commands::cart::clear_cart,
            // Sale commands
            commands::sale::create_sale,
            commands::sale::add_payment,
            commands::sale::finalize_sale,
            // Config commands
            commands::config::get_config,
            commands::config::get_device_info,
            // Sync commands
            commands::sync::get_sync_status,
            commands::sync::get_sync_config,
            commands::sync::set_sync_mode,
            commands::sync::get_pending_sync_count,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Initializes the tracing subscriber for structured logging.
///
/// ## Log Levels
/// - `RUST_LOG=debug` - Show debug messages
/// - `RUST_LOG=titan=trace` - Show trace for titan crates only
/// - Default: INFO level
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,titan=debug,sqlx=warn"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_max_level(Level::TRACE)
        .init();
}

/// Determines the database file path based on the platform.
///
/// ## Development Mode
/// In development, the app looks for a seeded database in `data/titan.db`
/// relative to the project root. This allows using the same database
/// seeded by `cargo run -p titan-db --bin seed`.
///
/// ## Platform-Specific Paths (Production)
/// - **macOS**: `~/Library/Application Support/com.titan.pos/titan.db`
/// - **Windows**: `%APPDATA%\titan\pos\titan.db`
/// - **Linux**: `~/.local/share/titan-pos/titan.db`
///
/// ## Environment Override
/// Set `TITAN_DB_PATH` environment variable to use a custom path.
///
/// ## Multi-Device Testing (Same Machine)
/// When testing multiple devices on the same machine, each device gets its own database:
/// - `TITAN_DEVICE_ID="pos-alpha"` → `data/titan-pos-alpha.db`
/// - `TITAN_DEVICE_ID="pos-beta"` → `data/titan-pos-beta.db`
/// This enables realistic sync testing where each device has truly separate data.
///
/// ## Development Workflow
/// ```bash
/// # 1. Seed the base database
/// cargo run -p titan-db --bin seed
///
/// # 2. Copy to device-specific databases for multi-device testing
/// cp data/titan.db data/titan-pos-alpha.db
/// cp data/titan.db data/titan-pos-beta.db
///
/// # 3. Run first device
/// TITAN_DEVICE_ID="pos-alpha" TITAN_SYNC_MODE="auto" TITAN_DEVICE_PRIORITY="80" pnpm tauri dev
///
/// # 4. Run second device (different terminal)
/// TITAN_DEVICE_ID="pos-beta" TITAN_SYNC_MODE="auto" TITAN_DEVICE_PRIORITY="50" VITE_PORT=5174 pnpm tauri dev
/// ```
fn get_database_path(_app: &tauri::App) -> Result<PathBuf, Box<dyn std::error::Error>> {
    // Check for explicit override first
    if let Ok(path) = std::env::var("TITAN_DB_PATH") {
        info!(path = %path, "Using TITAN_DB_PATH override");
        return Ok(PathBuf::from(path));
    }

    // In development, look for the seeded database in data/titan.db
    // Note: Tauri runs the binary from target/debug, so relative paths won't work
    // We use CARGO_MANIFEST_DIR at compile time to find the project root
    #[cfg(debug_assertions)]
    {
        // Check if running multi-device test (TITAN_DEVICE_ID set)
        // Each device gets its own database file for realistic sync testing
        let device_id = std::env::var("TITAN_DEVICE_ID").ok();
        
        // Base path from CARGO_MANIFEST_DIR
        let data_dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../data"));
        
        if let Some(ref id) = device_id {
            // Use device-specific database: data/titan-{device_id}.db
            let device_db = data_dir.join(format!("titan-{}.db", id));
            
            if device_db.exists() {
                let canonical = device_db.canonicalize()?;
                info!(?canonical, device_id = %id, "Using device-specific database");
                return Ok(canonical);
            }
            
            // Device DB doesn't exist - copy from base database
            let base_db = data_dir.join("titan.db");
            if base_db.exists() {
                info!(
                    device_id = %id,
                    src = ?base_db,
                    dst = ?device_db,
                    "Copying base database to device-specific file"
                );
                std::fs::copy(&base_db, &device_db)?;
                let canonical = device_db.canonicalize()?;
                return Ok(canonical);
            }
        }
        
        // Paths to try, in order of preference:
        // 1. Relative to CARGO_MANIFEST_DIR (set at compile time for src-tauri)
        // 2. Standard project root locations
        let paths_to_try = [
            // From apps/desktop/src-tauri, go up to project root
            data_dir.join("titan.db"),
            // From project root (if running cargo run directly)
            PathBuf::from("./data/titan.db"),
            // From apps/desktop directory
            PathBuf::from("../../data/titan.db"),
        ];

        for path in &paths_to_try {
            if path.exists() {
                let canonical = path.canonicalize()?;
                info!(?canonical, "Using development database");
                return Ok(canonical);
            }
        }

        info!("No development database found, using platform-specific path");
    }

    // Use platform-specific app data directory (production)
    let proj_dirs =
        ProjectDirs::from("com", "titan", "pos").ok_or("Could not determine app data directory")?;

    let data_dir = proj_dirs.data_dir();

    // Create directory if it doesn't exist
    std::fs::create_dir_all(data_dir)?;

    Ok(data_dir.join("titan.db"))
}
/// Starts the sync agent based on environment variables.
///
/// ## Environment Variables
/// - `TITAN_SYNC_MODE`: primary | secondary | auto | offline (default: primary)
/// - `TITAN_DEVICE_ID`: Device identifier (auto-generated if not set)
/// - `TITAN_HUB_PORT`: Hub port for PRIMARY mode (default: 8765)
/// - `TITAN_HUB_URL`: Hub URL for SECONDARY mode (required for secondary)
///
/// ## Mode Behavior
/// - **primary**: Starts WebSocket hub server, accepts connections from secondaries (DEFAULT)
/// - **secondary**: Connects to hub URL as client
/// - **auto**: Priority-based election (high priority becomes PRIMARY, low connects as SECONDARY)
/// - **offline**: No sync - purely local operations
async fn start_sync_agent_if_configured(
    app_handle: tauri::AppHandle,
    sync_state: SyncState,
    db: Arc<Database>,
) {
    // Read configuration from environment
    // Default to "primary" for demo-friendly behavior (single instance works with sync ready)
    let sync_mode = std::env::var("TITAN_SYNC_MODE")
        .unwrap_or_else(|_| "primary".to_string())
        .to_lowercase();

    let device_id = std::env::var("TITAN_DEVICE_ID")
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());

    let hub_port: u16 = std::env::var("TITAN_HUB_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8765);

    let hub_url = std::env::var("TITAN_HUB_URL").ok();

    info!(
        sync_mode = %sync_mode,
        device_id = %device_id,
        hub_port = hub_port,
        hub_url = ?hub_url,
        "Sync configuration loaded"
    );

    match sync_mode.as_str() {
        "primary" => {
            info!("Starting as PRIMARY - launching hub server on port {}", hub_port);
            
            // Create sync config for primary mode
            let mut config = SyncConfig::load_or_default(None);
            config.device.id = device_id.clone();
            config.sync.mode = SyncMode::Primary;
            config.hub.port = hub_port;

            sync_state.set_config(config.clone());

            // Create broadcast channel for hub to broadcast to connected secondaries
            // This channel is shared with SyncState so sale commands can broadcast
            let (hub_broadcast_tx, _) = tokio::sync::broadcast::channel::<String>(256);
            
            // Store broadcast channel in SyncState so sale finalization can use it
            sync_state.set_hub_broadcast_tx(hub_broadcast_tx.clone());

            // Spawn the hub server - pass db and device_id for protocol handling
            let db_for_hub = db.clone();
            let device_id_for_hub = device_id.clone();
            tauri::async_runtime::spawn(async move {
                match start_primary_hub(hub_port, db_for_hub, device_id_for_hub, hub_broadcast_tx).await {
                    Ok(()) => info!("Primary hub server running"),
                    Err(e) => error!(?e, "Failed to start primary hub"),
                }
            });

            // Update sync state to reflect PRIMARY status
            let status = state::SyncStatusDto {
                connection_state: "listening".to_string(),
                sync_mode: "primary".to_string(),
                is_healthy: true,
                hub_url: Some(format!("ws://0.0.0.0:{}/ws", hub_port)),
                ..Default::default()
            };
            sync_state.update_status(status);
        }

        "secondary" => {
            if let Some(url) = hub_url {
                info!("Starting as SECONDARY - connecting to hub at {}", url);
                
                // Create sync config for secondary mode
                let mut config = SyncConfig::load_or_default(None);
                config.device.id = device_id.clone();
                config.sync.mode = SyncMode::Secondary;
                config.sync.hub_url = Some(url.clone());

                sync_state.set_config(config.clone());

                // Create event emitter using std::sync::RwLock (not tokio)
                let status_state = Arc::new(std::sync::RwLock::new(SyncStatusDto::default()));
                let emitter = Arc::new(TauriSyncEventEmitter::new(
                    app_handle.clone(),
                    status_state.clone(),
                ));

                // Create and start sync agent (db is already Arc<Database>)
                let mut agent = SyncAgent::with_emitter(config, db.clone(), emitter);
                
                // Clone sync_state to move into the spawned task
                let sync_state_for_agent = sync_state.clone();
                
                tauri::async_runtime::spawn(async move {
                    match agent.start().await {
                        Ok(handle) => {
                            info!("Sync agent started successfully");
                            // CRITICAL: Store the handle to keep the agent alive!
                            // The agent will shut down if this handle is dropped.
                            sync_state_for_agent.set_agent_handle(handle);
                        }
                        Err(e) => {
                            error!(?e, "Failed to start sync agent");
                        }
                    }
                });

                // Update sync state
                let status = state::SyncStatusDto {
                    connection_state: "connecting".to_string(),
                    sync_mode: "secondary".to_string(),
                    is_healthy: false,
                    hub_url: Some(url),
                    ..Default::default()
                };
                sync_state.update_status(status);
            } else {
                warn!("SECONDARY mode requires TITAN_HUB_URL to be set");
                let status = state::SyncStatusDto {
                    connection_state: "error".to_string(),
                    sync_mode: "secondary".to_string(),
                    is_healthy: false,
                    error_message: Some("TITAN_HUB_URL not set".to_string()),
                    ..Default::default()
                };
                sync_state.update_status(status);
            }
        }

        "auto" => {
            // Read device priority for election (higher = more likely to become PRIMARY)
            let device_priority: u8 = std::env::var("TITAN_DEVICE_PRIORITY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(50);

            info!(
                device_priority = device_priority,
                "AUTO mode - starting priority-based election"
            );

            // Set initial discovering status
            let status = state::SyncStatusDto {
                connection_state: "discovering".to_string(),
                sync_mode: "auto".to_string(),
                is_healthy: false,
                ..Default::default()
            };
            sync_state.update_status(status);

            // AUTO mode election:
            // 1. First, try to connect to existing hub
            // 2. If no hub found after retries, become PRIMARY ourselves
            // 3. Higher priority devices wait shorter before becoming PRIMARY
            let sync_state_clone = sync_state.clone();
            let device_id_clone = device_id.clone();
            let db_clone = db.clone();

            tauri::async_runtime::spawn(async move {
                // First, check if a hub already exists by trying to connect
                let hub_url = format!("ws://localhost:{}/sync", hub_port);
                
                info!(hub_url = %hub_url, "AUTO mode - checking for existing hub");
                
                // Try to connect with a few quick retries
                let mut hub_found = false;
                for attempt in 0..3 {
                    match tokio::time::timeout(
                        tokio::time::Duration::from_millis(500),
                        tokio::net::TcpStream::connect(format!("localhost:{}", hub_port))
                    ).await {
                        Ok(Ok(_)) => {
                            info!(attempt = attempt, "Found existing hub");
                            hub_found = true;
                            break;
                        }
                        Ok(Err(_)) | Err(_) => {
                            // Connection refused or timeout - no hub yet
                            debug!(attempt = attempt, "No hub found, retrying...");
                            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                        }
                    }
                }
                
                if hub_found {
                    // Hub exists - connect as SECONDARY
                    info!("Existing hub found - connecting as SECONDARY");
                    
                    let status = state::SyncStatusDto {
                        connection_state: "connecting".to_string(),
                        sync_mode: "auto".to_string(),
                        is_healthy: false,
                        hub_url: Some(hub_url.clone()),
                        ..Default::default()
                    };
                    sync_state_clone.update_status(status);
                    
                    // Start sync agent in secondary mode
                    start_secondary_agent(
                        hub_url,
                        device_id_clone,
                        db_clone,
                        sync_state_clone
                    ).await;
                } else {
                    // No hub found - wait based on priority, then become PRIMARY
                    // Higher priority = shorter wait (so they win the race)
                    let election_delay_ms = 500 + ((100 - device_priority) as u64 * 15);
                    info!(
                        election_delay_ms = election_delay_ms,
                        device_priority = device_priority,
                        "No hub found - waiting before becoming PRIMARY"
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(election_delay_ms)).await;
                    
                    // Check one more time if a hub appeared during our wait
                    let hub_appeared = tokio::net::TcpStream::connect(format!("localhost:{}", hub_port))
                        .await
                        .is_ok();
                    
                    if hub_appeared {
                        info!("Hub appeared during election wait - connecting as SECONDARY");
                        start_secondary_agent(
                            hub_url,
                            device_id_clone,
                            db_clone,
                            sync_state_clone
                        ).await;
                    } else {
                        // Still no hub - become PRIMARY
                        info!("No hub detected - becoming PRIMARY");
                        
                        let status = state::SyncStatusDto {
                            connection_state: "listening".to_string(),
                            sync_mode: "auto".to_string(),
                            is_healthy: true,
                            hub_url: Some(format!("ws://0.0.0.0:{}/sync", hub_port)),
                            ..Default::default()
                        };
                        sync_state_clone.update_status(status);
                        
                        // Create broadcast channel for AUTO-elected PRIMARY
                        let (hub_broadcast_tx, _) = tokio::sync::broadcast::channel::<String>(256);
                        sync_state_clone.set_hub_broadcast_tx(hub_broadcast_tx.clone());
                        
                        match start_primary_hub(hub_port, db_clone, device_id_clone, hub_broadcast_tx).await {
                            Ok(()) => {
                                info!("Auto-elected PRIMARY hub stopped");
                            }
                            Err(e) => {
                                error!(?e, "Failed to start auto-elected PRIMARY hub");
                                let status = state::SyncStatusDto {
                                    connection_state: "error".to_string(),
                                    sync_mode: "auto".to_string(),
                                    is_healthy: false,
                                    error_message: Some(format!("Failed to start hub: {:?}", e)),
                                    ..Default::default()
                                };
                                sync_state_clone.update_status(status);
                            }
                        }
                    }
                }
            });
        }

        "offline" | _ => {
            info!("Sync disabled (offline mode)");
            let status = state::SyncStatusDto {
                connection_state: "offline".to_string(),
                sync_mode: "offline".to_string(),
                is_healthy: true, // Offline is healthy - it's intentional
                ..Default::default()
            };
            sync_state.update_status(status);
        }
    }
}

/// Helper function to start sync agent in SECONDARY mode.
/// 
/// This connects to an existing hub and syncs data bidirectionally.
async fn start_secondary_agent(
    hub_url: String,
    device_id: String,
    db: Arc<Database>,
    sync_state: state::SyncState,
) {
    info!(hub_url = %hub_url, "Starting SECONDARY sync agent");
    
    // Create sync config for secondary mode
    let mut config = SyncConfig::load_or_default(None);
    config.device.id = device_id;
    config.sync.mode = SyncMode::Secondary;
    config.sync.hub_url = Some(hub_url.clone());

    // Create event emitter that updates our sync_state
    let sync_state_for_emitter = sync_state.clone();
    let hub_url_for_emitter = hub_url.clone();
    let hub_url_for_error = hub_url.clone();
    let emitter = Arc::new(SimpleSyncEmitter::new(move |connected| {
        let status = state::SyncStatusDto {
            connection_state: if connected { "connected".to_string() } else { "connecting".to_string() },
            sync_mode: "auto".to_string(),
            is_healthy: connected,
            hub_url: Some(hub_url_for_emitter.clone()),
            ..Default::default()
        };
        sync_state_for_emitter.update_status(status);
    }));

    // Create and start sync agent
    let mut agent = SyncAgent::with_emitter(config, db, emitter);
    
    match agent.start().await {
        Ok(handle) => {
            info!("SECONDARY sync agent started successfully");
            // CRITICAL: Store the handle in sync_state to keep the agent alive!
            sync_state.set_agent_handle(handle);
        }
        Err(e) => {
            error!(?e, "Failed to start SECONDARY sync agent");
            let status = state::SyncStatusDto {
                connection_state: "error".to_string(),
                sync_mode: "auto".to_string(),
                is_healthy: false,
                error_message: Some(format!("Connection failed: {:?}", e)),
                hub_url: Some(hub_url_for_error),
                ..Default::default()
            };
            sync_state.update_status(status);
        }
    }
}

/// Starts a simple WebSocket hub server for PRIMARY mode.
/// 
/// This creates a WebSocket server that:
/// - Accepts connections from SECONDARY devices
/// - Speaks the titan-sync protocol (Hello/Welcome handshake)
/// - Broadcasts inventory updates to all connected clients
/// - Applies received inventory deltas to local database
/// 
/// ## Protocol Flow
/// ```text
/// SECONDARY ───► Hello { device_id, store_id, ... }
/// PRIMARY   ◄─── Welcome { hub_device_id, store_id, election_term, ... }
///
/// SECONDARY ───► InventoryDelta { product_id, delta_qty }
/// PRIMARY   ───► (apply to DB, then broadcast)
/// PRIMARY   ───► InventoryUpdate { product_id, delta_qty } (to all clients)
/// ```
async fn start_primary_hub(
    port: u16,
    db: Arc<Database>,
    device_id: String,
    broadcast_tx: tokio::sync::broadcast::Sender<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use axum::{
        extract::ws::{Message, WebSocket, WebSocketUpgrade},
        extract::State,
        response::IntoResponse,
        routing::get,
        Router,
    };
    use futures_util::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio::sync::RwLock;

    // Shared state for the hub
    #[derive(Clone)]
    struct HubState {
        broadcast_tx: tokio::sync::broadcast::Sender<String>,
        client_count: Arc<RwLock<usize>>,
        db: Arc<Database>,
        hub_device_id: String,
    }

    let state = HubState {
        broadcast_tx: broadcast_tx.clone(),
        client_count: Arc::new(RwLock::new(0)),
        db: db.clone(),
        hub_device_id: device_id.clone(),
    };
    
    // Spawn outbox broadcaster task for PRIMARY
    // This reads PRIMARY's sync_outbox and broadcasts to connected secondaries
    let broadcast_tx_for_outbox = broadcast_tx.clone();
    let db_for_outbox = db.clone();
    let hub_device_id_for_outbox = device_id.clone();
    tokio::spawn(async move {
        info!("PRIMARY outbox broadcaster started");
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        
        loop {
            interval.tick().await;
            
            // Get pending outbox entries
            match db_for_outbox.sync_outbox().get_pending(100).await {
                Ok(entries) if !entries.is_empty() => {
                    info!(count = entries.len(), "PRIMARY broadcasting outbox entries to secondaries");
                    
                    // Collect entry IDs for marking as synced
                    let entry_ids: Vec<String> = entries.iter().map(|e| e.id.clone()).collect();
                    
                    // Build EntityUpdate messages for each entry
                    for entry in &entries {
                        // Parse the payload to get the actual entity data
                        let data = serde_json::from_str::<serde_json::Value>(&entry.payload).unwrap_or_default();
                        // Extract version from the data if available (sync_version field)
                        let version = data.get("sync_version")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(1);
                        // Extract updated_at from the data if available
                        let updated_at = data.get("updated_at")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&chrono::Utc::now().to_rfc3339())
                            .to_string();
                            
                        let entity_update = serde_json::json!({
                            "type": "EntityUpdate",
                            "payload": {
                                "entityType": entry.entity_type,
                                "entityId": entry.entity_id,
                                "operation": "upsert",
                                "data": data,
                                "version": version,
                                "updatedAt": updated_at,
                                "sourceDeviceId": hub_device_id_for_outbox,
                                "timestamp": chrono::Utc::now().to_rfc3339()
                            }
                        });
                        
                        // Broadcast to all connected clients
                        if let Err(e) = broadcast_tx_for_outbox.send(entity_update.to_string()) {
                            debug!(?e, "No clients subscribed to broadcast (this is OK if no secondaries connected)");
                        } else {
                            debug!(
                                entity_type = %entry.entity_type,
                                entity_id = %entry.entity_id,
                                "Broadcast EntityUpdate"
                            );
                        }
                    }
                    
                    // Mark entries as synced
                    for id in entry_ids {
                        if let Err(e) = db_for_outbox.sync_outbox().mark_synced(&id).await {
                            error!(?e, id = %id, "Failed to mark outbox entry as synced");
                        }
                    }
                    
                    info!("PRIMARY outbox broadcast complete");
                }
                Ok(_) => {
                    // No entries, normal case
                    debug!("PRIMARY outbox: no pending entries");
                }
                Err(e) => {
                    error!(?e, "Failed to query PRIMARY outbox");
                }
            }
        }
    });

    // WebSocket handler
    async fn ws_handler(
        ws: WebSocketUpgrade,
        State(state): State<HubState>,
    ) -> impl IntoResponse {
        ws.on_upgrade(move |socket| handle_socket(socket, state))
    }

    async fn handle_socket(socket: WebSocket, state: HubState) {
        let (mut sender, mut receiver) = socket.split();
        let mut rx = state.broadcast_tx.subscribe();

        // Increment client count
        {
            let mut count = state.client_count.write().await;
            *count += 1;
            info!(clients = *count, "Client connected to hub");
        }

        // Wait for Hello message from client
        let mut client_device_id = String::from("unknown");
        let mut handshake_complete = false;
        
        // Use the Sender in an Arc+Mutex so we can share it
        let sender = Arc::new(tokio::sync::Mutex::new(sender));
        let sender_for_broadcast = sender.clone();
        let sender_for_recv = sender.clone();
        
        // Handle messages until we get Hello (ignoring PING/PONG control frames)
        // WebSocket clients may send PING frames for keepalive before Hello
        let mut attempts = 0;
        const MAX_HANDSHAKE_ATTEMPTS: u32 = 50; // Give plenty of time for Hello
        
        while !handshake_complete && attempts < MAX_HANDSHAKE_ATTEMPTS {
            attempts += 1;
            
            match receiver.next().await {
                Some(Ok(Message::Text(text))) => {
                    // Parse as JSON to check message type
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text.to_string()) {
                        if json.get("type").and_then(|t| t.as_str()) == Some("Hello") {
                            // Extract device info from Hello
                            if let Some(payload) = json.get("payload") {
                                client_device_id = payload
                                    .get("deviceId")
                                    .or(payload.get("device_id"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                            }
                            
                            info!(client_device_id = %client_device_id, "Received Hello from client");
                            
                            // Send Welcome response in proper protocol format
                            let welcome = serde_json::json!({
                                "type": "Welcome",
                                "payload": {
                                    "hubDeviceId": state.hub_device_id,
                                    "storeId": "local-store",
                                    "electionTerm": 1,
                                    "serverTime": chrono::Utc::now().to_rfc3339()
                                }
                            });
                            
                            let mut s = sender.lock().await;
                            if s.send(Message::Text(welcome.to_string().into())).await.is_ok() {
                                handshake_complete = true;
                                info!(client_device_id = %client_device_id, "Sent Welcome, handshake complete");
                            }
                        } else {
                            debug!("Received non-Hello message during handshake: {:?}", json.get("type"));
                        }
                    }
                }
                Some(Ok(Message::Ping(data))) => {
                    // Respond to PING with PONG (standard WebSocket behavior)
                    debug!("Received PING during handshake, sending PONG");
                    let mut s = sender.lock().await;
                    let _ = s.send(Message::Pong(data)).await;
                }
                Some(Ok(Message::Pong(_))) => {
                    // Ignore PONG frames
                    debug!("Received PONG during handshake (ignored)");
                }
                Some(Ok(Message::Close(_))) => {
                    debug!("Received Close during handshake");
                    break;
                }
                Some(Err(e)) => {
                    warn!(?e, "WebSocket error during handshake");
                    break;
                }
                None => {
                    debug!("Connection closed during handshake");
                    break;
                }
                _ => {
                    // Binary or other frames - ignore
                    debug!("Received unexpected frame type during handshake");
                }
            }
        }

        if !handshake_complete {
            warn!("Handshake failed after {} attempts, closing connection", attempts);
            let mut count = state.client_count.write().await;
            *count = count.saturating_sub(1);
            return;
        }

        // Clone client_device_id for use after async closures
        let client_device_id_for_recv = client_device_id.clone();
        let client_device_id_for_disconnect = client_device_id.clone();
        
        // Spawn task to forward broadcasts to this client
        let send_task = tokio::spawn(async move {
            while let Ok(msg) = rx.recv().await {
                let mut s = sender_for_broadcast.lock().await;
                if s.send(Message::Text(msg.into())).await.is_err() {
                    break;
                }
            }
        });

        // Handle incoming messages
        let broadcast_tx = state.broadcast_tx.clone();
        let db = state.db.clone();
        let hub_device_id = state.hub_device_id.clone();
        
        let recv_task = tokio::spawn(async move {
            let client_device_id = client_device_id_for_recv;
            while let Some(Ok(msg)) = receiver.next().await {
                match msg {
                    Message::Text(text) => {
                        let text_str = text.to_string();
                        
                        // Parse message to handle protocol
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text_str) {
                            let msg_type = json.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            
                            match msg_type {
                                "InventoryDelta" => {
                                    // Extract delta info
                                    if let Some(payload) = json.get("payload") {
                                        let product_id = payload
                                            .get("productId")
                                            .or(payload.get("product_id"))
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        let sku = payload
                                            .get("sku")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        let delta_qty = payload
                                            .get("deltaQuantity")
                                            .or(payload.get("delta_quantity"))
                                            .or(payload.get("deltaQty"))
                                            .or(payload.get("delta_qty"))
                                            .and_then(|v| v.as_i64())
                                            .unwrap_or(0) as i32;
                                        
                                        info!(
                                            product_id = %product_id,
                                            sku = %sku,
                                            delta_qty = delta_qty,
                                            "Received InventoryDelta, applying to DB"
                                        );
                                        
                                        // Apply delta to PRIMARY's database
                                        if let Err(e) = db.products().update_stock(product_id, delta_qty).await {
                                            error!(?e, "Failed to apply inventory delta");
                                        } else {
                                            // Broadcast InventoryUpdate to all clients
                                            // Use the exact field names expected by SyncMessage::InventoryUpdate
                                            let update = serde_json::json!({
                                                "type": "InventoryUpdate",
                                                "payload": {
                                                    "productId": product_id,
                                                    "sku": sku,
                                                    "deltaQuantity": delta_qty,
                                                    "sourceDeviceId": client_device_id,
                                                    "timestamp": chrono::Utc::now().to_rfc3339()
                                                }
                                            });
                                            
                                            let _ = broadcast_tx.send(update.to_string());
                                            info!(product_id = %product_id, delta_qty = delta_qty, "Broadcast InventoryUpdate");
                                        }
                                    }
                                }
                                "OutboxBatch" => {
                                    // Handle batch entries from SECONDARY and broadcast inventory updates
                                    if let Some(payload) = json.get("payload") {
                                        let entities = payload
                                            .get("entities")
                                            .and_then(|e| e.as_array());
                                        
                                        let entries_count = entities.map(|arr| arr.len()).unwrap_or(0);
                                        info!(entries = entries_count, from = ?client_device_id, "Received OutboxBatch from SECONDARY");
                                        
                                        // Process each entry and extract inventory deltas
                                        if let Some(entities) = entities {
                                            for entity in entities {
                                                let entity_type = entity.get("entityType")
                                                    .and_then(|t| t.as_str())
                                                    .unwrap_or("");
                                                
                                                // For SALE entries, extract items and broadcast inventory updates
                                                if entity_type == "SALE" {
                                                    if let Some(payload_str) = entity.get("payload").and_then(|p| p.as_str()) {
                                                        // Parse the sale payload
                                                        if let Ok(sale_json) = serde_json::from_str::<serde_json::Value>(payload_str) {
                                                            // Extract items array
                                                            if let Some(items) = sale_json.get("items").and_then(|i| i.as_array()) {
                                                                for item in items {
                                                                    let product_id = item.get("productId")
                                                                        .or_else(|| item.get("product_id"))
                                                                        .and_then(|p| p.as_str());
                                                                    let quantity = item.get("quantity")
                                                                        .and_then(|q| q.as_i64())
                                                                        .unwrap_or(0);
                                                                    
                                                                    if let Some(product_id) = product_id {
                                                                        // Broadcast negative delta (items sold)
                                                                        let delta_qty = -(quantity as i32);
                                                                        
                                                                        let update = serde_json::json!({
                                                                            "type": "InventoryUpdate",
                                                                            "payload": {
                                                                                "productId": product_id,
                                                                                "deltaQuantity": delta_qty,
                                                                                "sourceDeviceId": client_device_id,
                                                                                "timestamp": chrono::Utc::now().to_rfc3339()
                                                                            }
                                                                        });
                                                                        
                                                                        let _ = broadcast_tx.send(update.to_string());
                                                                        info!(
                                                                            product_id = %product_id, 
                                                                            delta = delta_qty, 
                                                                            source = ?client_device_id,
                                                                            "Broadcast InventoryUpdate from SECONDARY sale"
                                                                        );
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        
                                        // Send BatchAck
                                        let acked_ids: Vec<String> = payload
                                            .get("entities")
                                            .and_then(|e| e.as_array())
                                            .map(|arr| {
                                                arr.iter()
                                                    .filter_map(|e| e.get("id").and_then(|id| id.as_str()))
                                                    .map(|s| s.to_string())
                                                    .collect()
                                            })
                                            .unwrap_or_default();
                                        
                                        let ack = serde_json::json!({
                                            "type": "BatchAck",
                                            "payload": {
                                                "ackedIds": acked_ids,
                                                "failedIds": [],
                                                "newCursor": 0
                                            }
                                        });
                                        
                                        let mut s = sender_for_recv.lock().await;
                                        let _ = s.send(Message::Text(ack.to_string().into())).await;
                                    }
                                }
                                "Ping" => {
                                    // Respond with Pong
                                    let timestamp = json.get("payload")
                                        .and_then(|p| p.get("timestamp"))
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("");
                                    
                                    let pong = serde_json::json!({
                                        "type": "Pong",
                                        "payload": {
                                            "pingTimestamp": timestamp,
                                            "pongTimestamp": chrono::Utc::now().to_rfc3339()
                                        }
                                    });
                                    
                                    let mut s = sender_for_recv.lock().await;
                                    let _ = s.send(Message::Text(pong.to_string().into())).await;
                                }
                                _ => {
                                    debug!(msg_type = %msg_type, "Received unknown message type");
                                }
                            }
                        }
                    }
                    Message::Ping(data) => {
                        let mut s = sender_for_recv.lock().await;
                        let _ = s.send(Message::Pong(data)).await;
                    }
                    Message::Close(_) => {
                        break;
                    }
                    _ => {}
                }
            }
        });

        // Wait for either task to complete
        tokio::select! {
            _ = send_task => {},
            _ = recv_task => {},
        }

        // Decrement client count
        {
            let mut count = state.client_count.write().await;
            *count = count.saturating_sub(1);
            info!(clients = *count, client = %client_device_id_for_disconnect, "Client disconnected from hub");
        }
    }

    async fn health_handler() -> &'static str {
        "OK"
    }

    // Build router - support both /ws and /sync for compatibility
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/sync", get(ws_handler))
        .route("/health", get(health_handler))
        .with_state(state);

    // Bind and run
    let bind_addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&bind_addr).await?;
    info!(addr = %bind_addr, "Hub server listening");

    axum::serve(listener, app).await?;

    Ok(())
}