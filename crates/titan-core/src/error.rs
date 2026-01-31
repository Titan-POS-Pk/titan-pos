//! # Error Types
//!
//! Domain-specific error types for titan-core.
//!
//! ## Error Hierarchy
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         Error Types                                     │
//! │                                                                         │
//! │  titan-core errors (this file)                                         │
//! │  ├── CoreError        - General domain errors                          │
//! │  └── ValidationError  - Input validation failures                      │
//! │                                                                         │
//! │  titan-db errors (separate crate)                                      │
//! │  └── DbError          - Database operation failures                    │
//! │                                                                         │
//! │  Tauri API errors (in app)                                             │
//! │  └── ApiError         - What frontend sees (serialized)                │
//! │                                                                         │
//! │  Flow: ValidationError → CoreError → DbError → ApiError → Frontend     │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Design Principles
//! 1. Use `thiserror` for derive macros (not manual impl)
//! 2. Include context in error messages (SKU, ID, etc.)
//! 3. Errors are enum variants, never String
//! 4. Each error variant maps to a user-facing message

use thiserror::Error;

// =============================================================================
// Core Error
// =============================================================================

/// Core business logic errors.
///
/// These errors represent business rule violations or domain logic failures.
/// They should be caught and translated to user-friendly messages.
#[derive(Debug, Error)]
pub enum CoreError {
    /// Product cannot be found.
    /// 
    /// ## When This Occurs
    /// - Product ID doesn't exist in database
    /// - Product was deleted (soft delete)
    /// - Sync conflict where product exists on one device but not another
    #[error("Product not found: {0}")]
    ProductNotFound(String),

    /// Insufficient stock to complete sale.
    ///
    /// ## When This Occurs
    /// - Trying to sell more than available stock
    /// - Product has track_inventory=true and allow_negative_stock=false
    ///
    /// ## User Workflow
    /// ```text
    /// Add to Cart (qty: 5)
    ///      │
    ///      ▼
    /// Check stock: available=3
    ///      │
    ///      ▼
    /// InsufficientStock { sku: "COKE", available: 3, requested: 5 }
    ///      │
    ///      ▼
    /// UI shows: "Only 3 COKE in stock"
    /// ```
    #[error("Insufficient stock for {sku}: available {available}, requested {requested}")]
    InsufficientStock {
        sku: String,
        available: i64,
        requested: i64,
    },

    /// Sale not found.
    #[error("Sale not found: {0}")]
    SaleNotFound(String),

    /// Sale is not in a state that allows the requested operation.
    ///
    /// ## When This Occurs
    /// - Trying to add items to a completed sale
    /// - Trying to void an already voided sale
    /// - Trying to finalize a sale that's already completed
    #[error("Sale {sale_id} is {current_status:?}, cannot perform operation")]
    InvalidSaleStatus {
        sale_id: String,
        current_status: String,
    },

    /// Cart has exceeded maximum allowed items.
    #[error("Cart cannot have more than {max} items")]
    CartTooLarge { max: usize },

    /// Item quantity exceeds maximum allowed.
    #[error("Quantity {requested} exceeds maximum allowed ({max})")]
    QuantityTooLarge { requested: i64, max: i64 },

    /// Payment amount is invalid.
    #[error("Invalid payment amount: {reason}")]
    InvalidPaymentAmount { reason: String },

    /// Validation error (wraps ValidationError).
    #[error("Validation error: {0}")]
    Validation(#[from] ValidationError),
}

// =============================================================================
// Validation Error
// =============================================================================

/// Input validation errors.
///
/// These errors occur when user input doesn't meet requirements.
/// Used for early validation before business logic runs.
#[derive(Debug, Error)]
pub enum ValidationError {
    /// A required field is missing or empty.
    #[error("{field} is required")]
    Required { field: String },

    /// Field value is too short.
    #[error("{field} must be at least {min} characters")]
    TooShort { field: String, min: usize },

    /// Field value is too long.
    #[error("{field} must be at most {max} characters")]
    TooLong { field: String, max: usize },

    /// Numeric value is out of range.
    #[error("{field} must be between {min} and {max}")]
    OutOfRange { field: String, min: i64, max: i64 },

    /// Value must be positive.
    #[error("{field} must be positive")]
    MustBePositive { field: String },

    /// Invalid format (e.g., invalid UUID, invalid date).
    #[error("{field} has invalid format: {reason}")]
    InvalidFormat { field: String, reason: String },

    /// Value is not in allowed set.
    #[error("{field} must be one of: {allowed:?}")]
    NotAllowed { field: String, allowed: Vec<String> },

    /// Duplicate value (e.g., duplicate SKU).
    #[error("{field} '{value}' already exists")]
    Duplicate { field: String, value: String },
}

// =============================================================================
// Result Type Alias
// =============================================================================

/// Convenience type alias for Results with CoreError.
pub type CoreResult<T> = Result<T, CoreError>;

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_messages() {
        let err = CoreError::InsufficientStock {
            sku: "COKE-330".to_string(),
            available: 3,
            requested: 5,
        };
        assert_eq!(
            err.to_string(),
            "Insufficient stock for COKE-330: available 3, requested 5"
        );
    }

    #[test]
    fn test_validation_error_messages() {
        let err = ValidationError::Required {
            field: "sku".to_string(),
        };
        assert_eq!(err.to_string(), "sku is required");

        let err = ValidationError::TooShort {
            field: "name".to_string(),
            min: 3,
        };
        assert_eq!(err.to_string(), "name must be at least 3 characters");
    }

    #[test]
    fn test_validation_converts_to_core_error() {
        let validation_err = ValidationError::Required {
            field: "sku".to_string(),
        };
        let core_err: CoreError = validation_err.into();
        assert!(matches!(core_err, CoreError::Validation(_)));
    }
}
