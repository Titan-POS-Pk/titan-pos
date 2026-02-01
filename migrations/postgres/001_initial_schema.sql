-- =============================================================================
-- Titan POS Cloud Database - Initial Schema
-- =============================================================================
--
-- This migration creates the core tables for the cloud database that stores
-- synchronized data from all Store Hubs.
--
-- Architecture:
-- - Multi-tenant with tenant_id on every table
-- - Stores within tenants identified by store_id
-- - CRDT-style inventory tracking with delta operations
-- - Sync cursor tracking per store/stream

-- -----------------------------------------------------------------------------
-- Tenants - Top-level organization (e.g., "Target Corporation")
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tenants (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    
    -- Company settings
    currency TEXT NOT NULL DEFAULT 'USD',
    timezone TEXT NOT NULL DEFAULT 'UTC',
    
    -- Status
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for active tenants
CREATE INDEX IF NOT EXISTS idx_tenants_active ON tenants(is_active) WHERE is_active = TRUE;

-- -----------------------------------------------------------------------------
-- Stores - Physical locations within a tenant
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS stores (
    id TEXT PRIMARY KEY NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    name TEXT NOT NULL,
    
    -- API key (hashed with argon2)
    api_key_hash TEXT NOT NULL,
    
    -- Location
    address TEXT,
    city TEXT,
    state TEXT,
    postal_code TEXT,
    country TEXT DEFAULT 'USA',
    timezone TEXT DEFAULT 'UTC',
    
    -- Status
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_stores_tenant ON stores(tenant_id);
CREATE INDEX IF NOT EXISTS idx_stores_active ON stores(is_active) WHERE is_active = TRUE;

-- -----------------------------------------------------------------------------
-- Store Configurations - Runtime settings for each store
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS store_configs (
    store_id TEXT PRIMARY KEY NOT NULL REFERENCES stores(id),
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    store_name TEXT NOT NULL,
    
    -- Location (copied from stores for convenience)
    address TEXT,
    city TEXT,
    state TEXT,
    postal_code TEXT,
    country TEXT,
    timezone TEXT,
    
    -- Financial settings
    currency TEXT NOT NULL DEFAULT 'USD',
    tax_mode TEXT NOT NULL DEFAULT 'EXCLUSIVE', -- INCLUSIVE or EXCLUSIVE
    allow_negative_inventory BOOLEAN NOT NULL DEFAULT FALSE,
    
    -- Receipt customization
    receipt_header TEXT,
    receipt_footer TEXT,
    
    -- Sync settings
    sync_batch_size INTEGER NOT NULL DEFAULT 100,
    sync_interval_secs INTEGER NOT NULL DEFAULT 30,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_store_configs_tenant ON store_configs(tenant_id);

-- -----------------------------------------------------------------------------
-- Devices - POS terminals/computers registered to stores
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS devices (
    id TEXT PRIMARY KEY NOT NULL,
    store_id TEXT NOT NULL REFERENCES stores(id),
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    name TEXT NOT NULL,
    
    -- Device metadata
    device_type TEXT DEFAULT 'POS', -- POS, SERVER, MOBILE
    priority INTEGER NOT NULL DEFAULT 50, -- For leader election
    
    -- Last known state
    last_seen_at TIMESTAMPTZ,
    last_ip_address TEXT,
    
    -- Status
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_devices_store ON devices(store_id);
CREATE INDEX IF NOT EXISTS idx_devices_tenant ON devices(tenant_id);

-- -----------------------------------------------------------------------------
-- Products - Master product catalog (tenant-wide)
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS products (
    id TEXT PRIMARY KEY NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    
    -- Identification
    sku TEXT NOT NULL,
    name TEXT NOT NULL,
    barcode TEXT,
    
    -- Pricing (in cents)
    price_cents BIGINT NOT NULL,
    cost_cents BIGINT,
    
    -- Tax
    tax_rate_id TEXT,
    tax_rate_bps INTEGER NOT NULL DEFAULT 0, -- Basis points (e.g., 825 = 8.25%)
    
    -- Inventory settings
    track_inventory BOOLEAN NOT NULL DEFAULT TRUE,
    current_stock BIGINT, -- Aggregate across all stores (for reporting)
    low_stock_threshold BIGINT DEFAULT 10,
    
    -- Status
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    
    -- Categorization
    category TEXT,
    department TEXT,
    
    -- Versioning (for sync)
    version BIGINT NOT NULL DEFAULT 1,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Unique SKU per tenant
    CONSTRAINT unique_sku_per_tenant UNIQUE (tenant_id, sku)
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_products_tenant ON products(tenant_id);
CREATE INDEX IF NOT EXISTS idx_products_sku ON products(tenant_id, sku);
CREATE INDEX IF NOT EXISTS idx_products_barcode ON products(tenant_id, barcode) WHERE barcode IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_products_version ON products(tenant_id, version);
CREATE INDEX IF NOT EXISTS idx_products_active ON products(tenant_id, is_active) WHERE is_active = TRUE;

-- Full-text search
CREATE INDEX IF NOT EXISTS idx_products_name_gin ON products USING gin(to_tsvector('english', name));

-- -----------------------------------------------------------------------------
-- Inventory - Store-level inventory (CRDT aggregate)
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS inventory (
    store_id TEXT NOT NULL REFERENCES stores(id),
    product_id TEXT NOT NULL REFERENCES products(id),
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    
    -- Current stock level (result of CRDT merge)
    current_stock BIGINT NOT NULL DEFAULT 0,
    
    -- Timestamps
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    PRIMARY KEY (store_id, product_id)
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_inventory_product ON inventory(product_id);
CREATE INDEX IF NOT EXISTS idx_inventory_tenant ON inventory(tenant_id);

-- -----------------------------------------------------------------------------
-- Inventory Deltas - CRDT operation log
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS inventory_deltas (
    id TEXT PRIMARY KEY NOT NULL,
    store_id TEXT NOT NULL REFERENCES stores(id),
    device_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    product_id TEXT NOT NULL REFERENCES products(id),
    
    -- The delta value (positive = received, negative = sold)
    delta INTEGER NOT NULL,
    
    -- Reason for the change
    reason TEXT NOT NULL, -- SALE, VOID, ADJUSTMENT, TRANSFER_IN, TRANSFER_OUT, RECEIVE
    
    -- Reference to related entity
    reference_id TEXT,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL,
    synced_at TIMESTAMPTZ DEFAULT NOW()
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_inventory_deltas_store ON inventory_deltas(store_id);
CREATE INDEX IF NOT EXISTS idx_inventory_deltas_product ON inventory_deltas(product_id);
CREATE INDEX IF NOT EXISTS idx_inventory_deltas_tenant ON inventory_deltas(tenant_id);
CREATE INDEX IF NOT EXISTS idx_inventory_deltas_created ON inventory_deltas(created_at);

-- -----------------------------------------------------------------------------
-- Tax Rates - Tax rate definitions
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tax_rates (
    id TEXT PRIMARY KEY NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    name TEXT NOT NULL,
    rate_bps INTEGER NOT NULL, -- Basis points (e.g., 825 = 8.25%)
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tax_rates_tenant ON tax_rates(tenant_id);

-- -----------------------------------------------------------------------------
-- Sales - Sales transactions
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sales (
    id TEXT PRIMARY KEY NOT NULL,
    store_id TEXT NOT NULL REFERENCES stores(id),
    device_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    receipt_number TEXT NOT NULL,
    
    -- Amounts (in cents)
    subtotal_cents BIGINT NOT NULL DEFAULT 0,
    tax_amount_cents BIGINT NOT NULL DEFAULT 0,
    discount_amount_cents BIGINT NOT NULL DEFAULT 0,
    total_cents BIGINT NOT NULL DEFAULT 0,
    
    -- Status
    status TEXT NOT NULL DEFAULT 'PENDING', -- PENDING, COMPLETED, VOIDED, REFUNDED
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ,
    synced_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_sales_store ON sales(store_id);
CREATE INDEX IF NOT EXISTS idx_sales_device ON sales(device_id);
CREATE INDEX IF NOT EXISTS idx_sales_tenant ON sales(tenant_id);
CREATE INDEX IF NOT EXISTS idx_sales_receipt ON sales(store_id, receipt_number);
CREATE INDEX IF NOT EXISTS idx_sales_created ON sales(created_at);
CREATE INDEX IF NOT EXISTS idx_sales_status ON sales(status);

-- Partitioning hint (for large deployments, consider partitioning by tenant_id or created_at)
-- CREATE TABLE sales (...) PARTITION BY RANGE (created_at);

-- -----------------------------------------------------------------------------
-- Sale Items - Line items within sales
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sale_items (
    id TEXT PRIMARY KEY NOT NULL,
    sale_id TEXT NOT NULL REFERENCES sales(id),
    product_id TEXT NOT NULL REFERENCES products(id),
    
    -- Product snapshot (frozen at sale time)
    sku TEXT NOT NULL,
    name TEXT NOT NULL,
    
    -- Quantities and prices (in cents)
    quantity INTEGER NOT NULL,
    unit_price_cents BIGINT NOT NULL,
    line_total_cents BIGINT NOT NULL,
    tax_amount_cents BIGINT NOT NULL DEFAULT 0,
    tax_rate_bps INTEGER NOT NULL DEFAULT 0,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_sale_items_sale ON sale_items(sale_id);
CREATE INDEX IF NOT EXISTS idx_sale_items_product ON sale_items(product_id);

-- -----------------------------------------------------------------------------
-- Payments - Payment records
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS payments (
    id TEXT PRIMARY KEY NOT NULL,
    sale_id TEXT NOT NULL REFERENCES sales(id),
    store_id TEXT NOT NULL REFERENCES stores(id),
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    
    -- Payment details
    method TEXT NOT NULL, -- CASH, CARD, EXTERNAL
    amount_cents BIGINT NOT NULL,
    change_given_cents BIGINT NOT NULL DEFAULT 0,
    
    -- Reference (for card payments)
    reference TEXT,
    authorization_code TEXT,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL,
    synced_at TIMESTAMPTZ DEFAULT NOW()
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_payments_sale ON payments(sale_id);
CREATE INDEX IF NOT EXISTS idx_payments_store ON payments(store_id);
CREATE INDEX IF NOT EXISTS idx_payments_tenant ON payments(tenant_id);
CREATE INDEX IF NOT EXISTS idx_payments_created ON payments(created_at);

-- -----------------------------------------------------------------------------
-- Users - Store staff/cashiers
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY NOT NULL,
    store_id TEXT NOT NULL REFERENCES stores(id),
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    
    username TEXT NOT NULL,
    display_name TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'CASHIER', -- CASHIER, MANAGER, ADMIN
    
    -- Authentication
    pin_hash TEXT, -- Hashed PIN for POS login
    
    -- Status
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Unique username per store
    CONSTRAINT unique_username_per_store UNIQUE (store_id, username)
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_users_store ON users(store_id);
CREATE INDEX IF NOT EXISTS idx_users_tenant ON users(tenant_id);

-- -----------------------------------------------------------------------------
-- Sync Cursors - Track sync position per store/stream
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sync_cursors (
    store_id TEXT NOT NULL REFERENCES stores(id),
    stream TEXT NOT NULL, -- 'upload', 'download', 'products', 'inventory', etc.
    position BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    PRIMARY KEY (store_id, stream)
);

-- -----------------------------------------------------------------------------
-- Trigger: Auto-update updated_at
-- -----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Apply trigger to tables with updated_at
CREATE TRIGGER update_tenants_updated_at BEFORE UPDATE ON tenants FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_stores_updated_at BEFORE UPDATE ON stores FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_store_configs_updated_at BEFORE UPDATE ON store_configs FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_devices_updated_at BEFORE UPDATE ON devices FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_products_updated_at BEFORE UPDATE ON products FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_tax_rates_updated_at BEFORE UPDATE ON tax_rates FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_sales_updated_at BEFORE UPDATE ON sales FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_users_updated_at BEFORE UPDATE ON users FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- -----------------------------------------------------------------------------
-- Trigger: Auto-increment product version on update
-- -----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION increment_product_version()
RETURNS TRIGGER AS $$
BEGIN
    NEW.version = OLD.version + 1;
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER increment_products_version BEFORE UPDATE ON products FOR EACH ROW EXECUTE FUNCTION increment_product_version();
