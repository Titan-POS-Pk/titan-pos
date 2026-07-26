//! # titan-core: Pure Business Logic for Titan POS
//!
//! This crate is the **heart** of Titan POS. It contains all business logic
//! as pure functions with zero I/O dependencies.
//!
//! ## Architecture Position
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                        Titan POS Architecture                           │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                    Frontend (SolidJS)                           │   │
//! │  │    Search UI ──► Cart UI ──► Tender UI ──► Receipt UI          │   │
//! │  └─────────────────────────────┬───────────────────────────────────┘   │
//! │                                │ Tauri IPC                              │
//! │  ┌─────────────────────────────▼───────────────────────────────────┐   │
//! │  │                    Tauri Commands                               │   │
//! │  │    search_products, add_to_cart, process_payment, etc.         │   │
//! │  └─────────────────────────────┬───────────────────────────────────┘   │
//! │                                │                                        │
//! │  ┌─────────────────────────────▼───────────────────────────────────┐   │
//! │  │               ★ titan-core (THIS CRATE) ★                       │   │
//! │  │                                                                 │   │
//! │  │   ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐  │   │
//! │  │   │   types   │  │   money   │  │   cart    │  │ validation│  │   │
//! │  │   │  Product  │  │   Money   │  │   Cart    │  │   rules   │  │   │
//! │  │   │   Sale    │  │ TaxCalc   │  │ CartItem  │  │  checks   │  │   │
//! │  │   └───────────┘  └───────────┘  └───────────┘  └───────────┘  │   │
//! │  │                                                                 │   │
//! │  │   NO I/O • NO DATABASE • NO NETWORK • PURE FUNCTIONS           │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                                │                                        │
//! │  ┌─────────────────────────────▼───────────────────────────────────┐   │
//! │  │                    titan-db (Database Layer)                    │   │
//! │  │              SQLite queries, migrations, repositories           │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Modules
//!
//! - [`types`] - Domain types (Product, Sale, Payment, etc.)
//! - [`money`] - Money type with integer arithmetic (no floating point!)
//! - [`error`] - Domain error types
//! - [`validation`] - Business rule validation
//!
//! ## Design Principles
//!
//! 1. **Pure Functions**: Every function is deterministic - same input = same output
//! 2. **No I/O**: Database, network, file system access is FORBIDDEN here
//! 3. **Integer Money**: All monetary values are in cents (i64) to avoid float errors
//! 4. **Explicit Errors**: All errors are typed, never strings or panics
//!
//! ## Example Usage
//!
//! ```rust
//! use titan_core::money::Money;
//! use titan_core::types::TaxRate;
//!
//! // Create money from cents (never from floats!)
//! let price = Money::from_cents(1099); // $10.99
//!
//! // Calculate tax using Bankers Rounding
//! let tax_rate = TaxRate::from_bps(825); // 8.25%
//! let tax = price.calculate_tax(tax_rate);
//!
//! // Tax on $10.99 at 8.25% = $0.91 (rounded)
//! assert_eq!(tax.cents(), 91);
//! ```

// =============================================================================
// Module Declarations
// =============================================================================

pub mod error;
pub mod money;
pub mod types;
pub mod validation;

// =============================================================================
// Re-exports for Convenience
// =============================================================================
// These allow users to do `use titan_core::Money` instead of
// `use titan_core::money::Money`

pub use error::{CoreError, ValidationError};
pub use money::Money;
pub use types::*;

// =============================================================================
// Crate-Level Constants
// =============================================================================

/// Default tenant ID for v0.1 (single-tenant runtime with multi-tenant schema)
///
/// ## Why a constant?
/// v0.1 is single-tenant, but the database schema includes tenant_id for future
/// multi-tenancy. This constant is used throughout the codebase and will be
/// replaced with dynamic tenant resolution in v0.5+.
pub const DEFAULT_TENANT_ID: &str = "00000000-0000-0000-0000-000000000001";

/// Maximum items allowed in a single cart
///
/// ## Business Reason
/// Prevents runaway carts and ensures reasonable transaction sizes.
/// Can be made configurable per-tenant in future versions.
pub const MAX_CART_ITEMS: usize = 100;

/// Maximum quantity of a single item in cart
///
/// ## Business Reason
/// Prevents accidental over-ordering (e.g., typing 1000 instead of 10)
/// Configurable per-tenant in future versions.
pub const MAX_ITEM_QUANTITY: i64 = 999;

/// Maximum price of a single item, in cents ($1,000,000,000.00).
///
/// ## Why a ceiling at all
/// Any price above this is a data-entry error, not a product. Bounding it here
/// is also what keeps cart arithmetic inside i64: the worst legal cart is
/// `MAX_PRICE_CENTS × MAX_ITEM_QUANTITY × MAX_CART_ITEMS ≈ 1.0e16` cents,
/// roughly 920× below `i64::MAX`. Without the bound, a price near `i64::MAX`
/// would overflow the line total, which wraps silently in release builds.
///
/// See `validation::tests::test_worst_case_cart_fits_in_i64`.
pub const MAX_PRICE_CENTS: i64 = 100_000_000_000;
