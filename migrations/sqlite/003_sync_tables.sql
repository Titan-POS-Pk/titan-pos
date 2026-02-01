-- Migration: 003_sync_tables.sql
-- Description: Tables for sync infrastructure (v0.2 Milestone 1)
--
-- Architecture Overview:
-- ┌──────────────────────────────────────────────────────────────────────────────┐
-- │                           Sync Infrastructure                                │
-- │                                                                              │
-- │  ┌─────────────────┐    ┌─────────────────┐    ┌─────────────────────────┐  │
-- │  │  inventory_     │    │  sync_cursors   │    │     node_state          │  │
-- │  │  deltas         │    │                 │    │                         │  │
-- │  │                 │    │ Track position  │    │ Device registration &   │  │
-- │  │ CRDT audit      │    │ in sync stream  │    │ election state          │  │
-- │  │ trail for       │    │ for replay/     │    │                         │  │
-- │  │ inventory       │    │ resume          │    │                         │  │
-- │  └─────────────────┘    └─────────────────┘    └─────────────────────────┘  │
-- └──────────────────────────────────────────────────────────────────────────────┘
--
-- Purpose:
-- 1. inventory_deltas: CRDT-style audit trail for inventory changes
-- 2. sync_cursors: Track sync position for replay and resume capabilities
-- 3. node_state: Device registration, discovery, and election state

--------------------------------------------------------------------------------
-- Table: inventory_deltas
--------------------------------------------------------------------------------
-- CRDT-style audit trail for inventory changes.
-- Instead of syncing absolute stock values (which cause conflicts), we sync
-- deltas (changes). Each node can independently apply deltas to derive the
-- current stock level.
--
-- Example Flow:
-- ┌──────────────────────────────────────────────────────────────────────────┐
-- │  Counter 1: Sells 2 units    │    Counter 2: Sells 3 units              │
-- │  delta = -2                  │    delta = -3                            │
-- │                              │                                          │
-- │  Both send deltas to hub     │                                          │
-- │           │                  │           │                              │
-- │           └──────────────────┴───────────┘                              │
-- │                              │                                          │
-- │                              ▼                                          │
-- │                      Hub receives both:                                 │
-- │                      delta_1 = -2                                       │
-- │                      delta_2 = -3                                       │
-- │                      Total change = -5                                  │
-- │                                                                         │
-- │                      No conflict! Stock correctly reduced by 5          │
-- └──────────────────────────────────────────────────────────────────────────┘
--
CREATE TABLE IF NOT EXISTS inventory_deltas (
    -- Primary key: UUID v4 for this delta entry
    id TEXT PRIMARY KEY NOT NULL,
    
    -- Foreign key to products table (the product whose inventory changed)
    product_id TEXT NOT NULL,
    
    -- The change in inventory (positive = received, negative = sold/adjusted)
    -- Using integers to avoid floating point issues
    delta INTEGER NOT NULL,
    
    -- The type of change for audit purposes
    -- Values: 'sale', 'adjustment', 'receiving', 'transfer', 'return', 'damage'
    delta_type TEXT NOT NULL DEFAULT 'sale',
    
    -- Reference to the transaction that caused this delta (e.g., sale_id)
    reference_id TEXT,
    
    -- Reference type: 'sale', 'adjustment', 'transfer', 'receiving'
    reference_type TEXT,
    
    -- Device that originated this delta
    origin_device_id TEXT NOT NULL,
    
    -- Wall-clock timestamp when the delta occurred (ISO8601)
    occurred_at TEXT NOT NULL,
    
    -- Logical timestamp / vector clock component for ordering
    -- Higher sequence = later in time from this device
    sequence_num INTEGER NOT NULL,
    
    -- Whether this delta has been sent to the sync hub
    synced INTEGER NOT NULL DEFAULT 0,
    
    -- When the delta was synced (NULL if not yet synced)
    synced_at TEXT,
    
    -- Standard audit columns
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- Foreign key constraint (products table from 001_initial_schema.sql)
    FOREIGN KEY(product_id) REFERENCES products(id)
);

-- Index for finding unsynced deltas (for outbox processor)
CREATE INDEX IF NOT EXISTS idx_inventory_deltas_unsynced 
    ON inventory_deltas(synced, created_at) 
    WHERE synced = 0;

-- Index for querying deltas by product (for recalculating stock)
CREATE INDEX IF NOT EXISTS idx_inventory_deltas_product 
    ON inventory_deltas(product_id, occurred_at);

-- Index for finding deltas by reference (e.g., all deltas for a sale)
CREATE INDEX IF NOT EXISTS idx_inventory_deltas_reference 
    ON inventory_deltas(reference_type, reference_id);

-- Index for querying by device (useful for debugging sync issues)
CREATE INDEX IF NOT EXISTS idx_inventory_deltas_device 
    ON inventory_deltas(origin_device_id, sequence_num);

--------------------------------------------------------------------------------
-- Table: sync_cursors
--------------------------------------------------------------------------------
-- Track sync position for replay and resume capabilities.
-- When a device reconnects after being offline, it needs to know where to
-- resume from. Cursors store the last successfully processed position for
-- each sync stream.
--
-- Streams:
-- - 'outbox': Last outbox entry successfully sent to hub
-- - 'inbound': Last inbound update successfully applied from hub
-- - 'inventory': Last inventory delta processed
--
CREATE TABLE IF NOT EXISTS sync_cursors (
    -- Stream identifier (e.g., 'outbox', 'inbound', 'inventory')
    stream_id TEXT PRIMARY KEY NOT NULL,
    
    -- Last successfully processed sequence number
    -- For outbox: last sync_outbox.id that was acknowledged
    -- For inbound: last message sequence from hub
    last_sequence INTEGER NOT NULL DEFAULT 0,
    
    -- Last processed timestamp (ISO8601)
    -- Used for time-based cursoring when sequence isn't available
    last_timestamp TEXT,
    
    -- Additional cursor metadata (JSON)
    -- Can store things like partition info, batch IDs, etc.
    metadata TEXT,
    
    -- When this cursor was last updated
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Pre-populate default cursors
INSERT OR IGNORE INTO sync_cursors (stream_id, last_sequence, updated_at) 
VALUES 
    ('outbox', 0, datetime('now')),
    ('inbound', 0, datetime('now')),
    ('inventory', 0, datetime('now'));

--------------------------------------------------------------------------------
-- Table: node_state
--------------------------------------------------------------------------------
-- Device registration, discovery, and election state.
-- Tracks all known nodes in the store cluster and manages leader election.
--
-- Election Process:
-- ┌──────────────────────────────────────────────────────────────────────────┐
-- │  1. Device boots, generates UUID, registers in node_state               │
-- │  2. Device queries for existing primary (node_role = 'primary')         │
-- │  3. If no primary exists OR primary hasn't heartbeat recently:          │
-- │     a. Device announces candidacy (node_role = 'candidate')             │
-- │     b. Waits for election timeout (randomized to prevent split-brain)   │
-- │     c. If no other candidate with higher priority, becomes primary      │
-- │  4. Primary sends heartbeats, secondaries track last_seen_at            │
-- │  5. If primary heartbeat > timeout, trigger new election                │
-- └──────────────────────────────────────────────────────────────────────────┘
--
CREATE TABLE IF NOT EXISTS node_state (
    -- Device UUID (same as device_id in config)
    device_id TEXT PRIMARY KEY NOT NULL,
    
    -- Human-readable device name (e.g., "Counter 1", "Backoffice PC")
    device_name TEXT,
    
    -- Store ID this device belongs to
    store_id TEXT NOT NULL,
    
    -- Current role in the cluster
    -- Values: 'primary', 'secondary', 'candidate', 'offline'
    node_role TEXT NOT NULL DEFAULT 'secondary',
    
    -- Priority for leader election (higher = more likely to become primary)
    -- Can be configured based on hardware capabilities, network position, etc.
    election_priority INTEGER NOT NULL DEFAULT 100,
    
    -- Election term number (incremented on each election)
    -- Used to prevent stale election messages from causing issues
    election_term INTEGER NOT NULL DEFAULT 0,
    
    -- Device capabilities (JSON)
    -- e.g., {"can_be_primary": true, "has_printer": true, "has_scanner": true}
    capabilities TEXT,
    
    -- IP address and port for direct communication
    -- NULL if not yet discovered
    ip_address TEXT,
    port INTEGER,
    
    -- When this device was last seen (heartbeat timestamp)
    last_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- When this device first registered
    registered_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- Sync version for CRDT merge (last-write-wins with this version)
    sync_version INTEGER NOT NULL DEFAULT 0,
    
    -- Whether this is the local device (only one row should have this = 1)
    is_local INTEGER NOT NULL DEFAULT 0
);

-- Index for finding the current primary
CREATE INDEX IF NOT EXISTS idx_node_state_primary 
    ON node_state(store_id, node_role) 
    WHERE node_role = 'primary';

-- Index for finding active nodes (for discovery)
CREATE INDEX IF NOT EXISTS idx_node_state_active 
    ON node_state(store_id, last_seen_at);

-- Ensure only one local device
CREATE UNIQUE INDEX IF NOT EXISTS idx_node_state_local 
    ON node_state(is_local) 
    WHERE is_local = 1;

--------------------------------------------------------------------------------
-- Table: sync_conflicts
--------------------------------------------------------------------------------
-- Log of sync conflicts for debugging and manual resolution.
-- When a version conflict occurs, we log it here for later analysis.
--
CREATE TABLE IF NOT EXISTS sync_conflicts (
    -- Auto-increment ID for ordering
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    
    -- Entity that had the conflict
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    
    -- Local version that was overwritten (or would have been)
    local_version INTEGER NOT NULL,
    
    -- Incoming version from sync
    incoming_version INTEGER NOT NULL,
    
    -- Resolution action taken
    -- Values: 'accepted', 'rejected', 'merged', 'manual'
    resolution TEXT NOT NULL,
    
    -- Snapshot of local data before resolution (JSON)
    local_snapshot TEXT,
    
    -- Snapshot of incoming data (JSON)
    incoming_snapshot TEXT,
    
    -- When the conflict occurred
    occurred_at TEXT NOT NULL DEFAULT (datetime('now')),
    
    -- Source device that sent the conflicting update
    source_device_id TEXT
);

-- Index for finding conflicts by entity
CREATE INDEX IF NOT EXISTS idx_sync_conflicts_entity 
    ON sync_conflicts(entity_type, entity_id, occurred_at);

--------------------------------------------------------------------------------
-- Triggers: Inventory delta creation on sales
--------------------------------------------------------------------------------
-- Automatically create inventory deltas when sale_items are inserted.
-- This ensures every sale generates the proper delta for sync.
--
-- Note: This trigger creates deltas for products with track_inventory = 1.
-- The delta is initially unsynced and will be picked up by the outbox processor.
--

-- We need the local device_id for the trigger. Store it in a temp table or
-- use a default. For now, we'll use 'local' and let the app update it.

CREATE TRIGGER IF NOT EXISTS trg_sale_item_inventory_delta
AFTER INSERT ON sale_items
WHEN (SELECT track_inventory FROM products WHERE id = NEW.product_id) = 1
BEGIN
    INSERT INTO inventory_deltas (
        id,
        product_id,
        delta,
        delta_type,
        reference_id,
        reference_type,
        origin_device_id,
        occurred_at,
        sequence_num,
        synced
    )
    VALUES (
        lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || 
              substr(hex(randomblob(2)), 2) || '-' || 
              substr('89ab', abs(random()) % 4 + 1, 1) || 
              substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6))),
        NEW.product_id,
        -NEW.quantity,  -- Negative because sales reduce inventory
        'sale',
        NEW.sale_id,
        'sale',
        COALESCE((SELECT device_id FROM node_state WHERE is_local = 1), 'local'),
        datetime('now'),
        COALESCE((SELECT MAX(sequence_num) + 1 FROM inventory_deltas 
                  WHERE origin_device_id = (SELECT device_id FROM node_state WHERE is_local = 1)), 1),
        0  -- Not yet synced
    );
END;

--------------------------------------------------------------------------------
-- Migration metadata
--------------------------------------------------------------------------------
-- Track that this migration has been applied
INSERT OR IGNORE INTO _sqlx_migrations (version, description, installed_on, success, checksum)
SELECT 3, 'sync_tables', datetime('now'), 1, X'0000'
WHERE NOT EXISTS (SELECT 1 FROM _sqlx_migrations WHERE version = 3);
