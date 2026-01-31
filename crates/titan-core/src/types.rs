//! # Domain Types
//!
//! Core domain types used throughout Titan POS.
//!
//! ## Type Hierarchy
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         Domain Types                                    │
//! │                                                                         │
//! │  ┌─────────────────┐   ┌─────────────────┐   ┌─────────────────┐       │
//! │  │    Product      │   │      Sale       │   │    Payment      │       │
//! │  │  ─────────────  │   │  ─────────────  │   │  ─────────────  │       │
//! │  │  id (UUID)      │   │  id (UUID)      │   │  id (UUID)      │       │
//! │  │  sku (business) │   │  receipt_number │   │  sale_id (FK)   │       │
//! │  │  name           │   │  status         │   │  method         │       │
//! │  │  price_cents    │   │  total_cents    │   │  amount_cents   │       │
//! │  └─────────────────┘   └─────────────────┘   └─────────────────┘       │
//! │                                                                         │
//! │  ┌─────────────────┐   ┌─────────────────┐   ┌─────────────────┐       │
//! │  │    TaxRate      │   │   SaleStatus    │   │ PaymentMethod   │       │
//! │  │  ─────────────  │   │  ─────────────  │   │  ─────────────  │       │
//! │  │  bps (u32)      │   │  Draft          │   │  Cash           │       │
//! │  │  825 = 8.25%    │   │  Completed      │   │  ExternalCard   │       │
//! │  └─────────────────┘   │  Voided         │   └─────────────────┘       │
//! │                        └─────────────────┘                              │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Dual-Key Identity Pattern
//! Every entity has:
//! - `id`: UUID v4 - immutable, used for database relations
//! - Business ID: (sku, receipt_number, etc.) - human-readable, potentially mutable

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::money::Money;

// =============================================================================
// Tax Rate
// =============================================================================

/// Tax rate represented in basis points (bps).
///
/// ## Why Basis Points?
/// 1 basis point = 0.01% = 1/10000
/// 825 bps = 8.25% (e.g., Texas sales tax)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaxRate(u32);

impl TaxRate {
    /// Creates a tax rate from basis points.
    #[inline]
    pub const fn from_bps(bps: u32) -> Self {
        TaxRate(bps)
    }

    /// Creates a tax rate from a percentage (for convenience).
    pub fn from_percentage(pct: f64) -> Self {
        TaxRate((pct * 100.0).round() as u32)
    }

    /// Returns the rate in basis points.
    #[inline]
    pub const fn bps(&self) -> u32 {
        self.0
    }

    /// Returns the rate as a percentage (for display only).
    #[inline]
    pub fn percentage(&self) -> f64 {
        self.0 as f64 / 100.0
    }

    /// Zero tax rate.
    #[inline]
    pub const fn zero() -> Self {
        TaxRate(0)
    }

    /// Checks if tax rate is zero.
    #[inline]
    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }
}

impl Default for TaxRate {
    fn default() -> Self {
        TaxRate::zero()
    }
}

// =============================================================================
// Product
// =============================================================================

/// A product available for sale.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Product {
    /// Unique identifier (UUID v4).
    pub id: String,

    /// Tenant this product belongs to.
    pub tenant_id: String,

    /// Stock Keeping Unit - business identifier.
    pub sku: String,

    /// Barcode (EAN-13, UPC-A, etc.).
    pub barcode: Option<String>,

    /// Display name shown to cashier and on receipt.
    pub name: String,

    /// Optional description for product details.
    pub description: Option<String>,

    /// Price in cents (smallest currency unit).
    pub price_cents: i64,

    /// Cost in cents (for profit margin calculations).
    pub cost_cents: Option<i64>,

    /// Tax rate in basis points (825 = 8.25%).
    pub tax_rate_bps: u32,

    /// Whether to track inventory for this product.
    pub track_inventory: bool,

    /// Allow selling when stock is zero or negative.
    pub allow_negative_stock: bool,

    /// Current stock level.
    pub current_stock: Option<i64>,

    /// Whether product is active (soft delete).
    pub is_active: bool,

    /// When the product was created.
    #[ts(as = "String")]
    pub created_at: DateTime<Utc>,

    /// When the product was last updated.
    #[ts(as = "String")]
    pub updated_at: DateTime<Utc>,

    /// Sync version for CRDT conflict resolution.
    pub sync_version: i64,
}

impl Product {
    /// Returns the price as a Money type.
    #[inline]
    pub fn price(&self) -> Money {
        Money::from_cents(self.price_cents)
    }

    /// Returns the tax rate.
    #[inline]
    pub fn tax_rate(&self) -> TaxRate {
        TaxRate::from_bps(self.tax_rate_bps)
    }

    /// Checks if product can be sold (in stock or doesn't track inventory).
    pub fn can_sell(&self, quantity: i64) -> bool {
        if !self.track_inventory {
            return true;
        }

        let stock = self.current_stock.unwrap_or(0);
        if stock >= quantity {
            return true;
        }

        self.allow_negative_stock
    }
}

// =============================================================================
// Sale Status
// =============================================================================

/// The status of a sale transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(feature = "sqlx", sqlx(rename_all = "lowercase"))]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum SaleStatus {
    /// Sale is in progress (items being added).
    Draft,
    /// Sale has been paid and finalized.
    Completed,
    /// Sale was cancelled/refunded.
    Voided,
}

impl Default for SaleStatus {
    fn default() -> Self {
        SaleStatus::Draft
    }
}

// =============================================================================
// Payment Method
// =============================================================================

#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(feature = "sqlx", sqlx(rename_all = "snake_case"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum PaymentMethod {
    /// Physical cash payment.
    Cash,
    /// Card payment on external terminal.
    ExternalCard,
}

// =============================================================================
// Sale
// =============================================================================

/// A completed or in-progress sale transaction.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Sale {
    pub id: String,
    pub tenant_id: String,
    pub receipt_number: String,
    pub status: SaleStatus,
    pub subtotal_cents: i64,
    pub tax_cents: i64,
    pub discount_cents: i64,
    pub total_cents: i64,
    pub user_id: String,
    pub device_id: String,
    pub notes: Option<String>,
    #[ts(as = "String")]
    pub created_at: DateTime<Utc>,
    #[ts(as = "String")]
    pub updated_at: DateTime<Utc>,
    #[ts(as = "Option<String>")]
    pub completed_at: Option<DateTime<Utc>>,
    pub sync_version: i64,
}

// =============================================================================
// Sale Item
// =============================================================================

/// A line item in a sale.
/// Uses snapshot pattern to freeze product data at time of sale.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SaleItem {
    pub id: String,
    pub sale_id: String,
    pub product_id: String,
    /// SKU at time of sale (frozen).
    pub sku_snapshot: String,
    /// Product name at time of sale (frozen).
    pub name_snapshot: String,
    /// Unit price in cents at time of sale (frozen).
    pub unit_price_cents: i64,
    /// Quantity sold.
    pub quantity: i64,
    /// Line total before tax (unit_price × quantity).
    pub line_total_cents: i64,
    /// Tax for this line item.
    pub tax_cents: i64,
    /// Discount applied to this line.
    pub discount_cents: i64,
    #[ts(as = "String")]
    pub created_at: DateTime<Utc>,
}

impl SaleItem {
    /// Returns the unit price as Money.
    #[inline]
    pub fn unit_price(&self) -> Money {
        Money::from_cents(self.unit_price_cents)
    }

    /// Returns the line total as Money.
    #[inline]
    pub fn line_total(&self) -> Money {
        Money::from_cents(self.line_total_cents)
    }
}

// =============================================================================
// Payment
// =============================================================================

/// A payment towards a sale.
/// A sale can have multiple payments for split tender scenarios.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Payment {
    pub id: String,
    pub sale_id: String,
    pub method: PaymentMethod,
    /// Amount paid in cents.
    pub amount_cents: i64,
    /// For cash: amount customer gave (to calculate change).
    pub tendered_cents: Option<i64>,
    /// For cash: change returned to customer.
    pub change_cents: Option<i64>,
    /// External reference (card auth code, etc.).
    pub reference: Option<String>,
    #[ts(as = "String")]
    pub created_at: DateTime<Utc>,
}

impl Payment {
    /// Returns the payment amount as Money.
    #[inline]
    pub fn amount(&self) -> Money {
        Money::from_cents(self.amount_cents)
    }
}

// =============================================================================
// Sync Outbox
// =============================================================================

/// An entry in the sync outbox queue.
/// Uses outbox pattern for reliable sync with cloud.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncOutboxEntry {
    pub id: String,
    pub tenant_id: String,
    /// Type of entity being synced: "SALE", "PRODUCT", "PAYMENT", etc.
    pub entity_type: String,
    /// ID of the entity being synced.
    pub entity_id: String,
    /// The full entity data as JSON.
    pub payload: String,
    /// Number of sync attempts.
    pub attempts: i64,
    /// Last error message if sync failed.
    pub last_error: Option<String>,
    #[ts(as = "String")]
    pub created_at: DateTime<Utc>,
    /// When last sync was attempted.
    #[ts(as = "Option<String>")]
    pub attempted_at: Option<DateTime<Utc>>,
    /// When successfully synced.
    #[ts(as = "Option<String>")]
    pub synced_at: Option<DateTime<Utc>>,
}

// =============================================================================
// Configuration Types
// =============================================================================

/// Tax calculation mode for the tenant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum TaxMode {
    /// Price + tax shown separately (USA model).
    Exclusive,
    /// Price includes tax (EU/UK model).
    Inclusive,
}

impl Default for TaxMode {
    fn default() -> Self {
        TaxMode::Exclusive
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tax_rate_from_bps() {
        let rate = TaxRate::from_bps(825);
        assert_eq!(rate.bps(), 825);
        assert!((rate.percentage() - 8.25).abs() < 0.001);
    }

    #[test]
    fn test_tax_rate_from_percentage() {
        let rate = TaxRate::from_percentage(8.25);
        assert_eq!(rate.bps(), 825);
    }

    #[test]
    fn test_sale_status_default() {
        let status = SaleStatus::default();
        assert_eq!(status, SaleStatus::Draft);
    }

    #[test]
    fn test_tax_mode_default() {
        let mode = TaxMode::default();
        assert_eq!(mode, TaxMode::Exclusive);
    }
}
