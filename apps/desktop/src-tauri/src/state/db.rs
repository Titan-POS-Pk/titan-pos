//! # Database State
//!
//! Wraps the `Database` connection for use in Tauri commands.
//!
//! ## Thread Safety
//! The `Database` struct from `titan-db` contains a `SqlitePool` which
//! is inherently thread-safe. Multiple commands can execute queries
//! concurrently without explicit locking.
//!
//! ## Usage in Commands
//! ```rust,ignore
//! #[tauri::command]
//! async fn search_products(
//!     db: State<'_, DbState>,
//!     query: String,
//! ) -> Result<Vec<ProductDto>, ApiError> {
//!     let products = db.inner().products().search(&query, 20).await?;
//!     Ok(products.into_iter().map(ProductDto::from).collect())
//! }
//! ```

use titan_db::Database;

/// Wrapper around `Database` for Tauri state management.
///
/// ## Why a Wrapper?
/// Tauri's state management requires types to implement `Send + Sync`.
/// This wrapper makes the intent explicit and provides a clean API
/// for accessing the database in commands.
#[derive(Debug)]
pub struct DbState {
    db: Database,
}

impl DbState {
    /// Creates a new DbState wrapping the database connection.
    pub fn new(db: Database) -> Self {
        DbState { db }
    }

    /// Returns a reference to the inner Database.
    ///
    /// ## Usage
    /// ```rust,ignore
    /// let products = db_state.inner().products().search("query", 20).await?;
    /// ```
    pub fn inner(&self) -> &Database {
        &self.db
    }
}
