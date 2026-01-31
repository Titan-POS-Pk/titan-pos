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
//! │   └── config.rs   ◄─── Configuration state
//! ├── commands/
//! │   ├── mod.rs      ◄─── Command exports
//! │   ├── product.rs  ◄─── Product search/CRUD commands
//! │   ├── sale.rs     ◄─── Sale/transaction commands
//! │   └── cart.rs     ◄─── Cart manipulation commands
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
//! │  ┌──────────────────┐ ┌──────────────────┐ ┌──────────────────────┐   │
//! │  │    DbState       │ │    CartState     │ │    ConfigState       │   │
//! │  │                  │ │                  │ │                      │   │
//! │  │  • Database pool │ │  • Current cart  │ │  • Tenant ID         │   │
//! │  │  • Repositories  │ │  • Cart items    │ │  • Tax rates         │   │
//! │  │                  │ │  • Totals        │ │  • Store name        │   │
//! │  └──────────────────┘ └──────────────────┘ └──────────────────────┘   │
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
use tauri::Manager;
use tracing::{info, Level};
use tracing_subscriber::EnvFilter;

use state::{CartState, ConfigState, DbState};
use titan_db::{Database, DbConfig};

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
            let db_state = DbState::new(db);
            let cart_state = CartState::new();
            let config_state = ConfigState::default();

            // Register state with Tauri
            app.manage(db_state);
            app.manage(cart_state);
            app.manage(config_state);

            info!("State initialized");
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
/// ## Platform-Specific Paths
/// - **macOS**: `~/Library/Application Support/com.titan.pos/titan.db`
/// - **Windows**: `%APPDATA%\titan\pos\titan.db`
/// - **Linux**: `~/.local/share/titan-pos/titan.db`
///
/// ## Development Override
/// Set `TITAN_DB_PATH` environment variable to use a custom path.
fn get_database_path(_app: &tauri::App) -> Result<PathBuf, Box<dyn std::error::Error>> {
    // Check for override
    if let Ok(path) = std::env::var("TITAN_DB_PATH") {
        return Ok(PathBuf::from(path));
    }

    // Use platform-specific app data directory
    let proj_dirs = ProjectDirs::from("com", "titan", "pos")
        .ok_or("Could not determine app data directory")?;

    let data_dir = proj_dirs.data_dir();

    // Create directory if it doesn't exist
    std::fs::create_dir_all(data_dir)?;

    Ok(data_dir.join("titan.db"))
}
