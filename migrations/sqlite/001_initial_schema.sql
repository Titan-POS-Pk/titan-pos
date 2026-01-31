-- =============================================================================
-- Titan POS: Initial Database Schema
-- Migration: 001_initial_schema.sql
-- =============================================================================
--
-- This migration creates the core tables for Titan POS.
--
-- ## Table Overview
-- ```
-- ┌─────────────────────────────────────────────────────────────────────────┐
-- │                         Core Tables                                      │
-- │                                                                         │
-- │  products ──────────┐                                                   │
-- │  (inventory)        │                                                   │
-- │                     ▼                                                   │
-- │              ┌─────────────┐                                            │
-- │              │   sales     │ ◄─────── sale_items (line items)          │
-- │              │  (orders)   │                                            │
-- │              └──────┬──────┘                                            │
-- │                     │                                                   │
-- │                     ▼                                                   │
-- │              ┌─────────────┐                                            │
-- │              │  payments   │                                            │
-- │              └─────────────┘                                            │
-- │                                                                         │
-- │  sync_outbox (offline sync queue)                                       │
-- │  config (app configuration)                                             │
-- │                                                                         │
-- └─────────────────────────────────────────────────────────────────────────┘
-- ```
--
-- ## Key Design Decisions
-- 1. All primary keys are UUID v4 (TEXT) for offline-safe ID generation
-- 2. All monetary values are in cents (INTEGER) to avoid floating point
-- 3. All tables include tenant_id for future multi-tenancy
-- 4. All tables include sync_version for CRDT conflict resolution
-- 5. Timestamps are ISO8601 strings (SQLite has no native datetime)
-- =============================================================================

-- =============================================================================
-- Configuration Table
-- =============================================================================
-- Stores key-value configuration for the app.
-- Used for tenant settings, device info, etc.
--
-- Example entries:
--   key: "tenant_id"          value: "00000000-0000-0000-0000-000000000001"
--   key: "device_id"          value: "POS-001"
--   key: "tax_mode"           value: "exclusive"
--   key: "default_tax_rate"   value: "825" (8.25% in basis points)

CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Insert default configuration
INSERT OR IGNORE INTO config (key, value) VALUES 
    ('tenant_id', '00000000-0000-0000-0000-000000000001'),
    ('device_id', 'POS-001'),
    ('tax_mode', 'exclusive'),
    ('default_tax_rate', '825');

-- =============================================================================
-- Products Table
-- =============================================================================
-- Stores the product catalog.
--
-- Key Fields:
-- - id: UUID v4 (immutable, used for relations)
-- - sku: Business identifier (human-readable, searchable)
-- - price_cents: Price in cents (INTEGER, not REAL!)
-- - tax_rate_bps: Tax rate in basis points (825 = 8.25%)
-- - track_inventory: Whether to track stock levels
-- - allow_negative_stock: Whether to allow selling when stock is 0

CREATE TABLE IF NOT EXISTS products (
    -- Primary key: UUID v4 for distributed/offline safety
    id TEXT PRIMARY KEY NOT NULL,
    
    -- Multi-tenancy support (v0.1: hardcoded to default tenant)
    tenant_id TEXT NOT NULL DEFAULT '00000000-0000-0000-0000-000000000001',
    
    -- Business identifiers (searchable)
    sku TEXT NOT NULL,
    barcode TEXT,
    
    -- Display information
    name TEXT NOT NULL,
    description TEXT,
    
    -- Pricing (in cents - NEVER use REAL for money!)
    price_cents INTEGER NOT NULL,
    cost_cents INTEGER,  -- Optional: for profit margin calculations
    
    -- Tax configuration
    -- Stored in basis points: 825 = 8.25%
    tax_rate_bps INTEGER NOT NULL DEFAULT 0,
    
    -- Inventory tracking configuration
    track_inventory INTEGER NOT NULL DEFAULT 1,  -- SQLite uses 0/1 for boolean
    allow_negative_stock INTEGER NOT NULL DEFAULT 0,
    current_stock INTEGER DEFAULT 0,
    
    -- Soft delete (never hard delete products - they may be referenced by sales)
    is_active INTEGER NOT NULL DEFAULT 1,
    
    -- Timestamps (ISO8601 format)
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- CRDT support: incremented on each change for conflict resolution
    sync_version INTEGER NOT NULL DEFAULT 0,
    
    -- Unique constraint: SKU must be unique per tenant
    UNIQUE(tenant_id, sku)
);

-- Index for fast barcode lookups (barcode scanner)
CREATE INDEX IF NOT EXISTS idx_products_barcode ON products(barcode) WHERE barcode IS NOT NULL;

-- Index for active products (used in search)
CREATE INDEX IF NOT EXISTS idx_products_active ON products(is_active, tenant_id);

-- =============================================================================
-- Sales Table
-- =============================================================================
-- Stores sales/transactions.
--
-- Status flow: draft → completed → (optionally) voided
--
-- A sale starts as 'draft' when items are added to cart.
-- It becomes 'completed' when fully paid.
-- It can be 'voided' for refunds/cancellations.

CREATE TABLE IF NOT EXISTS sales (
    id TEXT PRIMARY KEY NOT NULL,
    tenant_id TEXT NOT NULL DEFAULT '00000000-0000-0000-0000-000000000001',
    
    -- Human-readable receipt number: YYYYMMDD-DD-NNNN
    receipt_number TEXT NOT NULL,
    
    -- Status: draft, completed, voided
    status TEXT NOT NULL DEFAULT 'draft',
    
    -- Totals (all in cents)
    subtotal_cents INTEGER NOT NULL DEFAULT 0,  -- Sum of line totals before tax
    tax_cents INTEGER NOT NULL DEFAULT 0,       -- Total tax
    discount_cents INTEGER NOT NULL DEFAULT 0,  -- Total discounts
    total_cents INTEGER NOT NULL DEFAULT 0,     -- Final amount due
    
    -- Who/where
    user_id TEXT NOT NULL,    -- Cashier who created the sale
    device_id TEXT NOT NULL,  -- POS terminal ID
    
    -- Optional notes
    notes TEXT,
    
    -- Timestamps
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT,  -- When sale was finalized (paid)
    
    -- CRDT support
    sync_version INTEGER NOT NULL DEFAULT 0,
    
    -- Receipt number unique per tenant
    UNIQUE(tenant_id, receipt_number)
);

-- Index for querying sales by status
CREATE INDEX IF NOT EXISTS idx_sales_status ON sales(tenant_id, status);

-- Index for date-based queries (reporting)
CREATE INDEX IF NOT EXISTS idx_sales_created ON sales(tenant_id, created_at);

-- =============================================================================
-- Sale Items Table
-- =============================================================================
-- Line items within a sale.
--
-- IMPORTANT: We "snapshot" product data at time of sale.
-- This preserves historical accuracy even if product details change later.
--
-- Example:
--   Product "Coke" price changes from $2.50 to $2.75
--   Historical sales still show $2.50 (the price at time of sale)

CREATE TABLE IF NOT EXISTS sale_items (
    id TEXT PRIMARY KEY NOT NULL,
    sale_id TEXT NOT NULL,
    product_id TEXT NOT NULL,
    
    -- Snapshot data (frozen at time of sale)
    sku_snapshot TEXT NOT NULL,
    name_snapshot TEXT NOT NULL,
    unit_price_cents INTEGER NOT NULL,
    
    -- Quantity and totals
    quantity INTEGER NOT NULL,
    line_total_cents INTEGER NOT NULL,  -- unit_price × quantity
    tax_cents INTEGER NOT NULL DEFAULT 0,
    discount_cents INTEGER NOT NULL DEFAULT 0,
    
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- Foreign keys
    FOREIGN KEY (sale_id) REFERENCES sales(id),
    FOREIGN KEY (product_id) REFERENCES products(id)
);

-- Index for querying items by sale
CREATE INDEX IF NOT EXISTS idx_sale_items_sale ON sale_items(sale_id);

-- =============================================================================
-- Payments Table
-- =============================================================================
-- Payments made towards a sale.
--
-- A sale can have multiple payments (split payment):
--   Payment 1: Cash $20
--   Payment 2: Card $15.50
--
-- For cash payments, we track tendered amount and change.

CREATE TABLE IF NOT EXISTS payments (
    id TEXT PRIMARY KEY NOT NULL,
    sale_id TEXT NOT NULL,
    
    -- Payment method: cash, external_card
    method TEXT NOT NULL,
    
    -- Amount paid (in cents)
    amount_cents INTEGER NOT NULL,
    
    -- For cash: amount given by customer
    tendered_cents INTEGER,
    -- For cash: change returned
    change_cents INTEGER,
    
    -- External reference (card auth code, etc.)
    reference TEXT,
    
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    FOREIGN KEY (sale_id) REFERENCES sales(id)
);

-- Index for querying payments by sale
CREATE INDEX IF NOT EXISTS idx_payments_sale ON payments(sale_id);

-- =============================================================================
-- Sync Outbox Table
-- =============================================================================
-- Queue for offline-to-cloud synchronization.
--
-- The Outbox Pattern:
-- 1. Local operation + outbox entry are in the SAME transaction
-- 2. Background worker reads pending entries
-- 3. Worker sends to cloud, marks as synced on success
-- 4. Never loses data, even if offline for extended periods

CREATE TABLE IF NOT EXISTS sync_outbox (
    id TEXT PRIMARY KEY NOT NULL,
    tenant_id TEXT NOT NULL DEFAULT '00000000-0000-0000-0000-000000000001',
    
    -- What entity is being synced
    entity_type TEXT NOT NULL,  -- 'SALE', 'PRODUCT', 'PAYMENT', etc.
    entity_id TEXT NOT NULL,    -- UUID of the entity
    
    -- Full JSON payload of the entity
    payload TEXT NOT NULL,
    
    -- Retry tracking
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    
    -- Timestamps
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    attempted_at TEXT,  -- Last sync attempt
    synced_at TEXT,     -- When successfully synced (NULL = pending)
    
    -- Index for finding pending entries
    UNIQUE(entity_type, entity_id)  -- Prevent duplicate queue entries
);

-- Index for finding pending sync entries
CREATE INDEX IF NOT EXISTS idx_sync_outbox_pending ON sync_outbox(synced_at) WHERE synced_at IS NULL;

-- Index for cleanup of old synced entries
CREATE INDEX IF NOT EXISTS idx_sync_outbox_synced ON sync_outbox(synced_at) WHERE synced_at IS NOT NULL;
