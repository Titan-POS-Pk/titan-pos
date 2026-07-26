//! # Store Aggregator Module
//!
//! Aggregates data from all connected POS devices at the store level.
//! Only runs on PRIMARY (Store Hub).
//!
//! ## Aggregation Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Store Aggregator Architecture                        │
//! │                                                                         │
//! │  The aggregator runs ONLY on PRIMARY (Store Hub).                       │
//! │  It processes incoming data from all connected SECONDARY devices.       │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                      Data Flow                                  │   │
//! │  │                                                                 │   │
//! │  │  SECONDARY #1  ──┐                                              │   │
//! │  │  SECONDARY #2  ──┼──► StoreAggregator ──► store_aggregates.db  │   │
//! │  │  SECONDARY #3  ──┘         │                                    │   │
//! │  │                            │                                    │   │
//! │  │                            ├──► Broadcast to all SECONDARY      │   │
//! │  │                            └──► Upload to Cloud (batched)       │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                                                                         │
//! │  AGGREGATION TYPES:                                                    │
//! │  ──────────────────                                                    │
//! │                                                                         │
//! │  1. INVENTORY (Real-time)                                              │
//! │     • Delta received → Update snapshot → Broadcast immediately         │
//! │     • No batching - inventory accuracy is critical                     │
//! │                                                                         │
//! │  2. SALES (Batched)                                                    │
//! │     • Sale finalized → Queue for aggregation                           │
//! │     • Every N seconds (configurable) → Aggregate & broadcast          │
//! │     • Reduces cloud API calls                                          │
//! │                                                                         │
//! │  3. PAYMENTS (Batched with Sales)                                      │
//! │     • Grouped by payment method                                        │
//! │     • Aggregated alongside sales                                       │
//! │                                                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Timelike, Utc};
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use crate::config::AggregationSettings;
use crate::error::{SyncError, SyncResult};
use crate::protocol::{
    AggregationSummary, SalesSummary,
};
use crate::store_db::StoreDatabase;

// =============================================================================
// Aggregation Event Types
// =============================================================================

/// Events that trigger aggregation.
#[derive(Debug, Clone)]
pub enum AggregationEvent {
    /// Inventory delta received from a device.
    InventoryDelta {
        device_id: String,
        product_id: String,
        sku: String,
        delta: i32,
        timestamp: DateTime<Utc>,
    },

    /// Sale completed on a device.
    SaleCompleted {
        device_id: String,
        sale_id: String,
        gross_cents: i64,
        tax_cents: i64,
        net_cents: i64,
        item_count: i32,
        payments: Vec<(String, i64)>, // (method, amount_cents)
        timestamp: DateTime<Utc>,
    },

    /// Force aggregation flush.
    Flush,

    /// Shutdown the aggregator.
    Shutdown,
}

// =============================================================================
// Pending Aggregation
// =============================================================================

/// Pending sales aggregation data.
#[derive(Debug, Default)]
struct PendingSales {
    sale_count: i32,
    item_count: i32,
    gross_cents: i64,
    tax_cents: i64,
    net_cents: i64,
    payments: HashMap<String, (i32, i64)>, // method -> (count, total_cents)
    period_start: Option<DateTime<Utc>>,
}

impl PendingSales {
    fn add_sale(
        &mut self,
        gross_cents: i64,
        tax_cents: i64,
        net_cents: i64,
        item_count: i32,
        payments: &[(String, i64)],
        timestamp: DateTime<Utc>,
    ) {
        if self.period_start.is_none() {
            // Round down to hour boundary
            self.period_start = Some(
                timestamp
                    .date_naive()
                    .and_hms_opt(timestamp.hour(), 0, 0)
                    .unwrap()
                    .and_utc(),
            );
        }

        self.sale_count += 1;
        self.item_count += item_count;
        self.gross_cents += gross_cents;
        self.tax_cents += tax_cents;
        self.net_cents += net_cents;

        for (method, amount) in payments {
            let entry = self.payments.entry(method.clone()).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += amount;
        }
    }

    fn is_empty(&self) -> bool {
        self.sale_count == 0
    }

    fn clear(&mut self) {
        self.sale_count = 0;
        self.item_count = 0;
        self.gross_cents = 0;
        self.tax_cents = 0;
        self.net_cents = 0;
        self.payments.clear();
        self.period_start = None;
    }
}

// =============================================================================
// Store Aggregator
// =============================================================================

/// Store-level data aggregator.
///
/// This component runs on the PRIMARY (Store Hub) and aggregates data
/// from all connected SECONDARY devices.
pub struct StoreAggregator {
    /// Store database for persisting aggregations.
    store_db: Arc<StoreDatabase>,

    /// Aggregation settings.
    settings: AggregationSettings,

    /// Store ID.
    store_id: String,

    /// Current inventory levels (in-memory cache).
    /// Maps product_id -> (sku, quantity)
    inventory_cache: RwLock<HashMap<String, (String, i32)>>,

    /// Pending sales data (awaiting batch aggregation).
    pending_sales: RwLock<PendingSales>,

    /// Broadcast channel for sending aggregation summaries.
    summary_tx: broadcast::Sender<AggregationSummary>,
}

/// Handle for interacting with the store aggregator.
pub struct StoreAggregatorHandle {
    /// Event sender.
    event_tx: mpsc::Sender<AggregationEvent>,

    /// Summary sender (for subscribing new receivers).
    summary_tx: broadcast::Sender<AggregationSummary>,

    /// Store database reference.
    store_db: Arc<StoreDatabase>,
}

impl Clone for StoreAggregatorHandle {
    fn clone(&self) -> Self {
        Self {
            event_tx: self.event_tx.clone(),
            summary_tx: self.summary_tx.clone(),
            store_db: Arc::clone(&self.store_db),
        }
    }
}

impl StoreAggregatorHandle {
    /// Subscribe to aggregation summaries.
    pub fn subscribe_summaries(&self) -> broadcast::Receiver<AggregationSummary> {
        self.summary_tx.subscribe()
    }

    /// Sends an inventory delta for aggregation.
    pub async fn record_inventory_delta(
        &self,
        device_id: &str,
        product_id: &str,
        sku: &str,
        delta: i32,
    ) -> SyncResult<()> {
        self.event_tx
            .send(AggregationEvent::InventoryDelta {
                device_id: device_id.to_string(),
                product_id: product_id.to_string(),
                sku: sku.to_string(),
                delta,
                timestamp: Utc::now(),
            })
            .await
            .map_err(|_| SyncError::ChannelError("Aggregator channel closed".into()))
    }

    /// Records a completed sale for aggregation.
    pub async fn record_sale(
        &self,
        device_id: &str,
        sale_id: &str,
        gross_cents: i64,
        tax_cents: i64,
        net_cents: i64,
        item_count: i32,
        payments: Vec<(String, i64)>,
    ) -> SyncResult<()> {
        self.event_tx
            .send(AggregationEvent::SaleCompleted {
                device_id: device_id.to_string(),
                sale_id: sale_id.to_string(),
                gross_cents,
                tax_cents,
                net_cents,
                item_count,
                payments,
                timestamp: Utc::now(),
            })
            .await
            .map_err(|_| SyncError::ChannelError("Aggregator channel closed".into()))
    }

    /// Forces an immediate aggregation flush.
    pub async fn flush(&self) -> SyncResult<()> {
        self.event_tx
            .send(AggregationEvent::Flush)
            .await
            .map_err(|_| SyncError::ChannelError("Aggregator channel closed".into()))
    }

    /// Shuts down the aggregator.
    pub async fn shutdown(&self) -> SyncResult<()> {
        self.event_tx
            .send(AggregationEvent::Shutdown)
            .await
            .map_err(|_| SyncError::ChannelError("Aggregator channel closed".into()))
    }

    /// Gets the store database reference.
    pub fn store_db(&self) -> &StoreDatabase {
        &self.store_db
    }

    /// Gets today's sales summary from the database.
    pub async fn get_today_sales(&self) -> SyncResult<SalesSummary> {
        self.store_db.get_today_sales().await
    }

    /// Gets current inventory for a product.
    pub async fn get_inventory(&self, product_id: &str) -> SyncResult<Option<i32>> {
        self.store_db.get_current_inventory(product_id).await
    }
}

impl StoreAggregator {
    /// Creates a new store aggregator.
    pub async fn new(
        store_db: Arc<StoreDatabase>,
        settings: AggregationSettings,
    ) -> SyncResult<Self> {
        let store_id = store_db.store_id().to_string();
        let (summary_tx, _) = broadcast::channel(64);

        // Initialize inventory cache from database
        let inventory_cache = RwLock::new(HashMap::new());
        {
            let inventory = store_db.get_all_current_inventory().await?;
            let mut cache = inventory_cache.write().await;
            for (product_id, sku, qty) in inventory {
                cache.insert(product_id, (sku, qty));
            }
            info!(count = cache.len(), "Loaded inventory cache");
        }

        Ok(StoreAggregator {
            store_db,
            settings,
            store_id,
            inventory_cache,
            pending_sales: RwLock::new(PendingSales::default()),
            summary_tx,
        })
    }

    /// Starts the aggregator and returns a handle.
    pub fn start(self) -> StoreAggregatorHandle {
        let (event_tx, event_rx) = mpsc::channel(256);
        let summary_tx = self.summary_tx.clone();
        let store_db = self.store_db.clone();

        // Spawn the aggregation loop
        tokio::spawn(async move {
            self.run(event_rx).await;
        });

        StoreAggregatorHandle {
            event_tx,
            summary_tx,
            store_db,
        }
    }

    /// Main aggregation loop.
    async fn run(self, mut event_rx: mpsc::Receiver<AggregationEvent>) {
        info!(
            store_id = %self.store_id,
            batch_interval_secs = self.settings.sales_batch_interval_secs,
            "Store aggregator started"
        );

        let mut batch_timer = interval(Duration::from_secs(self.settings.sales_batch_interval_secs));
        let mut last_daily_reset: Option<DateTime<Utc>> = None;

        loop {
            tokio::select! {
                Some(event) = event_rx.recv() => {
                    match event {
                        AggregationEvent::Shutdown => {
                            info!("Store aggregator shutting down");
                            // Flush pending data before shutdown
                            self.flush_sales().await;
                            break;
                        }
                        AggregationEvent::InventoryDelta { device_id, product_id, sku, delta, timestamp } => {
                            self.handle_inventory_delta(&device_id, &product_id, &sku, delta, timestamp).await;
                        }
                        AggregationEvent::SaleCompleted { device_id, sale_id, gross_cents, tax_cents, net_cents, item_count, payments, timestamp } => {
                            self.handle_sale_completed(&device_id, &sale_id, gross_cents, tax_cents, net_cents, item_count, &payments, timestamp).await;
                        }
                        AggregationEvent::Flush => {
                            self.flush_sales().await;
                        }
                    }
                }
                _ = batch_timer.tick() => {
                    // Periodic batch aggregation
                    self.flush_sales().await;

                    // Check for daily reset
                    let now = Utc::now();
                    let today_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
                    if last_daily_reset.is_none_or(|t| t < today_start) {
                        self.daily_reset().await;
                        last_daily_reset = Some(now);
                    }
                }
            }
        }
    }

    /// Handles an inventory delta.
    async fn handle_inventory_delta(
        &self,
        device_id: &str,
        product_id: &str,
        sku: &str,
        delta: i32,
        _timestamp: DateTime<Utc>,
    ) {
        debug!(
            device_id,
            product_id,
            sku,
            delta,
            "Processing inventory delta"
        );

        // Update in-memory cache
        let new_qty = {
            let mut cache = self.inventory_cache.write().await;
            let entry = cache.entry(product_id.to_string()).or_insert((sku.to_string(), 0));
            entry.1 += delta;
            entry.1
        };

        // Record snapshot in database
        if let Err(e) = self
            .store_db
            .record_inventory_snapshot(product_id, sku, None, new_qty, delta, Some(device_id))
            .await
        {
            error!(?e, product_id, "Failed to record inventory snapshot");
        }

        // Update device activity
        if let Err(e) = self
            .store_db
            .update_device_activity(device_id, None, "connected", None)
            .await
        {
            warn!(?e, device_id, "Failed to update device activity");
        }
    }

    /// Handles a completed sale.
    async fn handle_sale_completed(
        &self,
        device_id: &str,
        sale_id: &str,
        gross_cents: i64,
        tax_cents: i64,
        net_cents: i64,
        item_count: i32,
        payments: &[(String, i64)],
        timestamp: DateTime<Utc>,
    ) {
        debug!(
            device_id,
            sale_id,
            gross_cents,
            item_count,
            "Processing completed sale"
        );

        // Add to pending aggregation
        {
            let mut pending = self.pending_sales.write().await;
            pending.add_sale(gross_cents, tax_cents, net_cents, item_count, payments, timestamp);
        }

        // Update device activity
        if let Err(e) = self
            .store_db
            .increment_device_sales(device_id, 1, gross_cents)
            .await
        {
            warn!(?e, device_id, "Failed to increment device sales");
        }
    }

    /// Flushes pending sales to the database and broadcasts summary.
    async fn flush_sales(&self) {
        let now = Utc::now();
        let pending = {
            let mut p = self.pending_sales.write().await;
            if p.is_empty() {
                return;
            }
            let snapshot = PendingSales {
                sale_count: p.sale_count,
                item_count: p.item_count,
                gross_cents: p.gross_cents,
                tax_cents: p.tax_cents,
                net_cents: p.net_cents,
                payments: p.payments.clone(),
                period_start: p.period_start,
            };
            p.clear();
            snapshot
        };

        let period_start = pending.period_start.unwrap_or(now);
        let period_start_str = period_start.to_rfc3339();
        let period_end_str = now.to_rfc3339();

        debug!(
            sale_count = pending.sale_count,
            gross_cents = pending.gross_cents,
            "Flushing sales aggregation"
        );

        // Log aggregation start
        let log_id = match self
            .store_db
            .log_aggregation_start("sales", Some(&period_start_str), Some(&period_end_str))
            .await
        {
            Ok(id) => Some(id),
            Err(e) => {
                warn!(?e, "Failed to log aggregation start");
                None
            }
        };

        let start_time = Instant::now();

        // Upsert sales summary
        if let Err(e) = self
            .store_db
            .upsert_sales_summary(
                "hour",
                &period_start_str,
                &period_end_str,
                pending.sale_count,
                pending.item_count,
                pending.gross_cents,
                pending.tax_cents,
                pending.net_cents,
            )
            .await
        {
            error!(?e, "Failed to upsert sales summary");
            if let Some(id) = log_id {
                let _ = self.store_db.log_aggregation_failed(id, &e.to_string()).await;
            }
            return;
        }

        // Upsert payment summaries
        for (method, (count, total)) in &pending.payments {
            if let Err(e) = self
                .store_db
                .upsert_payment_summary("hour", &period_start_str, method, *count, *total)
                .await
            {
                error!(?e, method, "Failed to upsert payment summary");
            }
        }

        let duration_ms = start_time.elapsed().as_millis() as i64;

        // Log completion
        if let Some(id) = log_id {
            let _ = self
                .store_db
                .log_aggregation_complete(id, pending.sale_count, duration_ms)
                .await;
        }

        // Generate and broadcast summary
        match self.store_db.generate_summary(&period_start_str, &period_end_str).await {
            Ok(summary) => {
                let _ = self.summary_tx.send(summary);
                info!(
                    sale_count = pending.sale_count,
                    duration_ms,
                    "Sales aggregation flushed"
                );
            }
            Err(e) => {
                warn!(?e, "Failed to generate aggregation summary");
            }
        }
    }

    /// Performs daily reset (clear device counters, etc.).
    async fn daily_reset(&self) {
        info!("Performing daily reset");

        if let Err(e) = self.store_db.reset_daily_counters().await {
            error!(?e, "Failed to reset daily counters");
        }

        // Purge old data based on retention settings
        if self.settings.retention_days > 0 {
            if let Err(e) = self
                .store_db
                .purge_old_data(self.settings.retention_days)
                .await
            {
                error!(?e, "Failed to purge old data");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_aggregator() -> (StoreAggregatorHandle, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_store.db");
        let store_db = Arc::new(StoreDatabase::open(&db_path, "test-store").await.unwrap());

        let settings = AggregationSettings {
            enabled: true,
            sales_batch_interval_secs: 1, // 1 second for tests
            ..Default::default()
        };

        let aggregator = StoreAggregator::new(store_db, settings).await.unwrap();
        let handle = aggregator.start();

        (handle, temp_dir)
    }

    #[tokio::test]
    async fn test_inventory_aggregation() {
        let (handle, _temp) = create_test_aggregator().await;

        handle
            .record_inventory_delta("device-001", "prod-123", "SKU-001", -5)
            .await
            .unwrap();

        // Give it a moment to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        let qty = handle.get_inventory("prod-123").await.unwrap();
        assert_eq!(qty, Some(-5));

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_sales_aggregation() {
        let (handle, _temp) = create_test_aggregator().await;

        handle
            .record_sale(
                "device-001",
                "sale-001",
                1000,
                50,
                1050,
                3,
                vec![("cash".to_string(), 1050)],
            )
            .await
            .unwrap();

        // Force flush
        handle.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        let summary = handle.get_today_sales().await.unwrap();
        assert!(summary.count >= 1);

        handle.shutdown().await.unwrap();
    }
}
