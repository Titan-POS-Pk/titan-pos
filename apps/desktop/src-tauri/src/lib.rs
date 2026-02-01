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
use tauri::Manager;
use tracing::{info, Level};
use tracing_subscriber::EnvFilter;

use state::{CartState, ConfigState, DbState, SyncState};
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
            let sync_state = SyncState::new();

            // Register state with Tauri
            app.manage(db_state);
            app.manage(cart_state);
            app.manage(config_state);
            app.manage(sync_state);

            info!("State initialized (sync agent not started - requires configuration)");
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
/// ## Development Workflow
/// ```bash
/// # 1. Seed the database from project root
/// cargo run -p titan-db --bin seed
///
/// # 2. Run the Tauri app (auto-detects data/titan.db)
/// cd apps/desktop && pnpm tauri dev
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
        // Paths to try, in order of preference:
        // 1. Relative to CARGO_MANIFEST_DIR (set at compile time for src-tauri)
        // 2. Standard project root locations
        let paths_to_try = [
            // From apps/desktop/src-tauri, go up to project root
            PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../data/titan.db")),
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
