//! # Titan Desktop Application Entry Point
//!
//! This is the main entry point for the Tauri desktop application.
//!
//! ## Application Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                        Titan POS Desktop                                │
//! │                                                                         │
//! │  ┌──────────────────────────────────────────────────────────────────┐  │
//! │  │                      Tauri WebView                               │  │
//! │  │  ┌────────────────────────────────────────────────────────────┐  │  │
//! │  │  │                    SolidJS Frontend                        │  │  │
//! │  │  │  • Product Search       • Cart Display                     │  │  │
//! │  │  │  • Payment Modal        • Receipt Printer                  │  │  │
//! │  │  └────────────────────────────────────────────────────────────┘  │  │
//! │  │                              │                                   │  │
//! │  │                     invoke('command')                           │  │
//! │  │                              │                                   │  │
//! │  └──────────────────────────────┼───────────────────────────────────┘  │
//! │                                 ▼                                       │
//! │  ┌──────────────────────────────────────────────────────────────────┐  │
//! │  │                    Rust Backend (this crate)                     │  │
//! │  │                                                                  │  │
//! │  │  main.rs ────► Sets up logging, database, state                 │  │
//! │  │                                                                  │  │
//! │  │  lib.rs ─────► Configures Tauri plugins and commands            │  │
//! │  │                                                                  │  │
//! │  │  commands/ ──► search_products, add_to_cart, process_payment    │  │
//! │  │                                                                  │  │
//! │  │  state/ ─────► DbState, CartState, ConfigState                  │  │
//! │  │                                                                  │  │
//! │  └──────────────────────────────────────────────────────────────────┘  │
//! │                                 │                                       │
//! │                                 ▼                                       │
//! │  ┌──────────────────────────────────────────────────────────────────┐  │
//! │  │                         SQLite Database                          │  │
//! │  │  titan.db (local file, WAL mode, FTS5 enabled)                   │  │
//! │  └──────────────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Startup Sequence
//! 1. Initialize tracing (logging)
//! 2. Determine database path (app data directory)
//! 3. Connect to database & run migrations
//! 4. Create state objects (DbState, CartState, ConfigState)
//! 5. Build Tauri application
//! 6. Register commands
//! 7. Launch window

// Prevents an additional console window on Windows in release
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    // Run the Tauri application
    // The actual setup is in lib.rs for better testability
    titan_desktop_lib::run();
}
