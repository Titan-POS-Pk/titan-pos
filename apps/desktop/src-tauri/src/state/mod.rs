//! # State Module
//!
//! Manages application state for the Tauri desktop app.
//!
//! ## Why Multiple State Types? (Option B)
//! Instead of a single `AppState` struct containing everything,
//! we use separate state types. This approach:
//!
//! 1. **Better Separation of Concerns**: Each state type has a single responsibility
//! 2. **Easier Testing**: Can mock/inject individual states
//! 3. **Clearer Command Signatures**: Commands declare exactly what state they need
//! 4. **Reduced Contention**: Independent states don't block each other
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    State Architecture                                   │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                      Tauri Runtime                              │   │
//! │  │  app.manage(db_state);                                          │   │
//! │  │  app.manage(cart_state);                                        │   │
//! │  │  app.manage(config_state);                                      │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                              │                                          │
//! │          ┌──────────────────┼──────────────────┐                       │
//! │          ▼                  ▼                  ▼                        │
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐              │
//! │  │   DbState    │  │  CartState   │  │   ConfigState    │              │
//! │  │              │  │              │  │                  │              │
//! │  │  Database    │  │  Arc<Mutex<  │  │  tenant_id       │              │
//! │  │  (SQLite     │  │    Cart      │  │  store_name      │              │
//! │  │   pool)      │  │  >>          │  │  tax_rate        │              │
//! │  └──────────────┘  └──────────────┘  └──────────────────┘              │
//! │                                                                         │
//! │  THREAD SAFETY:                                                        │
//! │  • DbState: Database has internal connection pool (thread-safe)        │
//! │  • CartState: Protected by Arc<Mutex<T>> for exclusive access          │
//! │  • ConfigState: Read-only after initialization                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

mod cart;
mod config;
mod db;

pub use cart::{Cart, CartItem, CartState, CartTotals};
pub use config::ConfigState;
pub use db::DbState;
