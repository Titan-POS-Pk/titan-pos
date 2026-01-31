//! # Database Migrations
//!
//! Embedded SQL migrations for Titan POS.
//!
//! ## How Migrations Work
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                      Migration Process                                  │
//! │                                                                         │
//! │  App Startup                                                           │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  Check _sqlx_migrations table                                          │
//! │       │                                                                 │
//! │       ├── Table doesn't exist? Create it                               │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  Compare embedded migrations vs applied                                │
//! │       │                                                                 │
//! │       ├── 001_initial_schema.sql ✓ (already applied)                  │
//! │       ├── 002_add_fts.sql        ✓ (already applied)                  │
//! │       └── 003_add_indexes.sql    ⬜ (NEW - needs to run)               │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  Run pending migrations in order                                       │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  Record in _sqlx_migrations                                            │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  App continues startup                                                 │
//! │                                                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Adding New Migrations
//!
//! 1. Create a new file in `migrations/sqlite/` with the next sequence number
//! 2. Name format: `NNN_description.sql` (e.g., `004_add_customer_table.sql`)
//! 3. Write idempotent SQL (use `IF NOT EXISTS` where possible)
//! 4. **NEVER** modify existing migrations - always add new ones
//! 5. Run `cargo sqlx prepare` to update offline query data

use sqlx::SqlitePool;
use tracing::info;

use crate::error::DbResult;

/// Embedded migrations from the `migrations/sqlite` directory.
///
/// ## How This Works
/// The `sqlx::migrate!()` macro embeds all SQL files from the specified
/// directory into the binary at compile time. No runtime file access needed.
///
/// ## Directory Structure
/// ```
/// migrations/sqlite/
/// ├── 001_initial_schema.sql  # Core tables
/// ├── 002_add_fts.sql         # Full-text search
/// └── ...
/// ```
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations/sqlite");

/// Runs all pending database migrations.
///
/// ## What This Does
/// - Creates `_sqlx_migrations` table if not exists
/// - Compares embedded migrations with applied migrations
/// - Runs any pending migrations in order
/// - Records each migration's checksum and timestamp
///
/// ## Safety
/// - Idempotent: safe to run multiple times
/// - Transactional: each migration runs in a transaction
/// - Ordered: migrations run in filename order (001, 002, ...)
///
/// ## Example
/// ```rust,ignore
/// run_migrations(&pool).await?;
/// ```
pub async fn run_migrations(pool: &SqlitePool) -> DbResult<()> {
    info!("Checking for pending migrations");

    MIGRATOR.run(pool).await?;

    info!("All migrations applied successfully");
    Ok(())
}

/// Returns information about migrations.
///
/// ## Returns
/// Tuple of (total_migrations, applied_migrations)
///
/// ## Usage
/// For diagnostics and health checks.
pub async fn migration_status(pool: &SqlitePool) -> DbResult<(usize, usize)> {
    let total = MIGRATOR.migrations.len();

    // Query applied migrations
    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    Ok((total, applied as usize))
}
