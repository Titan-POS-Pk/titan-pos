//! # titan-db: Database Layer for Titan POS
//!
//! This crate provides database access for the Titan POS system.
//! It uses SQLite for local storage with sqlx for async operations.
//!
//! ## Architecture Position
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                        Titan POS Data Flow                              │
//! │                                                                         │
//! │  Tauri Command (search_products)                                       │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                     titan-db (THIS CRATE)                       │   │
//! │  │                                                                 │   │
//! │  │   ┌───────────────┐    ┌───────────────┐    ┌──────────────┐  │   │
//! │  │   │   Database    │    │  Repositories │    │  Migrations  │  │   │
//! │  │   │   (pool.rs)   │    │ (product.rs)  │    │  (embedded)  │  │   │
//! │  │   │               │    │               │    │              │  │   │
//! │  │   │ SqlitePool    │    │ ProductRepo   │    │ 001_init.sql │  │   │
//! │  │   │ Connection    │◄───│ SaleRepo      │    │ 002_fts.sql  │  │   │
//! │  │   │ Management    │    │ PaymentRepo   │    │ ...          │  │   │
//! │  │   └───────────────┘    └───────────────┘    └──────────────┘  │   │
//! │  │                                                                 │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                     SQLite Database                             │   │
//! │  │   ~/Library/Application Support/com.titan.pos/titan.db         │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Module Organization
//!
//! - [`pool`] - Connection pool creation and configuration
//! - [`migrations`] - Embedded database migrations
//! - [`error`] - Database error types
//! - [`repository`] - Repository implementations (product, sale, etc.)
//!
//! ## Usage
//!
//! ```rust,ignore
//! use titan_db::{Database, DbConfig};
//!
//! // Create database with default config
//! let config = DbConfig::new("path/to/db.sqlite");
//! let db = Database::new(config).await?;
//!
//! // Run migrations
//! db.run_migrations().await?;
//!
//! // Use repositories
//! let products = db.products().search("coke", 20).await?;
//! ```

// =============================================================================
// Module Declarations
// =============================================================================

pub mod error;
pub mod migrations;
pub mod pool;
pub mod repository;

// =============================================================================
// Re-exports
// =============================================================================

pub use error::DbError;
pub use pool::{Database, DbConfig};

// Repository re-exports for convenience
pub use repository::product::ProductRepository;
pub use repository::sale::SaleRepository;
pub use repository::sync::SyncOutboxRepository;
