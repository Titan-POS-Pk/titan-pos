//! # Database Error Types
//!
//! Error types for database operations.
//!
//! ## Error Flow
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Error Propagation                                    │
//! │                                                                         │
//! │  SQLite Error (sqlx::Error)                                            │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  DbError (this module) ← Adds context and categorization               │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  ApiError (in Tauri app) ← Serialized for frontend                     │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  Frontend displays user-friendly message                               │
//! │                                                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use thiserror::Error;

/// Database operation errors.
///
/// These errors wrap sqlx errors and provide additional context
/// for debugging and user feedback.
#[derive(Debug, Error)]
pub enum DbError {
    /// Entity not found in database.
    ///
    /// ## When This Occurs
    /// - `fetch_one` returns no rows
    /// - ID doesn't exist
    /// - Soft-deleted record
    #[error("{entity} not found: {id}")]
    NotFound {
        entity: String,
        id: String,
    },

    /// Unique constraint violation.
    ///
    /// ## When This Occurs
    /// - Inserting duplicate SKU
    /// - Duplicate receipt number
    /// - Any UNIQUE index violation
    #[error("Duplicate {field}: '{value}' already exists")]
    UniqueViolation {
        field: String,
        value: String,
    },

    /// Foreign key constraint violation.
    ///
    /// ## When This Occurs
    /// - Referencing non-existent product_id
    /// - Referencing non-existent sale_id
    #[error("Foreign key violation: {message}")]
    ForeignKeyViolation {
        message: String,
    },

    /// Database connection failed.
    ///
    /// ## When This Occurs
    /// - Database file doesn't exist and can't be created
    /// - File permissions issue
    /// - Disk full
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Migration failed.
    ///
    /// ## When This Occurs
    /// - Invalid SQL in migration
    /// - Migration version conflict
    /// - Schema incompatibility
    #[error("Migration failed: {0}")]
    MigrationFailed(String),

    /// Query execution failed.
    ///
    /// ## When This Occurs
    /// - SQL syntax error (shouldn't happen with sqlx compile-time checks)
    /// - Runtime SQL error
    #[error("Query failed: {0}")]
    QueryFailed(String),

    /// Transaction failed.
    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    /// Pool exhausted (all connections in use).
    #[error("Connection pool exhausted")]
    PoolExhausted,

    /// Internal database error.
    #[error("Internal database error: {0}")]
    Internal(String),
}

impl DbError {
    /// Creates a NotFound error for a given entity type and ID.
    pub fn not_found(entity: impl Into<String>, id: impl Into<String>) -> Self {
        DbError::NotFound {
            entity: entity.into(),
            id: id.into(),
        }
    }

    /// Creates a UniqueViolation error.
    pub fn duplicate(field: impl Into<String>, value: impl Into<String>) -> Self {
        DbError::UniqueViolation {
            field: field.into(),
            value: value.into(),
        }
    }
}

/// Convert sqlx errors to DbError.
///
/// ## Error Mapping
/// ```text
/// sqlx::Error::RowNotFound    → DbError::NotFound
/// sqlx::Error::Database       → Analyze message for constraint type
/// sqlx::Error::PoolTimedOut   → DbError::PoolExhausted
/// Other                       → DbError::Internal
/// ```
impl From<sqlx::Error> for DbError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => DbError::NotFound {
                entity: "Record".to_string(),
                id: "unknown".to_string(),
            },

            sqlx::Error::Database(db_err) => {
                let msg = db_err.message();

                // SQLite error codes for constraints:
                // UNIQUE constraint: "UNIQUE constraint failed: <table>.<column>"
                // FK constraint: "FOREIGN KEY constraint failed"
                if msg.contains("UNIQUE constraint failed") {
                    // Parse the field name from the error message
                    let field = msg
                        .split("UNIQUE constraint failed: ")
                        .nth(1)
                        .unwrap_or("unknown")
                        .to_string();
                    DbError::UniqueViolation {
                        field,
                        value: "unknown".to_string(),
                    }
                } else if msg.contains("FOREIGN KEY constraint failed") {
                    DbError::ForeignKeyViolation {
                        message: msg.to_string(),
                    }
                } else {
                    DbError::QueryFailed(msg.to_string())
                }
            }

            sqlx::Error::PoolTimedOut => DbError::PoolExhausted,

            sqlx::Error::PoolClosed => DbError::ConnectionFailed("Pool is closed".to_string()),

            _ => DbError::Internal(err.to_string()),
        }
    }
}

impl From<sqlx::migrate::MigrateError> for DbError {
    fn from(err: sqlx::migrate::MigrateError) -> Self {
        DbError::MigrationFailed(err.to_string())
    }
}

/// Result type for database operations.
pub type DbResult<T> = Result<T, DbError>;
