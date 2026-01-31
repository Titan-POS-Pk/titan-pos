-- =============================================================================
-- Titan POS: Full-Text Search Setup
-- Migration: 002_add_fts.sql
-- =============================================================================
--
-- This migration sets up FTS5 (Full-Text Search) for fast product search.
--
-- ## Why FTS5?
-- ```
-- ┌─────────────────────────────────────────────────────────────────────────┐
-- │                    LIKE vs FTS5 Performance                             │
-- │                                                                         │
-- │  LIKE Query (slow):                                                     │
-- │  SELECT * FROM products WHERE name LIKE '%coke%'                        │
-- │  - Full table scan                                                      │
-- │  - 50,000 products: ~100ms+ 😰                                          │
-- │                                                                         │
-- │  FTS5 Query (fast):                                                     │
-- │  SELECT * FROM products_fts WHERE products_fts MATCH 'coke*'            │
-- │  - Uses inverted index                                                  │
-- │  - 50,000 products: <10ms 🚀                                            │
-- │                                                                         │
-- │  POS Requirement: Instant search as cashier types                       │
-- │  FTS5 is essential for good UX                                          │
-- └─────────────────────────────────────────────────────────────────────────┘
-- ```
--
-- ## How FTS5 Works
-- 1. Virtual table stores indexed tokens from sku, name, barcode
-- 2. Triggers keep FTS index in sync with products table
-- 3. MATCH queries use the index for O(log n) lookups
--
-- ## Search Examples
-- - "coke" matches "Coca-Cola", "COKE-330", "Diet Coke"
-- - "5449" matches barcode "5449000000996"
-- - "330" matches "COKE-330", "PEPSI-330"
-- =============================================================================

-- =============================================================================
-- FTS5 Virtual Table
-- =============================================================================
-- content='products': FTS5 will look up actual content from products table
-- content_rowid='rowid': Links FTS entries to products table rowid
--
-- This is a "content" FTS table, meaning it doesn't duplicate data.
-- The actual text is stored in products; FTS5 just maintains the index.

CREATE VIRTUAL TABLE IF NOT EXISTS products_fts USING fts5(
    sku,      -- Product code: "COKE-330"
    name,     -- Product name: "Coca-Cola 330ml"
    barcode,  -- Barcode: "5449000000996"
    content='products',
    content_rowid='rowid'
);

-- =============================================================================
-- Sync Triggers
-- =============================================================================
-- These triggers keep the FTS index in sync with the products table.
-- Any INSERT, UPDATE, or DELETE on products automatically updates the index.

-- Trigger: After INSERT on products
-- Adds the new product to the FTS index
CREATE TRIGGER IF NOT EXISTS products_ai AFTER INSERT ON products BEGIN
    INSERT INTO products_fts(rowid, sku, name, barcode) 
    VALUES (new.rowid, new.sku, new.name, new.barcode);
END;

-- Trigger: After DELETE on products
-- Removes the product from the FTS index
-- Note: We use soft delete (is_active=0), but this handles hard deletes too
CREATE TRIGGER IF NOT EXISTS products_ad AFTER DELETE ON products BEGIN
    INSERT INTO products_fts(products_fts, rowid, sku, name, barcode) 
    VALUES ('delete', old.rowid, old.sku, old.name, old.barcode);
END;

-- Trigger: After UPDATE on products
-- Updates the FTS index when product details change
-- FTS5 requires delete + insert to update (no direct update)
CREATE TRIGGER IF NOT EXISTS products_au AFTER UPDATE ON products BEGIN
    INSERT INTO products_fts(products_fts, rowid, sku, name, barcode) 
    VALUES ('delete', old.rowid, old.sku, old.name, old.barcode);
    INSERT INTO products_fts(rowid, sku, name, barcode) 
    VALUES (new.rowid, new.sku, new.name, new.barcode);
END;

-- =============================================================================
-- Populate FTS Index
-- =============================================================================
-- If there are existing products (from migration or seed), index them.
-- This is idempotent - safe to run multiple times.

INSERT OR REPLACE INTO products_fts(rowid, sku, name, barcode)
SELECT rowid, sku, name, barcode FROM products;
