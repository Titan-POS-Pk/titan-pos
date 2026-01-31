//! # Product Repository
//!
//! Database operations for products.
//!
//! ## Key Operations
//! - Full-text search using FTS5
//! - CRUD operations
//! - Inventory updates
//!
//! ## FTS5 Search
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    How FTS5 Search Works                                │
//! │                                                                         │
//! │  User types: "coke"                                                    │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  FTS5 searches across: sku, name, barcode                              │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  ┌─────────────────────────────────────────┐                           │
//! │  │ products_fts (virtual table)            │                           │
//! │  │                                         │                           │
//! │  │ COKE-330  | Coca-Cola 330ml | 54490... │ ← MATCH!                  │
//! │  │ COKE-500  | Coca-Cola 500ml | 54490... │ ← MATCH!                  │
//! │  │ PEPSI-330 | Pepsi 330ml     | 12345... │                           │
//! │  └─────────────────────────────────────────┘                           │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  Results: [COKE-330, COKE-500]                                         │
//! │                                                                         │
//! │  Performance: <10ms for 50,000 products (indexed search)               │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use chrono::Utc;
use sqlx::SqlitePool;
use tracing::debug;
use uuid::Uuid;

use crate::error::{DbError, DbResult};
use titan_core::{Product, DEFAULT_TENANT_ID};

/// Repository for product database operations.
///
/// ## Usage
/// ```rust,ignore
/// let repo = ProductRepository::new(pool);
///
/// // Search products
/// let results = repo.search("coke", 20).await?;
///
/// // Get by ID
/// let product = repo.get_by_id("uuid-here").await?;
/// ```
#[derive(Debug, Clone)]
pub struct ProductRepository {
    pool: SqlitePool,
}

impl ProductRepository {
    /// Creates a new ProductRepository.
    pub fn new(pool: SqlitePool) -> Self {
        ProductRepository { pool }
    }

    /// Searches products using full-text search.
    ///
    /// ## How It Works
    /// 1. Uses FTS5 virtual table for instant search
    /// 2. Searches across: SKU, name, barcode
    /// 3. Returns products ordered by relevance
    ///
    /// ## Performance
    /// - Target: <10ms for 50,000 products
    /// - Uses FTS5 MATCH (not LIKE which would be slow)
    ///
    /// ## Arguments
    /// * `query` - Search term (can be partial)
    /// * `limit` - Maximum results to return
    ///
    /// ## Example
    /// ```rust,ignore
    /// // Search for "coke"
    /// let products = repo.search("coke", 20).await?;
    ///
    /// // Empty query returns recent/popular products
    /// let products = repo.search("", 20).await?;
    /// ```
    pub async fn search(&self, query: &str, limit: u32) -> DbResult<Vec<Product>> {
        let query = query.trim();

        debug!(query = %query, limit = %limit, "Searching products");

        // If query is empty, return active products (could be sorted by popularity later)
        if query.is_empty() {
            return self.list_active(limit).await;
        }

        // FTS5 search with wildcard suffix for prefix matching
        // "coke" becomes "coke*" to match "coke", "coke-330", etc.
        let fts_query = format!("{}*", query);

        // Query using FTS5 MATCH
        // We join back to products table to get all columns
        let products = sqlx::query_as!(
            Product,
            r#"
            SELECT 
                p.id,
                p.tenant_id,
                p.sku,
                p.barcode,
                p.name,
                p.description,
                p.price_cents,
                p.cost_cents,
                p.tax_rate_bps as "tax_rate_bps: u32",
                p.track_inventory as "track_inventory: bool",
                p.allow_negative_stock as "allow_negative_stock: bool",
                p.current_stock,
                p.is_active as "is_active: bool",
                p.created_at as "created_at: chrono::DateTime<Utc>",
                p.updated_at as "updated_at: chrono::DateTime<Utc>",
                p.sync_version
            FROM products p
            INNER JOIN products_fts fts ON p.rowid = fts.rowid
            WHERE products_fts MATCH ?1
            AND p.is_active = 1
            ORDER BY rank
            LIMIT ?2
            "#,
            fts_query,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        debug!(count = products.len(), "Search returned products");
        Ok(products)
    }

    /// Lists active products (no search filter).
    ///
    /// ## Usage
    /// Called when search query is empty.
    /// Returns products sorted by name.
    async fn list_active(&self, limit: u32) -> DbResult<Vec<Product>> {
        let products = sqlx::query_as!(
            Product,
            r#"
            SELECT 
                id,
                tenant_id,
                sku,
                barcode,
                name,
                description,
                price_cents,
                cost_cents,
                tax_rate_bps as "tax_rate_bps: u32",
                track_inventory as "track_inventory: bool",
                allow_negative_stock as "allow_negative_stock: bool",
                current_stock,
                is_active as "is_active: bool",
                created_at as "created_at: chrono::DateTime<Utc>",
                updated_at as "updated_at: chrono::DateTime<Utc>",
                sync_version
            FROM products
            WHERE is_active = 1
            ORDER BY name
            LIMIT ?1
            "#,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(products)
    }

    /// Gets a product by its ID.
    ///
    /// ## Arguments
    /// * `id` - Product UUID
    ///
    /// ## Returns
    /// * `Ok(Some(Product))` - Product found
    /// * `Ok(None)` - Product not found
    pub async fn get_by_id(&self, id: &str) -> DbResult<Option<Product>> {
        let product = sqlx::query_as!(
            Product,
            r#"
            SELECT 
                id,
                tenant_id,
                sku,
                barcode,
                name,
                description,
                price_cents,
                cost_cents,
                tax_rate_bps as "tax_rate_bps: u32",
                track_inventory as "track_inventory: bool",
                allow_negative_stock as "allow_negative_stock: bool",
                current_stock,
                is_active as "is_active: bool",
                created_at as "created_at: chrono::DateTime<Utc>",
                updated_at as "updated_at: chrono::DateTime<Utc>",
                sync_version
            FROM products
            WHERE id = ?1
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(product)
    }

    /// Gets a product by its SKU.
    ///
    /// ## Arguments
    /// * `sku` - Product SKU (e.g., "COKE-330")
    ///
    /// ## Returns
    /// * `Ok(Some(Product))` - Product found
    /// * `Ok(None)` - Product not found
    pub async fn get_by_sku(&self, sku: &str) -> DbResult<Option<Product>> {
        let product = sqlx::query_as!(
            Product,
            r#"
            SELECT 
                id,
                tenant_id,
                sku,
                barcode,
                name,
                description,
                price_cents,
                cost_cents,
                tax_rate_bps as "tax_rate_bps: u32",
                track_inventory as "track_inventory: bool",
                allow_negative_stock as "allow_negative_stock: bool",
                current_stock,
                is_active as "is_active: bool",
                created_at as "created_at: chrono::DateTime<Utc>",
                updated_at as "updated_at: chrono::DateTime<Utc>",
                sync_version
            FROM products
            WHERE sku = ?1 AND tenant_id = ?2
            "#,
            sku,
            DEFAULT_TENANT_ID
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(product)
    }

    /// Inserts a new product.
    ///
    /// ## Arguments
    /// * `product` - Product to insert (id should be generated beforehand)
    ///
    /// ## Returns
    /// * `Ok(Product)` - Inserted product with generated fields
    /// * `Err(DbError::UniqueViolation)` - SKU already exists
    pub async fn insert(&self, product: &Product) -> DbResult<Product> {
        debug!(sku = %product.sku, "Inserting product");

        sqlx::query!(
            r#"
            INSERT INTO products (
                id, tenant_id, sku, barcode, name, description,
                price_cents, cost_cents, tax_rate_bps,
                track_inventory, allow_negative_stock, current_stock,
                is_active, created_at, updated_at, sync_version
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9,
                ?10, ?11, ?12,
                ?13, ?14, ?15, ?16
            )
            "#,
            product.id,
            product.tenant_id,
            product.sku,
            product.barcode,
            product.name,
            product.description,
            product.price_cents,
            product.cost_cents,
            product.tax_rate_bps,
            product.track_inventory,
            product.allow_negative_stock,
            product.current_stock,
            product.is_active,
            product.created_at,
            product.updated_at,
            product.sync_version
        )
        .execute(&self.pool)
        .await?;

        // Return the product as-is (it already has all fields)
        Ok(product.clone())
    }

    /// Updates an existing product.
    ///
    /// ## Arguments
    /// * `product` - Product with updated fields
    ///
    /// ## Returns
    /// * `Ok(())` - Update successful
    /// * `Err(DbError::NotFound)` - Product doesn't exist
    pub async fn update(&self, product: &Product) -> DbResult<()> {
        debug!(id = %product.id, "Updating product");

        let now = Utc::now();

        let result = sqlx::query!(
            r#"
            UPDATE products SET
                sku = ?2,
                barcode = ?3,
                name = ?4,
                description = ?5,
                price_cents = ?6,
                cost_cents = ?7,
                tax_rate_bps = ?8,
                track_inventory = ?9,
                allow_negative_stock = ?10,
                current_stock = ?11,
                is_active = ?12,
                updated_at = ?13,
                sync_version = sync_version + 1
            WHERE id = ?1
            "#,
            product.id,
            product.sku,
            product.barcode,
            product.name,
            product.description,
            product.price_cents,
            product.cost_cents,
            product.tax_rate_bps,
            product.track_inventory,
            product.allow_negative_stock,
            product.current_stock,
            product.is_active,
            now
        )
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::not_found("Product", &product.id));
        }

        Ok(())
    }

    /// Updates product stock level.
    ///
    /// ## CRDT Delta Pattern
    /// ```text
    /// ┌─────────────────────────────────────────────────────────────────────┐
    /// │                    Stock Update Strategy                            │
    /// │                                                                     │
    /// │  ❌ WRONG: Absolute update (causes sync conflicts)                 │
    /// │     UPDATE products SET current_stock = 7 WHERE id = ?             │
    /// │                                                                     │
    /// │  ✅ CORRECT: Delta update (CRDT-friendly)                          │
    /// │     UPDATE products SET current_stock = current_stock - 3          │
    /// │                                                                     │
    /// │  Why?                                                               │
    /// │  Terminal A: sells 3 → stock - 3                                   │
    /// │  Terminal B: sells 2 → stock - 2                                   │
    /// │  Both can sync without conflict: -3 + -2 = -5 total               │
    /// └─────────────────────────────────────────────────────────────────────┘
    /// ```
    ///
    /// ## Arguments
    /// * `id` - Product ID
    /// * `delta` - Change in stock (negative for sales, positive for restocking)
    pub async fn update_stock(&self, id: &str, delta: i32) -> DbResult<()> {
        debug!(id = %id, delta = %delta, "Updating stock");

        let now = Utc::now();

        let result = sqlx::query!(
            r#"
            UPDATE products 
            SET 
                current_stock = COALESCE(current_stock, 0) + ?2,
                updated_at = ?3,
                sync_version = sync_version + 1
            WHERE id = ?1
            "#,
            id,
            delta,
            now
        )
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::not_found("Product", id));
        }

        Ok(())
    }

    /// Soft-deletes a product by setting is_active = false.
    ///
    /// ## Why Soft Delete?
    /// - Historical sales still reference this product
    /// - Can be restored if deleted by mistake
    /// - Sync can propagate the deletion
    pub async fn soft_delete(&self, id: &str) -> DbResult<()> {
        debug!(id = %id, "Soft-deleting product");

        let now = Utc::now();

        let result = sqlx::query!(
            r#"
            UPDATE products 
            SET 
                is_active = 0,
                updated_at = ?2,
                sync_version = sync_version + 1
            WHERE id = ?1
            "#,
            id,
            now
        )
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::not_found("Product", id));
        }

        Ok(())
    }

    /// Counts total products (for diagnostics).
    pub async fn count(&self) -> DbResult<i64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM products WHERE is_active = 1")
            .fetch_one(&self.pool)
            .await?;

        Ok(count)
    }
}

/// Helper to generate a new product ID.
///
/// ## Usage
/// ```rust,ignore
/// let id = generate_product_id();
/// let product = Product { id, ... };
/// ```
pub fn generate_product_id() -> String {
    Uuid::new_v4().to_string()
}
