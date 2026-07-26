-- Migration: 004_store_aggregates_schema.sql
-- Description: Schema for store-level aggregation database (Milestone 4)
--
-- NOTE: This schema is for a SEPARATE database file (store_aggregates.db),
-- not the main titan.db. The store aggregates database is maintained by the
-- PRIMARY (Store Hub) and contains aggregated data from all POS devices.
--
-- Architecture Overview:
-- ┌──────────────────────────────────────────────────────────────────────────────┐
-- │                    Store Aggregates Database                                 │
-- │                                                                              │
-- │  Location: {data_dir}/store_aggregates.db                                   │
-- │  Managed by: PRIMARY (Store Hub) only                                       │
-- │                                                                              │
-- │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────────┐  │
-- │  │  inventory_     │  │  sales_         │  │     payment_                │  │
-- │  │  snapshots      │  │  summaries      │  │     summaries               │  │
-- │  │                 │  │                 │  │                             │  │
-- │  │ Point-in-time   │  │ Aggregated      │  │ Payment breakdown           │  │
-- │  │ inventory       │  │ sales by period │  │ by method per period        │  │
-- │  │ per product     │  │                 │  │                             │  │
-- │  └─────────────────┘  └─────────────────┘  └─────────────────────────────┘  │
-- │                                                                              │
-- │  ┌─────────────────┐  ┌─────────────────┐                                   │
-- │  │  device_        │  │  aggregation_   │                                   │
-- │  │  activity       │  │  log            │                                   │
-- │  │                 │  │                 │                                   │
-- │  │ Connected       │  │ Audit trail     │                                   │
-- │  │ devices &       │  │ of aggregation  │                                   │
-- │  │ their status    │  │ runs            │                                   │
-- │  └─────────────────┘  └─────────────────┘                                   │
-- └──────────────────────────────────────────────────────────────────────────────┘
--
-- Purpose:
-- 1. inventory_snapshots: Current and historical inventory levels
-- 2. sales_summaries: Aggregated sales data by time period
-- 3. payment_summaries: Payment breakdown by method
-- 4. device_activity: Track connected devices and their status
-- 5. aggregation_log: Audit trail of aggregation runs

-- Enable WAL mode for better concurrent access
PRAGMA journal_mode = WAL;

--------------------------------------------------------------------------------
-- Table: store_info
--------------------------------------------------------------------------------
-- Store identity information. One row per database.
--
CREATE TABLE IF NOT EXISTS store_info (
    -- Store UUID
    store_id TEXT PRIMARY KEY NOT NULL,
    
    -- Human-readable store name
    store_name TEXT NOT NULL,
    
    -- Tenant ID this store belongs to
    tenant_id TEXT NOT NULL,
    
    -- Store timezone (for time-based aggregations)
    timezone TEXT NOT NULL DEFAULT 'UTC',
    
    -- When this store was registered
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- Last aggregation run timestamp
    last_aggregation_at TEXT,
    
    -- Configuration JSON for store-specific settings
    config TEXT
);

--------------------------------------------------------------------------------
-- Table: inventory_snapshots
--------------------------------------------------------------------------------
-- Point-in-time inventory snapshots per product.
-- Updated in real-time as inventory deltas arrive from POS devices.
--
-- Usage:
-- - Current inventory = latest snapshot for each product
-- - Historical queries = filter by snapshot_at timestamp
--
CREATE TABLE IF NOT EXISTS inventory_snapshots (
    -- Auto-increment ID for ordering
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    
    -- Store ID (denormalized for query efficiency)
    store_id TEXT NOT NULL,
    
    -- Product UUID
    product_id TEXT NOT NULL,
    
    -- Product SKU (denormalized for queries)
    sku TEXT NOT NULL,
    
    -- Product name (denormalized for reports)
    product_name TEXT,
    
    -- Current stock level at this snapshot
    quantity INTEGER NOT NULL,
    
    -- Delta that caused this snapshot (for audit)
    delta INTEGER NOT NULL DEFAULT 0,
    
    -- Source of the delta (device_id or 'adjustment', 'receiving', etc.)
    delta_source TEXT,
    
    -- When this snapshot was created
    snapshot_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- Cost value of inventory (cents)
    cost_cents INTEGER,
    
    -- Index for efficient queries
    UNIQUE(store_id, product_id, snapshot_at)
);

-- Index for finding current inventory (latest snapshot per product)
CREATE INDEX IF NOT EXISTS idx_inventory_snapshots_current 
    ON inventory_snapshots(store_id, product_id, snapshot_at DESC);

-- Index for product lookups
CREATE INDEX IF NOT EXISTS idx_inventory_snapshots_sku 
    ON inventory_snapshots(store_id, sku);

--------------------------------------------------------------------------------
-- Table: sales_summaries
--------------------------------------------------------------------------------
-- Aggregated sales data by time period (hourly and daily).
--
-- Aggregation granularity:
-- - period_type = 'hour': One row per hour
-- - period_type = 'day': One row per day
--
CREATE TABLE IF NOT EXISTS sales_summaries (
    -- Auto-increment ID
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    
    -- Store ID
    store_id TEXT NOT NULL,
    
    -- Period type: 'hour', 'day', 'week', 'month'
    period_type TEXT NOT NULL,
    
    -- Period start timestamp (ISO8601)
    period_start TEXT NOT NULL,
    
    -- Period end timestamp (ISO8601)
    period_end TEXT NOT NULL,
    
    -- Number of completed sales in this period
    sale_count INTEGER NOT NULL DEFAULT 0,
    
    -- Total number of items sold
    item_count INTEGER NOT NULL DEFAULT 0,
    
    -- Gross total before tax (cents)
    gross_total_cents INTEGER NOT NULL DEFAULT 0,
    
    -- Total tax collected (cents)
    tax_total_cents INTEGER NOT NULL DEFAULT 0,
    
    -- Net total after tax (cents)
    net_total_cents INTEGER NOT NULL DEFAULT 0,
    
    -- Total discounts applied (cents)
    discount_total_cents INTEGER NOT NULL DEFAULT 0,
    
    -- Number of refunds/returns
    refund_count INTEGER NOT NULL DEFAULT 0,
    
    -- Refund total (cents)
    refund_total_cents INTEGER NOT NULL DEFAULT 0,
    
    -- Average transaction value (cents)
    avg_transaction_cents INTEGER NOT NULL DEFAULT 0,
    
    -- Number of unique customers (if tracked)
    customer_count INTEGER DEFAULT 0,
    
    -- When this summary was created
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- When this summary was last updated
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- Version for optimistic locking
    version INTEGER NOT NULL DEFAULT 1,
    
    -- Unique constraint for idempotent updates
    UNIQUE(store_id, period_type, period_start)
);

-- Index for time-based queries
CREATE INDEX IF NOT EXISTS idx_sales_summaries_period 
    ON sales_summaries(store_id, period_type, period_start DESC);

--------------------------------------------------------------------------------
-- Table: payment_summaries
--------------------------------------------------------------------------------
-- Payment breakdown by method for each period.
--
CREATE TABLE IF NOT EXISTS payment_summaries (
    -- Auto-increment ID
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    
    -- Store ID
    store_id TEXT NOT NULL,
    
    -- Period type: 'hour', 'day', 'week', 'month'
    period_type TEXT NOT NULL,
    
    -- Period start timestamp (ISO8601)
    period_start TEXT NOT NULL,
    
    -- Payment method: 'cash', 'card', 'mobile', etc.
    payment_method TEXT NOT NULL,
    
    -- Number of transactions with this payment method
    transaction_count INTEGER NOT NULL DEFAULT 0,
    
    -- Total amount paid via this method (cents)
    total_cents INTEGER NOT NULL DEFAULT 0,
    
    -- Average payment amount (cents)
    avg_amount_cents INTEGER NOT NULL DEFAULT 0,
    
    -- Total tips if applicable (cents)
    tips_cents INTEGER NOT NULL DEFAULT 0,
    
    -- When this summary was created
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- When this summary was last updated
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- Unique constraint
    UNIQUE(store_id, period_type, period_start, payment_method)
);

-- Index for payment reports
CREATE INDEX IF NOT EXISTS idx_payment_summaries_period 
    ON payment_summaries(store_id, period_type, period_start DESC);

CREATE INDEX IF NOT EXISTS idx_payment_summaries_method 
    ON payment_summaries(store_id, payment_method, period_start DESC);

--------------------------------------------------------------------------------
-- Table: device_activity
--------------------------------------------------------------------------------
-- Tracks connected POS devices and their activity.
--
CREATE TABLE IF NOT EXISTS device_activity (
    -- Device UUID (same as device_id in config)
    device_id TEXT PRIMARY KEY NOT NULL,
    
    -- Human-readable device name
    device_name TEXT,
    
    -- Store ID this device belongs to
    store_id TEXT NOT NULL,
    
    -- Current connection status: 'connected', 'disconnected', 'unknown'
    status TEXT NOT NULL DEFAULT 'unknown',
    
    -- IP address of the device
    ip_address TEXT,
    
    -- Last seen timestamp
    last_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- First connected timestamp
    first_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- Number of sales processed by this device today
    sales_today INTEGER NOT NULL DEFAULT 0,
    
    -- Total revenue processed by this device today (cents)
    revenue_today_cents INTEGER NOT NULL DEFAULT 0,
    
    -- Number of inventory deltas from this device
    deltas_received INTEGER NOT NULL DEFAULT 0,
    
    -- Any error/warning message
    last_error TEXT,
    
    -- Device metadata (JSON)
    metadata TEXT
);

-- Index for active devices
CREATE INDEX IF NOT EXISTS idx_device_activity_status 
    ON device_activity(store_id, status, last_seen_at DESC);

--------------------------------------------------------------------------------
-- Table: aggregation_log
--------------------------------------------------------------------------------
-- Audit trail of aggregation runs.
--
CREATE TABLE IF NOT EXISTS aggregation_log (
    -- Auto-increment ID
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    
    -- Store ID
    store_id TEXT NOT NULL,
    
    -- Type of aggregation: 'inventory', 'sales', 'payments', 'full'
    aggregation_type TEXT NOT NULL,
    
    -- Period that was aggregated
    period_start TEXT,
    period_end TEXT,
    
    -- Status: 'started', 'completed', 'failed'
    status TEXT NOT NULL,
    
    -- Number of records processed
    records_processed INTEGER DEFAULT 0,
    
    -- Duration in milliseconds
    duration_ms INTEGER,
    
    -- Error message if failed
    error TEXT,
    
    -- When aggregation started
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- When aggregation completed
    completed_at TEXT
);

-- Index for recent aggregations
CREATE INDEX IF NOT EXISTS idx_aggregation_log_recent 
    ON aggregation_log(store_id, started_at DESC);

--------------------------------------------------------------------------------
-- Table: top_products
--------------------------------------------------------------------------------
-- Cached top-selling products per period (for quick dashboard queries).
--
CREATE TABLE IF NOT EXISTS top_products (
    -- Auto-increment ID
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    
    -- Store ID
    store_id TEXT NOT NULL,
    
    -- Period type: 'day', 'week', 'month'
    period_type TEXT NOT NULL,
    
    -- Period start timestamp
    period_start TEXT NOT NULL,
    
    -- Product ID
    product_id TEXT NOT NULL,
    
    -- Product SKU (denormalized)
    sku TEXT NOT NULL,
    
    -- Product name (denormalized)
    product_name TEXT,
    
    -- Rank in this period (1 = top seller)
    rank INTEGER NOT NULL,
    
    -- Quantity sold
    quantity_sold INTEGER NOT NULL,
    
    -- Revenue generated (cents)
    revenue_cents INTEGER NOT NULL,
    
    -- When this was calculated
    calculated_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- Unique constraint
    UNIQUE(store_id, period_type, period_start, product_id)
);

-- Index for dashboard queries
CREATE INDEX IF NOT EXISTS idx_top_products_period 
    ON top_products(store_id, period_type, period_start, rank);

--------------------------------------------------------------------------------
-- Table: cloud_sync_status
--------------------------------------------------------------------------------
-- Tracks what has been synced to cloud.
--
CREATE TABLE IF NOT EXISTS cloud_sync_status (
    -- Entity type: 'sales_summary', 'payment_summary', 'inventory_snapshot'
    entity_type TEXT NOT NULL,
    
    -- Last synced period/timestamp
    last_synced_at TEXT,
    
    -- Last synced ID/cursor
    last_synced_id INTEGER,
    
    -- Sync status: 'synced', 'pending', 'error'
    status TEXT NOT NULL DEFAULT 'pending',
    
    -- Last error if any
    last_error TEXT,
    
    -- When status was updated
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    PRIMARY KEY(entity_type)
);

-- Initialize sync status
INSERT OR IGNORE INTO cloud_sync_status (entity_type, status) VALUES 
    ('sales_summary', 'pending'),
    ('payment_summary', 'pending'),
    ('inventory_snapshot', 'pending');

--------------------------------------------------------------------------------
-- Views for common queries
--------------------------------------------------------------------------------

-- Current inventory view (latest snapshot per product)
CREATE VIEW IF NOT EXISTS current_inventory AS
SELECT 
    i.store_id,
    i.product_id,
    i.sku,
    i.product_name,
    i.quantity,
    i.cost_cents,
    i.snapshot_at as last_updated
FROM inventory_snapshots i
INNER JOIN (
    SELECT store_id, product_id, MAX(snapshot_at) as max_snapshot
    FROM inventory_snapshots
    GROUP BY store_id, product_id
) latest ON i.store_id = latest.store_id 
    AND i.product_id = latest.product_id 
    AND i.snapshot_at = latest.max_snapshot;

-- Today's sales summary view
CREATE VIEW IF NOT EXISTS today_sales AS
SELECT 
    store_id,
    SUM(sale_count) as total_sales,
    SUM(item_count) as total_items,
    SUM(gross_total_cents) as gross_total,
    SUM(tax_total_cents) as tax_total,
    SUM(net_total_cents) as net_total
FROM sales_summaries
WHERE period_type = 'hour' 
    AND period_start >= date('now', 'start of day')
GROUP BY store_id;

-- Active devices view
CREATE VIEW IF NOT EXISTS active_devices AS
SELECT *
FROM device_activity
WHERE status = 'connected' 
    AND last_seen_at > datetime('now', '-5 minutes');
