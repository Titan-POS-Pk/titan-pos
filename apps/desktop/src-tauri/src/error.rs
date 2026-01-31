//! # API Error Type
//!
//! Unified error type for Tauri commands.
//!
//! ## Error Handling Strategy
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Error Flow in Titan POS                              │
//! │                                                                         │
//! │  Frontend                    Rust Backend                               │
//! │  ────────                    ────────────                               │
//! │                                                                         │
//! │  invoke('search_products')                                              │
//! │         │                                                               │
//! │         ▼                                                               │
//! │  ┌──────────────────────────────────────────────────────────────────┐  │
//! │  │  Command Function                                                │  │
//! │  │  Result<T, ApiError>                                             │  │
//! │  │         │                                                        │  │
//! │  │         ▼                                                        │  │
//! │  │  Database Error? ─── DbError::QueryFailed("...") ──┐            │  │
//! │  │         │                                          │            │  │
//! │  │         ▼                                          ▼            │  │
//! │  │  Validation Error? ─── CoreError::InvalidInput ── ApiError ────►│  │
//! │  │         │                                                        │  │
//! │  │         ▼                                                        │  │
//! │  │  Success ──────────────────────────────────────────────────────►│  │
//! │  └──────────────────────────────────────────────────────────────────┘  │
//! │                                                                         │
//! │  ◄────────────────────────────────────────────────────────────────────  │
//! │                                                                         │
//! │  try {                                                                  │
//! │    await invoke('search_products')                                      │
//! │  } catch (e) {                                                          │
//! │    // e.message = "Product not found: ABC-123"                          │
//! │    // e.code = "NOT_FOUND"                                              │
//! │  }                                                                      │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Tauri Error Serialization
//! Tauri requires errors to be serializable. We implement `Serialize`
//! and include both a machine-readable `code` and human-readable `message`.

use serde::Serialize;
use titan_core::CoreError;
use titan_db::DbError;

/// API error returned from Tauri commands.
///
/// ## Serialization
/// This is what the frontend receives when a command fails:
/// ```json
/// {
///   "code": "NOT_FOUND",
///   "message": "Product not found: SKU-123"
/// }
/// ```
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    /// Machine-readable error code for programmatic handling
    pub code: ErrorCode,

    /// Human-readable error message for display
    pub message: String,
}

/// Error codes for API responses.
///
/// ## Usage in Frontend
/// ```typescript
/// try {
///   await invoke('get_product', { id });
/// } catch (e) {
///   switch (e.code) {
///     case 'NOT_FOUND':
///       showNotification('Product not found');
///       break;
///     case 'VALIDATION_ERROR':
///       showForm(e.message);
///       break;
///     default:
///       showError('An error occurred');
///   }
/// }
/// ```
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// Resource not found (404)
    NotFound,

    /// Input validation failed (400)
    ValidationError,

    /// Database operation failed (500)
    DatabaseError,

    /// Business logic error (422)
    BusinessLogic,

    /// Internal server error (500)
    Internal,

    /// Cart operation failed
    CartError,

    /// Insufficient stock
    InsufficientStock,

    /// Payment processing error
    PaymentError,
}

impl ApiError {
    /// Creates a new API error.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        ApiError {
            code,
            message: message.into(),
        }
    }

    /// Creates a not found error.
    pub fn not_found(resource: &str, id: &str) -> Self {
        ApiError::new(
            ErrorCode::NotFound,
            format!("{} not found: {}", resource, id),
        )
    }

    /// Creates a validation error.
    pub fn validation(message: impl Into<String>) -> Self {
        ApiError::new(ErrorCode::ValidationError, message)
    }

    /// Creates an internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        ApiError::new(ErrorCode::Internal, message)
    }

    /// Creates a cart error.
    pub fn cart(message: impl Into<String>) -> Self {
        ApiError::new(ErrorCode::CartError, message)
    }
}

/// Converts database errors to API errors.
impl From<DbError> for ApiError {
    fn from(err: DbError) -> Self {
        match err {
            DbError::NotFound { entity, id } => ApiError::not_found(&entity, &id),
            DbError::UniqueViolation { field, value } => ApiError::new(
                ErrorCode::ValidationError,
                format!("{} '{}' already exists", field, value),
            ),
            DbError::ConnectionFailed(_) => {
                ApiError::new(ErrorCode::DatabaseError, "Database connection failed")
            }
            DbError::MigrationFailed(_) => {
                ApiError::new(ErrorCode::DatabaseError, "Database migration failed")
            }
            DbError::QueryFailed(e) => {
                // Log the actual error but return a generic message
                tracing::error!("Database query failed: {}", e);
                ApiError::new(ErrorCode::DatabaseError, "Database operation failed")
            }
            DbError::TransactionFailed(e) => {
                tracing::error!("Transaction failed: {}", e);
                ApiError::new(ErrorCode::DatabaseError, "Database transaction failed")
            }
            DbError::ForeignKeyViolation { message } => {
                tracing::error!("Foreign key violation: {}", message);
                ApiError::new(ErrorCode::ValidationError, "Invalid reference")
            }
            DbError::PoolExhausted => {
                ApiError::new(ErrorCode::DatabaseError, "Database pool exhausted")
            }
            DbError::Internal(e) => {
                tracing::error!("Internal database error: {}", e);
                ApiError::new(ErrorCode::DatabaseError, "Database operation failed")
            }
        }
    }
}

/// Converts core errors to API errors.
impl From<CoreError> for ApiError {
    fn from(err: CoreError) -> Self {
        match err {
            CoreError::ProductNotFound(id) => ApiError::not_found("Product", &id),
            CoreError::SaleNotFound(id) => ApiError::not_found("Sale", &id),
            CoreError::InsufficientStock {
                sku,
                available,
                requested,
            } => ApiError::new(
                ErrorCode::InsufficientStock,
                format!(
                    "Insufficient stock for {}: {} available, {} requested",
                    sku, available, requested
                ),
            ),
            CoreError::InvalidSaleStatus { sale_id, current_status } => ApiError::new(
                ErrorCode::BusinessLogic,
                format!("Sale {} is in {} status", sale_id, current_status),
            ),
            CoreError::CartTooLarge { max } => ApiError::new(
                ErrorCode::CartError,
                format!("Cart cannot have more than {} items", max),
            ),
            CoreError::QuantityTooLarge { requested, max } => ApiError::new(
                ErrorCode::ValidationError,
                format!("Quantity {} exceeds maximum allowed ({})", requested, max),
            ),
            CoreError::InvalidPaymentAmount { reason } => ApiError::new(
                ErrorCode::PaymentError,
                format!("Invalid payment amount: {}", reason),
            ),
            CoreError::Validation(e) => ApiError::validation(e.to_string()),
        }
    }
}

/// Makes ApiError work as a Tauri command error.
///
/// Tauri requires the error type to implement `Into<tauri::ipc::InvokeError>`.
/// Since we implement `Serialize`, we can convert to JSON string.
impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}
