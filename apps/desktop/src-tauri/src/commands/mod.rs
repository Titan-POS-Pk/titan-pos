//! # Tauri Commands Module
//!
//! All commands exposed to the SolidJS frontend.
//!
//! ## Command Organization
//! ```text
//! commands/
//! ├── mod.rs      ◄─── You are here (exports)
//! ├── product.rs  ◄─── Product search, CRUD
//! ├── cart.rs     ◄─── Cart manipulation
//! ├── sale.rs     ◄─── Sale/payment processing
//! ├── config.rs   ◄─── Configuration retrieval
//! └── sync.rs     ◄─── Sync status and control
//! ```
//!
//! ## How Commands Work
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Tauri Command Flow                                   │
//! │                                                                         │
//! │  SolidJS Frontend                                                       │
//! │  ─────────────────                                                      │
//! │  import { invoke } from '@tauri-apps/api/core';                         │
//! │                                                                         │
//! │  const products = await invoke('search_products', {                     │
//! │    query: 'coke',                                                       │
//! │    limit: 20                                                            │
//! │  });                                                                    │
//! │         │                                                               │
//! │         │ (IPC via WebView)                                             │
//! │         ▼                                                               │
//! │  Rust Backend                                                           │
//! │  ────────────                                                           │
//! │  #[tauri::command]                                                      │
//! │  async fn search_products(                                              │
//! │      db: State<'_, DbState>,  ◄── Injected by Tauri                    │
//! │      query: String,           ◄── From invoke params                   │
//! │      limit: Option<u32>,      ◄── Optional param                       │
//! │  ) -> Result<Vec<ProductDto>, ApiError>                                 │
//! │         │                                                               │
//! │         │ (JSON serialization)                                          │
//! │         ▼                                                               │
//! │  SolidJS receives: ProductDto[]                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## State Injection (Option B)
//! Each command declares only the state it needs:
//! ```rust,ignore
//! // Only needs database
//! async fn search_products(db: State<'_, DbState>, ...)
//!
//! // Only needs cart
//! async fn get_cart(cart: State<'_, CartState>)
//!
//! // Needs both
//! async fn add_to_cart(db: State<'_, DbState>, cart: State<'_, CartState>, ...)
//!
//! // Sync commands
//! async fn get_sync_status(sync: State<'_, SyncState>)
//! ```

pub mod cart;
pub mod config;
pub mod product;
pub mod sale;
pub mod sync;
