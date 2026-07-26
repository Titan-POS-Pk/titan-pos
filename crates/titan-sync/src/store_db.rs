//! # Store Database Module
//!
//! Manages the separate store-level aggregation database (`store_aggregates.db`).
//!
//! ## Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Store Database Architecture                          │
//! │                                                                         │
//! │  The store database is a SEPARATE SQLite file from the main titan.db.  │
//! │  This separation provides:                                              │
//! │                                                                         │
//! │  1. ISOLATION: Aggregation doesn't impact main POS performance         │
//! │  2. BACKUP: Can backup store data independently                        │
//! │  3. PORTABILITY: Can be synced to cloud without main DB                │
//! │  4. FAILURE DOMAIN: Store DB issues don't affect local sales           │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │  titan.db (Main Database)                                       │   │
//! │  │  • products, sales, payments                                    │   │
//! │  │  • sync_outbox, inventory_deltas                                │   │
//! │  │  • Local POS operations                                         │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                              │                                          │
//! │                              │ Aggregated by PRIMARY                    │
//! │                              ▼                                          │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │  store_aggregates.db (This Module)                              │   │
//! │  │  • inventory_snapshots                                          │   │
//! │  │  • sales_summaries                                              │   │
//! │  │  • payment_summaries                                            │   │
//! │  │  • device_activity                                              │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                              │                                          │
//! │                              │ Uploaded by PRIMARY                      │
//! │                              ▼                                          │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │  Cloud PostgreSQL                                               │   │
//! │  │  • Multi-tenant store data                                      │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use std::path::{Path, PathBuf};

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use sqlx::Row;
use tracing::{debug, info};

use crate::error::{SyncError, SyncResult};
use crate::protocol::{AggregationSummary, InventoryMover, PaymentSummary, SalesSummary};

/// Schema for the store aggregates database.
/// This is embedded at compile time from the migration file.
const STORE_SCHEMA: &str =
    include_str!("../../../migrations/sqlite/004_store_aggregates_schema.sql");

// =============================================================================
// Store Database
// =============================================================================

/// Store-level aggregation database.
///
/// This database is maintained by the PRIMARY (Store Hub) and contains
/// aggregated data from all connected POS devices.
#[derive(Clone)]
pub struct StoreDatabase {
    pool: SqlitePool,
    store_id: String,
    path: PathBuf,
}

impl StoreDatabase {
    /// Creates or opens the store database at the given path.
    ///
    /// # Arguments
    /// * `path` - Path to the database file
    /// * `store_id` - Store identifier
    pub async fn open(path: impl AsRef<Path>, store_id: &str) -> SyncResult<Self> {
        let path = path.as_ref().to_path_buf();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        info!(?path, store_id, "Opening store aggregates database");

        // Configure SQLite connection
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .journal_mode(SqliteJournalMode::Wal)
            .create_if_missing(true)
            .foreign_keys(true);

        // Create connection pool
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(|e| {
                SyncError::DatabaseError(format!("Failed to open store database: {}", e))
            })?;

        // Run schema migration
        Self::run_schema(&pool).await?;

        // Initialize store info
        Self::init_store_info(&pool, store_id).await?;

        Ok(StoreDatabase {
            pool,
            store_id: store_id.to_string(),
            path,
        })
    }

    /// Runs the schema migration (idempotent).
    async fn run_schema(pool: &SqlitePool) -> SyncResult<()> {
        debug!("Running store database schema");

        // Execute schema SQL (CREATE IF NOT EXISTS makes this idempotent)
        sqlx::raw_sql(STORE_SCHEMA)
            .execute(pool)
            .await
            .map_err(|e| SyncError::DatabaseError(format!("Schema migration failed: {}", e)))?;

        info!("Store database schema applied successfully");
        Ok(())
    }

    /// Initializes store info if not exists.
    async fn init_store_info(pool: &SqlitePool, store_id: &str) -> SyncResult<()> {
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO store_info (store_id, store_name, tenant_id)
            VALUES (?, ?, ?)
            "#,
        )
        .bind(store_id)
        .bind(format!("Store {}", store_id))
        .bind("default-tenant")
        .execute(pool)
        .await
        .map_err(|e| SyncError::DatabaseError(format!("Failed to init store info: {}", e)))?;

        Ok(())
    }

    /// Returns the store ID.
    pub fn store_id(&self) -> &str {
        &self.store_id
    }

    /// Returns the database path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    // =========================================================================
    // Inventory Operations
    // =========================================================================

    /// Records an inventory snapshot.
    pub async fn record_inventory_snapshot(
        &self,
        product_id: &str,
        sku: &str,
        product_name: Option<&str>,
        quantity: i32,
        delta: i32,
        delta_source: Option<&str>,
    ) -> SyncResult<()> {
        sqlx::query(
            r#"
            INSERT INTO inventory_snapshots (
                store_id, product_id, sku, product_name, quantity, delta, delta_source
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&self.store_id)
        .bind(product_id)
        .bind(sku)
        .bind(product_name)
        .bind(quantity)
        .bind(delta)
        .bind(delta_source)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            SyncError::DatabaseError(format!("Failed to record inventory snapshot: {}", e))
        })?;

        Ok(())
    }

    /// Gets the current inventory for a product.
    pub async fn get_current_inventory(&self, product_id: &str) -> SyncResult<Option<i32>> {
        let result = sqlx::query(
            r#"
            SELECT quantity FROM inventory_snapshots
            WHERE store_id = ? AND product_id = ?
            ORDER BY snapshot_at DESC
            LIMIT 1
            "#,
        )
        .bind(&self.store_id)
        .bind(product_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SyncError::DatabaseError(format!("Failed to get inventory: {}", e)))?;

        Ok(result.map(|row| row.get("quantity")))
    }

    /// Gets all current inventory levels.
    pub async fn get_all_current_inventory(&self) -> SyncResult<Vec<(String, String, i32)>> {
        let rows = sqlx::query(
            r#"
            SELECT product_id, sku, quantity
            FROM current_inventory
            WHERE store_id = ?
            "#,
        )
        .bind(&self.store_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SyncError::DatabaseError(format!("Failed to get all inventory: {}", e)))?;

        Ok(rows
            .iter()
            .map(|row| (row.get("product_id"), row.get("sku"), row.get("quantity")))
            .collect())
    }

    // =========================================================================
    // Sales Aggregation Operations
    // =========================================================================

    /// Records or updates a sales summary for a period.
    pub async fn upsert_sales_summary(
        &self,
        period_type: &str,
        period_start: &str,
        period_end: &str,
        sale_count: i32,
        item_count: i32,
        gross_cents: i64,
        tax_cents: i64,
        net_cents: i64,
    ) -> SyncResult<()> {
        let avg_cents = if sale_count > 0 {
            gross_cents / sale_count as i64
        } else {
            0
        };

        sqlx::query(
            r#"
            INSERT INTO sales_summaries (
                store_id, period_type, period_start, period_end,
                sale_count, item_count, gross_total_cents, tax_total_cents, net_total_cents,
                avg_transaction_cents, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))
            ON CONFLICT(store_id, period_type, period_start) DO UPDATE SET
                sale_count = sale_count + excluded.sale_count,
                item_count = item_count + excluded.item_count,
                gross_total_cents = gross_total_cents + excluded.gross_total_cents,
                tax_total_cents = tax_total_cents + excluded.tax_total_cents,
                net_total_cents = net_total_cents + excluded.net_total_cents,
                avg_transaction_cents = (gross_total_cents + excluded.gross_total_cents) / 
                    CASE WHEN sale_count + excluded.sale_count > 0 
                         THEN sale_count + excluded.sale_count ELSE 1 END,
                updated_at = datetime('now'),
                version = version + 1
            "#,
        )
        .bind(&self.store_id)
        .bind(period_type)
        .bind(period_start)
        .bind(period_end)
        .bind(sale_count)
        .bind(item_count)
        .bind(gross_cents)
        .bind(tax_cents)
        .bind(net_cents)
        .bind(avg_cents)
        .execute(&self.pool)
        .await
        .map_err(|e| SyncError::DatabaseError(format!("Failed to upsert sales summary: {}", e)))?;

        Ok(())
    }

    /// Gets sales summary for a period.
    pub async fn get_sales_summary(
        &self,
        period_type: &str,
        period_start: &str,
    ) -> SyncResult<Option<SalesSummary>> {
        let result = sqlx::query(
            r#"
            SELECT sale_count, item_count, gross_total_cents, tax_total_cents, 
                   net_total_cents, avg_transaction_cents
            FROM sales_summaries
            WHERE store_id = ? AND period_type = ? AND period_start = ?
            "#,
        )
        .bind(&self.store_id)
        .bind(period_type)
        .bind(period_start)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SyncError::DatabaseError(format!("Failed to get sales summary: {}", e)))?;

        Ok(result.map(|row| SalesSummary {
            count: row.get::<i32, _>("sale_count") as u32,
            gross_cents: row.get("gross_total_cents"),
            tax_cents: row.get("tax_total_cents"),
            net_cents: row.get("net_total_cents"),
            items_sold: row.get::<i32, _>("item_count") as u32,
            avg_transaction_cents: row.get("avg_transaction_cents"),
        }))
    }

    /// Gets today's sales summary.
    pub async fn get_today_sales(&self) -> SyncResult<SalesSummary> {
        let result = sqlx::query(
            r#"
            SELECT 
                COALESCE(SUM(sale_count), 0) as sale_count,
                COALESCE(SUM(item_count), 0) as item_count,
                COALESCE(SUM(gross_total_cents), 0) as gross_total_cents,
                COALESCE(SUM(tax_total_cents), 0) as tax_total_cents,
                COALESCE(SUM(net_total_cents), 0) as net_total_cents
            FROM sales_summaries
            WHERE store_id = ? 
                AND period_type = 'hour'
                AND period_start >= date('now', 'start of day')
            "#,
        )
        .bind(&self.store_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| SyncError::DatabaseError(format!("Failed to get today's sales: {}", e)))?;

        let count: i32 = result.get("sale_count");
        let gross: i64 = result.get("gross_total_cents");

        Ok(SalesSummary {
            count: count as u32,
            gross_cents: gross,
            tax_cents: result.get("tax_total_cents"),
            net_cents: result.get("net_total_cents"),
            items_sold: result.get::<i32, _>("item_count") as u32,
            avg_transaction_cents: if count > 0 { gross / count as i64 } else { 0 },
        })
    }

    // =========================================================================
    // Payment Aggregation Operations
    // =========================================================================

    /// Records or updates a payment summary.
    pub async fn upsert_payment_summary(
        &self,
        period_type: &str,
        period_start: &str,
        payment_method: &str,
        count: i32,
        total_cents: i64,
    ) -> SyncResult<()> {
        let avg_cents = if count > 0 {
            total_cents / count as i64
        } else {
            0
        };

        sqlx::query(
            r#"
            INSERT INTO payment_summaries (
                store_id, period_type, period_start, payment_method,
                transaction_count, total_cents, avg_amount_cents, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, datetime('now'))
            ON CONFLICT(store_id, period_type, period_start, payment_method) DO UPDATE SET
                transaction_count = transaction_count + excluded.transaction_count,
                total_cents = total_cents + excluded.total_cents,
                avg_amount_cents = (total_cents + excluded.total_cents) /
                    CASE WHEN transaction_count + excluded.transaction_count > 0
                         THEN transaction_count + excluded.transaction_count ELSE 1 END,
                updated_at = datetime('now')
            "#,
        )
        .bind(&self.store_id)
        .bind(period_type)
        .bind(period_start)
        .bind(payment_method)
        .bind(count)
        .bind(total_cents)
        .bind(avg_cents)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            SyncError::DatabaseError(format!("Failed to upsert payment summary: {}", e))
        })?;

        Ok(())
    }

    /// Gets payment summaries for a period.
    pub async fn get_payment_summaries(
        &self,
        period_type: &str,
        period_start: &str,
    ) -> SyncResult<Vec<PaymentSummary>> {
        let rows = sqlx::query(
            r#"
            SELECT payment_method, transaction_count, total_cents
            FROM payment_summaries
            WHERE store_id = ? AND period_type = ? AND period_start = ?
            "#,
        )
        .bind(&self.store_id)
        .bind(period_type)
        .bind(period_start)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SyncError::DatabaseError(format!("Failed to get payment summaries: {}", e)))?;

        Ok(rows
            .iter()
            .map(|row| PaymentSummary {
                method: row.get("payment_method"),
                count: row.get::<i32, _>("transaction_count") as u32,
                total_cents: row.get("total_cents"),
            })
            .collect())
    }

    // =========================================================================
    // Device Activity Operations
    // =========================================================================

    /// Updates device activity.
    pub async fn update_device_activity(
        &self,
        device_id: &str,
        device_name: Option<&str>,
        status: &str,
        ip_address: Option<&str>,
    ) -> SyncResult<()> {
        sqlx::query(
            r#"
            INSERT INTO device_activity (
                device_id, device_name, store_id, status, ip_address, last_seen_at
            ) VALUES (?, ?, ?, ?, ?, datetime('now'))
            ON CONFLICT(device_id) DO UPDATE SET
                device_name = COALESCE(excluded.device_name, device_name),
                status = excluded.status,
                ip_address = COALESCE(excluded.ip_address, ip_address),
                last_seen_at = datetime('now')
            "#,
        )
        .bind(device_id)
        .bind(device_name)
        .bind(&self.store_id)
        .bind(status)
        .bind(ip_address)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            SyncError::DatabaseError(format!("Failed to update device activity: {}", e))
        })?;

        Ok(())
    }

    /// Increments device sales counter.
    pub async fn increment_device_sales(
        &self,
        device_id: &str,
        sale_count: i32,
        revenue_cents: i64,
    ) -> SyncResult<()> {
        sqlx::query(
            r#"
            UPDATE device_activity
            SET sales_today = sales_today + ?,
                revenue_today_cents = revenue_today_cents + ?,
                last_seen_at = datetime('now')
            WHERE device_id = ?
            "#,
        )
        .bind(sale_count)
        .bind(revenue_cents)
        .bind(device_id)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            SyncError::DatabaseError(format!("Failed to increment device sales: {}", e))
        })?;

        Ok(())
    }

    /// Gets active devices.
    pub async fn get_active_devices(&self) -> SyncResult<Vec<(String, String, String)>> {
        let rows = sqlx::query(
            r#"
            SELECT device_id, device_name, ip_address
            FROM device_activity
            WHERE store_id = ? AND status = 'connected'
                AND last_seen_at > datetime('now', '-5 minutes')
            "#,
        )
        .bind(&self.store_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SyncError::DatabaseError(format!("Failed to get active devices: {}", e)))?;

        Ok(rows
            .iter()
            .map(|row| {
                (
                    row.get("device_id"),
                    row.get::<Option<String>, _>("device_name")
                        .unwrap_or_default(),
                    row.get::<Option<String>, _>("ip_address")
                        .unwrap_or_default(),
                )
            })
            .collect())
    }

    /// Resets daily counters (call at midnight).
    pub async fn reset_daily_counters(&self) -> SyncResult<()> {
        sqlx::query(
            r#"
            UPDATE device_activity
            SET sales_today = 0, revenue_today_cents = 0
            WHERE store_id = ?
            "#,
        )
        .bind(&self.store_id)
        .execute(&self.pool)
        .await
        .map_err(|e| SyncError::DatabaseError(format!("Failed to reset daily counters: {}", e)))?;

        Ok(())
    }

    // =========================================================================
    // Aggregation Logging
    // =========================================================================

    /// Logs the start of an aggregation run.
    pub async fn log_aggregation_start(
        &self,
        aggregation_type: &str,
        period_start: Option<&str>,
        period_end: Option<&str>,
    ) -> SyncResult<i64> {
        let result = sqlx::query(
            r#"
            INSERT INTO aggregation_log (
                store_id, aggregation_type, period_start, period_end, status
            ) VALUES (?, ?, ?, ?, 'started')
            RETURNING id
            "#,
        )
        .bind(&self.store_id)
        .bind(aggregation_type)
        .bind(period_start)
        .bind(period_end)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| SyncError::DatabaseError(format!("Failed to log aggregation start: {}", e)))?;

        Ok(result.get("id"))
    }

    /// Logs the completion of an aggregation run.
    pub async fn log_aggregation_complete(
        &self,
        log_id: i64,
        records_processed: i32,
        duration_ms: i64,
    ) -> SyncResult<()> {
        sqlx::query(
            r#"
            UPDATE aggregation_log
            SET status = 'completed',
                records_processed = ?,
                duration_ms = ?,
                completed_at = datetime('now')
            WHERE id = ?
            "#,
        )
        .bind(records_processed)
        .bind(duration_ms)
        .bind(log_id)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            SyncError::DatabaseError(format!("Failed to log aggregation complete: {}", e))
        })?;

        Ok(())
    }

    /// Logs a failed aggregation run.
    pub async fn log_aggregation_failed(&self, log_id: i64, error: &str) -> SyncResult<()> {
        sqlx::query(
            r#"
            UPDATE aggregation_log
            SET status = 'failed', error = ?, completed_at = datetime('now')
            WHERE id = ?
            "#,
        )
        .bind(error)
        .bind(log_id)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            SyncError::DatabaseError(format!("Failed to log aggregation failure: {}", e))
        })?;

        Ok(())
    }

    // =========================================================================
    // Summary Generation
    // =========================================================================

    /// Generates an aggregation summary for broadcast/upload.
    pub async fn generate_summary(
        &self,
        period_start: &str,
        period_end: &str,
    ) -> SyncResult<AggregationSummary> {
        // Get sales summary
        let sales = self
            .get_sales_summary("hour", period_start)
            .await?
            .unwrap_or(SalesSummary {
                count: 0,
                gross_cents: 0,
                tax_cents: 0,
                net_cents: 0,
                items_sold: 0,
                avg_transaction_cents: 0,
            });

        // Get payment summaries
        let payments = self.get_payment_summaries("hour", period_start).await?;

        // Get top movers (optional)
        let top_movers = self.get_top_movers(period_start).await.ok();

        Ok(AggregationSummary {
            store_id: self.store_id.clone(),
            period_start: period_start.to_string(),
            period_end: period_end.to_string(),
            sales,
            payments,
            top_movers,
        })
    }

    /// Gets top inventory movers for a period.
    async fn get_top_movers(&self, period_start: &str) -> SyncResult<Vec<InventoryMover>> {
        let rows = sqlx::query(
            r#"
            SELECT product_id, sku, product_name, SUM(delta) as total_delta
            FROM inventory_snapshots
            WHERE store_id = ? AND snapshot_at >= ?
            GROUP BY product_id
            ORDER BY ABS(SUM(delta)) DESC
            LIMIT 10
            "#,
        )
        .bind(&self.store_id)
        .bind(period_start)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SyncError::DatabaseError(format!("Failed to get top movers: {}", e)))?;

        Ok(rows
            .iter()
            .map(|row| InventoryMover {
                product_id: row.get("product_id"),
                sku: row.get("sku"),
                name: row
                    .get::<Option<String>, _>("product_name")
                    .unwrap_or_default(),
                quantity_delta: row.get("total_delta"),
            })
            .collect())
    }

    // =========================================================================
    // Cleanup Operations
    // =========================================================================

    /// Purges data older than the retention period.
    pub async fn purge_old_data(&self, retention_days: u32) -> SyncResult<u64> {
        let cutoff = format!("-{} days", retention_days);
        let mut total_deleted = 0u64;

        // Purge old inventory snapshots (keep latest per product)
        let result = sqlx::query(
            r#"
            DELETE FROM inventory_snapshots
            WHERE store_id = ? 
                AND snapshot_at < datetime('now', ?)
                AND id NOT IN (
                    SELECT id FROM (
                        SELECT id, ROW_NUMBER() OVER (
                            PARTITION BY product_id ORDER BY snapshot_at DESC
                        ) as rn
                        FROM inventory_snapshots WHERE store_id = ?
                    ) WHERE rn = 1
                )
            "#,
        )
        .bind(&self.store_id)
        .bind(&cutoff)
        .bind(&self.store_id)
        .execute(&self.pool)
        .await
        .map_err(|e| SyncError::DatabaseError(format!("Failed to purge inventory: {}", e)))?;
        total_deleted += result.rows_affected();

        // Purge old sales summaries
        let result = sqlx::query(
            r#"
            DELETE FROM sales_summaries
            WHERE store_id = ? AND period_start < datetime('now', ?)
            "#,
        )
        .bind(&self.store_id)
        .bind(&cutoff)
        .execute(&self.pool)
        .await
        .map_err(|e| SyncError::DatabaseError(format!("Failed to purge sales: {}", e)))?;
        total_deleted += result.rows_affected();

        // Purge old payment summaries
        let result = sqlx::query(
            r#"
            DELETE FROM payment_summaries
            WHERE store_id = ? AND period_start < datetime('now', ?)
            "#,
        )
        .bind(&self.store_id)
        .bind(&cutoff)
        .execute(&self.pool)
        .await
        .map_err(|e| SyncError::DatabaseError(format!("Failed to purge payments: {}", e)))?;
        total_deleted += result.rows_affected();

        // Purge old aggregation logs
        let result = sqlx::query(
            r#"
            DELETE FROM aggregation_log
            WHERE store_id = ? AND started_at < datetime('now', ?)
            "#,
        )
        .bind(&self.store_id)
        .bind(&cutoff)
        .execute(&self.pool)
        .await
        .map_err(|e| SyncError::DatabaseError(format!("Failed to purge logs: {}", e)))?;
        total_deleted += result.rows_affected();

        info!(
            total_deleted,
            retention_days, "Purged old data from store database"
        );
        Ok(total_deleted)
    }

    /// Closes the database connection.
    pub async fn close(self) {
        self.pool.close().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_db() -> (StoreDatabase, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_store.db");
        let db = StoreDatabase::open(&db_path, "test-store-001")
            .await
            .unwrap();
        (db, temp_dir)
    }

    #[tokio::test]
    async fn test_inventory_snapshot() {
        let (db, _temp) = create_test_db().await;

        db.record_inventory_snapshot(
            "prod-123",
            "SKU-001",
            Some("Test Product"),
            100,
            -5,
            Some("device-001"),
        )
        .await
        .unwrap();

        let qty = db.get_current_inventory("prod-123").await.unwrap();
        assert_eq!(qty, Some(100));
    }

    #[tokio::test]
    async fn test_sales_summary() {
        let (db, _temp) = create_test_db().await;

        db.upsert_sales_summary(
            "hour",
            "2026-02-01T10:00:00Z",
            "2026-02-01T11:00:00Z",
            5,
            15,
            10000,
            500,
            10500,
        )
        .await
        .unwrap();

        let summary = db
            .get_sales_summary("hour", "2026-02-01T10:00:00Z")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(summary.count, 5);
        assert_eq!(summary.gross_cents, 10000);
    }

    #[tokio::test]
    async fn test_device_activity() {
        let (db, _temp) = create_test_db().await;

        db.update_device_activity(
            "device-001",
            Some("Register 1"),
            "connected",
            Some("192.168.1.10"),
        )
        .await
        .unwrap();

        let devices = db.get_active_devices().await.unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].0, "device-001");
    }
}
