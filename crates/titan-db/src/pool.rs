//! # Database Pool Management
//!
//! Connection pool creation and configuration for SQLite.
//!
//! ## Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                      Database Connection Pool                           │
//! │                                                                         │
//! │  Tauri App Startup                                                     │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  DbConfig::new(path) ← Configure pool settings                         │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  Database::new(config).await ← Create pool + run migrations            │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  ┌─────────────────────────────────────────┐                           │
//! │  │            SqlitePool                    │                           │
//! │  │  ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐       │                           │
//! │  │  │Conn1│ │Conn2│ │Conn3│ │Conn4│ ...   │  (max_connections)        │
//! │  │  └─────┘ └─────┘ └─────┘ └─────┘       │                           │
//! │  └─────────────────────────────────────────┘                           │
//! │       │                                                                 │
//! │       │ Concurrent access from Tauri commands                          │
//! │       ▼                                                                 │
//! │  Command 1 ──► uses Conn1                                              │
//! │  Command 2 ──► uses Conn2                                              │
//! │  Command 3 ──► uses Conn3                                              │
//! │  (Commands can run in parallel with different connections)             │
//! │                                                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## WAL Mode
//! SQLite WAL (Write-Ahead Logging) mode is enabled for:
//! - Better concurrent read performance
//! - Readers don't block writers
//! - Writers don't block readers
//! - Better crash recovery

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use tracing::{debug, info};

use crate::error::{DbError, DbResult};
use crate::migrations;
use crate::repository::product::ProductRepository;
use crate::repository::sale::SaleRepository;
use crate::repository::sync::SyncOutboxRepository;

// =============================================================================
// Configuration
// =============================================================================

/// Database configuration.
///
/// ## Example
/// ```rust,ignore
/// let config = DbConfig::new("/path/to/titan.db")
///     .max_connections(5)
///     .min_connections(1);
/// ```
#[derive(Debug, Clone)]
pub struct DbConfig {
    /// Path to the SQLite database file.
    pub database_path: PathBuf,

    /// Maximum number of connections in the pool.
    /// Default: 5 (sufficient for a local POS app)
    pub max_connections: u32,

    /// Minimum number of connections to keep alive.
    /// Default: 1
    pub min_connections: u32,

    /// Connection timeout duration.
    /// Default: 30 seconds
    pub connect_timeout: Duration,

    /// Idle timeout before closing a connection.
    /// Default: 10 minutes
    pub idle_timeout: Duration,

    /// Whether to run migrations on connect.
    /// Default: true
    pub run_migrations: bool,
}

impl DbConfig {
    /// Creates a new database configuration with the given path.
    ///
    /// ## Arguments
    /// * `path` - Path to the SQLite database file. Will be created if it doesn't exist.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let config = DbConfig::new("./data/titan.db");
    /// ```
    pub fn new(path: impl Into<PathBuf>) -> Self {
        DbConfig {
            database_path: path.into(),
            max_connections: 5,
            min_connections: 1,
            connect_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(600),
            run_migrations: true,
        }
    }

    /// Sets the maximum number of connections.
    pub fn max_connections(mut self, max: u32) -> Self {
        self.max_connections = max;
        self
    }

    /// Sets the minimum number of connections.
    pub fn min_connections(mut self, min: u32) -> Self {
        self.min_connections = min;
        self
    }

    /// Sets the connection timeout.
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Sets whether to run migrations on connect.
    pub fn run_migrations(mut self, run: bool) -> Self {
        self.run_migrations = run;
        self
    }

    /// Creates an in-memory database configuration (for testing).
    ///
    /// ## Usage
    /// ```rust,ignore
    /// let config = DbConfig::in_memory();
    /// let db = Database::new(config).await?;
    /// // Database is isolated, perfect for tests
    /// ```
    pub fn in_memory() -> Self {
        DbConfig {
            database_path: PathBuf::from(":memory:"),
            max_connections: 1, // In-memory requires single connection
            min_connections: 1,
            connect_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(60),
            run_migrations: true,
        }
    }
}

// =============================================================================
// Database
// =============================================================================

/// Main database handle providing repository access.
///
/// ## Design: Multiple State Types (Option B)
/// ```text
/// ┌─────────────────────────────────────────────────────────────────────────┐
/// │  Tauri State Management                                                 │
/// │                                                                         │
/// │  Instead of one big AppState, we have separate state types:            │
/// │                                                                         │
/// │  State<'_, Database>       ← Database operations                       │
/// │  State<'_, CartState>      ← Cart management (future)                  │
/// │  State<'_, ConfigState>    ← App configuration (future)                │
/// │                                                                         │
/// │  Benefits:                                                              │
/// │  • Fine-grained access control                                         │
/// │  • Commands only get what they need                                    │
/// │  • Easier testing (mock only what's needed)                            │
/// │  • Clear separation of concerns                                        │
/// └─────────────────────────────────────────────────────────────────────────┘
/// ```
///
/// ## Usage in Tauri Commands
/// ```rust,ignore
/// #[tauri::command]
/// async fn search_products(
///     db: State<'_, Database>,  // Only gets database access
///     query: String,
/// ) -> Result<Vec<ProductDto>, ApiError> {
///     let products = db.products().search(&query, 20).await?;
///     Ok(products.into_iter().map(ProductDto::from).collect())
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Database {
    /// The SQLite connection pool.
    pool: SqlitePool,
}

impl Database {
    /// Creates a new database connection pool.
    ///
    /// ## What This Does
    /// 1. Creates the database file if it doesn't exist
    /// 2. Configures SQLite for optimal POS performance:
    ///    - WAL mode for concurrent reads
    ///    - NORMAL synchronous (balance of safety/speed)
    ///    - Foreign keys enabled
    /// 3. Creates the connection pool
    /// 4. Runs migrations (if enabled)
    ///
    /// ## Arguments
    /// * `config` - Database configuration
    ///
    /// ## Returns
    /// * `Ok(Database)` - Ready-to-use database handle
    /// * `Err(DbError)` - Connection or migration failed
    ///
    /// ## Example
    /// ```rust,ignore
    /// let config = DbConfig::new("./titan.db");
    /// let db = Database::new(config).await?;
    /// ```
    pub async fn new(config: DbConfig) -> DbResult<Self> {
        info!(
            path = %config.database_path.display(),
            "Initializing database connection"
        );

        // Build connection options
        // sqlite://path creates file if not exists
        let connect_url = format!("sqlite://{}?mode=rwc", config.database_path.display());

        let connect_options = SqliteConnectOptions::from_str(&connect_url)
            .map_err(|e| DbError::ConnectionFailed(e.to_string()))?
            // WAL mode: Better concurrent read performance
            // Readers don't block writers, writers don't block readers
            .journal_mode(SqliteJournalMode::Wal)
            // NORMAL synchronous: Good balance of durability and speed
            // Data is safe from corruption, may lose last transaction on crash
            .synchronous(SqliteSynchronous::Normal)
            // Enable foreign key constraints
            // SQLite has them disabled by default for backwards compatibility
            .foreign_keys(true)
            // Create file if it doesn't exist
            .create_if_missing(true);

        debug!("Connection options configured");

        // Build the pool
        let pool = SqlitePoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(config.min_connections)
            .acquire_timeout(config.connect_timeout)
            .idle_timeout(Some(config.idle_timeout))
            .connect_with(connect_options)
            .await
            .map_err(|e| DbError::ConnectionFailed(e.to_string()))?;

        info!(
            max_connections = config.max_connections,
            "Database pool created"
        );

        let db = Database { pool };

        // Run migrations if enabled
        if config.run_migrations {
            db.run_migrations().await?;
        }

        Ok(db)
    }

    /// Runs database migrations.
    ///
    /// ## What This Does
    /// - Applies all pending migrations in order
    /// - Tracks applied migrations in `_sqlx_migrations` table
    /// - Idempotent: safe to run multiple times
    ///
    /// ## When To Call
    /// - Automatically called by `new()` if `run_migrations` is true
    /// - Manually call when migrations are disabled in config
    pub async fn run_migrations(&self) -> DbResult<()> {
        info!("Running database migrations");
        migrations::run_migrations(&self.pool).await?;
        info!("Migrations complete");
        Ok(())
    }

    /// Returns a reference to the connection pool.
    ///
    /// ## Usage
    /// For advanced queries not covered by repositories.
    /// Prefer using repository methods when available.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Returns the product repository.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let products = db.products().search("coke", 20).await?;
    /// ```
    pub fn products(&self) -> ProductRepository {
        ProductRepository::new(self.pool.clone())
    }

    /// Returns the sale repository.
    pub fn sales(&self) -> SaleRepository {
        SaleRepository::new(self.pool.clone())
    }

    /// Returns the sync outbox repository.
    pub fn sync_outbox(&self) -> SyncOutboxRepository {
        SyncOutboxRepository::new(self.pool.clone())
    }

    /// Closes the database connection pool.
    ///
    /// ## When To Call
    /// - On application shutdown
    /// - When switching databases (rare)
    ///
    /// ## Note
    /// After calling close, all repository operations will fail.
    pub async fn close(&self) {
        info!("Closing database connection pool");
        self.pool.close().await;
    }

    /// Checks if the database is healthy (can execute queries).
    ///
    /// ## Returns
    /// * `true` - Database is responsive
    /// * `false` - Database is unavailable
    pub async fn health_check(&self) -> bool {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .is_ok()
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_database() {
        let config = DbConfig::in_memory();
        let db = Database::new(config).await.unwrap();

        assert!(db.health_check().await);
    }

    #[tokio::test]
    async fn test_config_builder() {
        let config = DbConfig::new("/tmp/test.db")
            .max_connections(10)
            .min_connections(2);

        assert_eq!(config.max_connections, 10);
        assert_eq!(config.min_connections, 2);
    }
}
