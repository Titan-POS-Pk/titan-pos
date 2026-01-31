//! # Repository Module
//!
//! Database repository implementations for Titan POS.
//!
//! ## Repository Pattern
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Repository Pattern Explained                         │
//! │                                                                         │
//! │  The Repository pattern abstracts database access behind a clean API.  │
//! │                                                                         │
//! │  Tauri Command                                                         │
//! │       │                                                                 │
//! │       │  db.products().search("coke", 20)                              │
//! │       │  ↓                                                              │
//! │       ▼                                                                 │
//! │  ProductRepository                                                     │
//! │  ├── search(&self, query, limit)                                       │
//! │  ├── get_by_id(&self, id)                                              │
//! │  ├── insert(&self, product)                                            │
//! │  └── update(&self, product)                                            │
//! │       │                                                                 │
//! │       │  SQL Query                                                      │
//! │       ▼                                                                 │
//! │  SQLite Database                                                       │
//! │                                                                         │
//! │  Benefits:                                                              │
//! │  • Clean separation of concerns                                        │
//! │  • Easy to test (mock the repository)                                  │
//! │  • SQL is isolated in one place                                        │
//! │  • Can swap database implementations                                   │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Available Repositories
//!
//! - [`ProductRepository`] - Product CRUD and search
//! - [`SaleRepository`] - Sale and sale item operations
//! - [`SyncOutboxRepository`] - Sync queue management

pub mod product;
pub mod sale;
pub mod sync;
