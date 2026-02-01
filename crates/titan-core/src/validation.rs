//! # Validation Module
//!
//! Input validation utilities for Titan POS.
//!
//! ## Validation Strategy
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                      Validation Layers                                  │
//! │                                                                         │
//! │  Layer 1: Frontend (TypeScript)                                        │
//! │  ├── Basic format checks (empty, length)                               │
//! │  └── Immediate user feedback                                           │
//! │           │                                                             │
//! │           ▼                                                             │
//! │  Layer 2: Tauri Command (Rust)                                         │
//! │  ├── Type validation (deserialization)                                 │
//! │  └── THIS MODULE: Business rule validation                             │
//! │           │                                                             │
//! │           ▼                                                             │
//! │  Layer 3: Database (SQLite)                                            │
//! │  ├── NOT NULL constraints                                              │
//! │  ├── UNIQUE constraints                                                │
//! │  └── Foreign key constraints                                           │
//! │                                                                         │
//! │  Defense in depth: Multiple layers catch different errors              │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//! ```rust,no_run
//! use titan_core::validation::{validate_sku, validate_quantity};
//!
//! // Validate SKU before database insert
//! validate_sku("COKE-330").unwrap();
//!
//! // Validate quantity before cart operation
//! validate_quantity(5).unwrap();
//! ```

use crate::error::ValidationError;
use crate::{MAX_CART_ITEMS, MAX_ITEM_QUANTITY};

/// Result type for validation operations.
pub type ValidationResult<T> = Result<T, ValidationError>;

// =============================================================================
// String Validators
// =============================================================================

/// Validates a SKU (Stock Keeping Unit).
///
/// ## Rules
/// - Must not be empty
/// - Must be between 1 and 50 characters
/// - Should contain only alphanumeric characters, hyphens, underscores
///
/// ## Example
/// ```rust
/// use titan_core::validation::validate_sku;
///
/// assert!(validate_sku("COKE-330").is_ok());
/// assert!(validate_sku("").is_err());
/// assert!(validate_sku("A".repeat(100).as_str()).is_err());
/// ```
pub fn validate_sku(sku: &str) -> ValidationResult<()> {
    let sku = sku.trim();

    if sku.is_empty() {
        return Err(ValidationError::Required {
            field: "sku".to_string(),
        });
    }

    if sku.len() > 50 {
        return Err(ValidationError::TooLong {
            field: "sku".to_string(),
            max: 50,
        });
    }

    // Check for valid characters (alphanumeric, hyphen, underscore)
    if !sku
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ValidationError::InvalidFormat {
            field: "sku".to_string(),
            reason: "must contain only letters, numbers, hyphens, and underscores".to_string(),
        });
    }

    Ok(())
}

/// Validates a product name.
///
/// ## Rules
/// - Must not be empty
/// - Must be between 1 and 200 characters
///
/// ## Example
/// ```rust
/// use titan_core::validation::validate_product_name;
///
/// assert!(validate_product_name("Coca-Cola 330ml").is_ok());
/// assert!(validate_product_name("").is_err());
/// ```
pub fn validate_product_name(name: &str) -> ValidationResult<()> {
    let name = name.trim();

    if name.is_empty() {
        return Err(ValidationError::Required {
            field: "name".to_string(),
        });
    }

    if name.len() > 200 {
        return Err(ValidationError::TooLong {
            field: "name".to_string(),
            max: 200,
        });
    }

    Ok(())
}

/// Validates a search query.
///
/// ## Rules
/// - Can be empty (returns all/default results)
/// - Maximum 100 characters
///
/// ## Returns
/// The trimmed query string.
pub fn validate_search_query(query: &str) -> ValidationResult<String> {
    let query = query.trim();

    if query.len() > 100 {
        return Err(ValidationError::TooLong {
            field: "query".to_string(),
            max: 100,
        });
    }

    Ok(query.to_string())
}

// =============================================================================
// Numeric Validators
// =============================================================================

/// Validates a quantity value.
///
/// ## Rules
/// - Must be positive (> 0)
/// - Must not exceed MAX_ITEM_QUANTITY (999)
///
/// ## User Workflow
/// ```text
/// ┌─────────────────────────────────────────────────────────────────────────┐
/// │  Cart: Add Item                                                         │
/// │                                                                         │
/// │  User enters quantity: 5                                               │
/// │       │                                                                 │
/// │       ▼                                                                 │
/// │  validate_quantity(5) ← THIS FUNCTION                                  │
/// │       │                                                                 │
/// │       ├── qty <= 0? → Error: "Quantity must be positive"               │
/// │       │                                                                 │
/// │       ├── qty > 999? → Error: "Quantity cannot exceed 999"             │
/// │       │                                                                 │
/// │       └── OK → Proceed with add_to_cart                                │
/// │                                                                         │
/// └─────────────────────────────────────────────────────────────────────────┘
/// ```
pub fn validate_quantity(qty: i64) -> ValidationResult<()> {
    if qty <= 0 {
        return Err(ValidationError::MustBePositive {
            field: "quantity".to_string(),
        });
    }

    if qty > MAX_ITEM_QUANTITY {
        return Err(ValidationError::OutOfRange {
            field: "quantity".to_string(),
            min: 1,
            max: MAX_ITEM_QUANTITY,
        });
    }

    Ok(())
}

/// Validates a price in cents.
///
/// ## Rules
/// - Must be non-negative (>= 0)
/// - Zero is allowed (free items)
///
/// ## Example
/// ```rust
/// use titan_core::validation::validate_price_cents;
///
/// assert!(validate_price_cents(1099).is_ok());  // $10.99
/// assert!(validate_price_cents(0).is_ok());     // Free item
/// assert!(validate_price_cents(-100).is_err()); // Invalid
/// ```
pub fn validate_price_cents(cents: i64) -> ValidationResult<()> {
    if cents < 0 {
        return Err(ValidationError::OutOfRange {
            field: "price".to_string(),
            min: 0,
            max: i64::MAX,
        });
    }

    Ok(())
}

/// Validates a payment amount in cents.
///
/// ## Rules
/// - Must be positive (> 0)
/// - Cannot pay zero or negative amounts
pub fn validate_payment_amount(cents: i64) -> ValidationResult<()> {
    if cents <= 0 {
        return Err(ValidationError::MustBePositive {
            field: "payment amount".to_string(),
        });
    }

    Ok(())
}

/// Validates a tax rate in basis points.
///
/// ## Rules
/// - Must be between 0 and 10000 (0% to 100%)
/// - Most tax rates are 0-2500 (0% to 25%)
pub fn validate_tax_rate_bps(bps: u32) -> ValidationResult<()> {
    if bps > 10000 {
        return Err(ValidationError::OutOfRange {
            field: "tax_rate".to_string(),
            min: 0,
            max: 10000,
        });
    }

    Ok(())
}

// =============================================================================
// Collection Validators
// =============================================================================

/// Validates cart size (number of unique items).
///
/// ## Rules
/// - Must not exceed MAX_CART_ITEMS (100)
pub fn validate_cart_size(current_items: usize) -> ValidationResult<()> {
    if current_items >= MAX_CART_ITEMS {
        return Err(ValidationError::OutOfRange {
            field: "cart items".to_string(),
            min: 0,
            max: MAX_CART_ITEMS as i64,
        });
    }

    Ok(())
}

// =============================================================================
// UUID Validators
// =============================================================================

/// Validates a UUID string format.
///
/// ## Rules
/// - Must be a valid UUID v4 format
/// - 36 characters with hyphens: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
///
/// ## Example
/// ```rust
/// use titan_core::validation::validate_uuid;
///
/// assert!(validate_uuid("550e8400-e29b-41d4-a716-446655440000").is_ok());
/// assert!(validate_uuid("not-a-uuid").is_err());
/// ```
pub fn validate_uuid(id: &str) -> ValidationResult<()> {
    if id.trim().is_empty() {
        return Err(ValidationError::Required {
            field: "id".to_string(),
        });
    }

    // Try to parse as UUID
    uuid::Uuid::parse_str(id).map_err(|_| ValidationError::InvalidFormat {
        field: "id".to_string(),
        reason: "must be a valid UUID".to_string(),
    })?;

    Ok(())
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_sku() {
        // Valid SKUs
        assert!(validate_sku("COKE-330").is_ok());
        assert!(validate_sku("ABC123").is_ok());
        assert!(validate_sku("product_1").is_ok());

        // Invalid SKUs
        assert!(validate_sku("").is_err());
        assert!(validate_sku("   ").is_err());
        assert!(validate_sku("has space").is_err());
        assert!(validate_sku(&"A".repeat(100)).is_err());
    }

    #[test]
    fn test_validate_product_name() {
        assert!(validate_product_name("Coca-Cola 330ml").is_ok());
        assert!(validate_product_name("").is_err());
        assert!(validate_product_name(&"A".repeat(300)).is_err());
    }

    #[test]
    fn test_validate_quantity() {
        assert!(validate_quantity(1).is_ok());
        assert!(validate_quantity(100).is_ok());
        assert!(validate_quantity(999).is_ok());

        assert!(validate_quantity(0).is_err());
        assert!(validate_quantity(-1).is_err());
        assert!(validate_quantity(1000).is_err());
    }

    #[test]
    fn test_validate_price_cents() {
        assert!(validate_price_cents(0).is_ok());
        assert!(validate_price_cents(1099).is_ok());
        assert!(validate_price_cents(-100).is_err());
    }

    #[test]
    fn test_validate_uuid() {
        assert!(validate_uuid("550e8400-e29b-41d4-a716-446655440000").is_ok());
        assert!(validate_uuid("").is_err());
        assert!(validate_uuid("not-a-uuid").is_err());
        assert!(validate_uuid("123").is_err());
    }

    #[test]
    fn test_validate_tax_rate_bps() {
        assert!(validate_tax_rate_bps(0).is_ok());
        assert!(validate_tax_rate_bps(825).is_ok());
        assert!(validate_tax_rate_bps(10000).is_ok());
        assert!(validate_tax_rate_bps(10001).is_err());
    }
}
