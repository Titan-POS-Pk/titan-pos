//! Database layer for Cloud API.
//!
//! Provides PostgreSQL connectivity and repository methods.

use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::Row;
use tracing::info;
use uuid::Uuid;
use chrono::{DateTime, Utc};

use crate::error::CloudError;

/// Database connection pool.
#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    /// Connect to the database.
    pub async fn connect(url: &str) -> Result<Self, CloudError> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .connect(url)
            .await
            .map_err(|e| CloudError::Database(e.to_string()))?;

        Ok(Database { pool })
    }

    /// Run database migrations.
    pub async fn run_migrations(&self) -> Result<(), CloudError> {
        sqlx::migrate!("../../migrations/postgres")
            .run(&self.pool)
            .await
            .map_err(|e| CloudError::Migration(e.to_string()))?;
        Ok(())
    }

    /// Get the connection pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    // =========================================================================
    // Store Operations
    // =========================================================================

    /// Validate a store's API key.
    pub async fn validate_api_key(
        &self,
        api_key: &str,
        store_id: &str,
        tenant_id: &str,
    ) -> Result<Option<StoreRecord>, CloudError> {
        let result = sqlx::query_as::<_, StoreRecord>(
            r#"
            SELECT 
                id, tenant_id, name, api_key_hash, is_active,
                created_at, updated_at
            FROM stores
            WHERE id = $1 AND tenant_id = $2 AND is_active = true
            "#
        )
        .bind(store_id)
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CloudError::Database(e.to_string()))?;

        // Verify API key hash
        if let Some(ref store) = result {
            if !verify_api_key(api_key, &store.api_key_hash) {
                return Ok(None);
            }
        }

        Ok(result)
    }

    /// Get store by ID.
    pub async fn get_store(&self, store_id: &str) -> Result<Option<StoreRecord>, CloudError> {
        let result = sqlx::query_as::<_, StoreRecord>(
            r#"
            SELECT 
                id, tenant_id, name, api_key_hash, is_active,
                created_at, updated_at
            FROM stores
            WHERE id = $1
            "#
        )
        .bind(store_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CloudError::Database(e.to_string()))?;

        Ok(result)
    }

    // =========================================================================
    // Sync Operations
    // =========================================================================

    /// Insert a sale record.
    pub async fn insert_sale(&self, sale: &SaleRecord) -> Result<(), CloudError> {
        sqlx::query(
            r#"
            INSERT INTO sales (
                id, store_id, device_id, tenant_id, receipt_number,
                subtotal_cents, tax_amount_cents, discount_amount_cents, total_cents,
                status, created_at, completed_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (id) DO UPDATE SET
                status = EXCLUDED.status,
                completed_at = EXCLUDED.completed_at,
                updated_at = NOW()
            "#
        )
        .bind(&sale.id)
        .bind(&sale.store_id)
        .bind(&sale.device_id)
        .bind(&sale.tenant_id)
        .bind(&sale.receipt_number)
        .bind(sale.subtotal_cents)
        .bind(sale.tax_amount_cents)
        .bind(sale.discount_amount_cents)
        .bind(sale.total_cents)
        .bind(&sale.status)
        .bind(&sale.created_at)
        .bind(&sale.completed_at)
        .execute(&self.pool)
        .await
        .map_err(|e| CloudError::Database(e.to_string()))?;

        Ok(())
    }

    /// Insert a sale item.
    pub async fn insert_sale_item(&self, item: &SaleItemRecord) -> Result<(), CloudError> {
        sqlx::query(
            r#"
            INSERT INTO sale_items (
                id, sale_id, product_id, sku, name,
                quantity, unit_price_cents, line_total_cents,
                tax_amount_cents, tax_rate_bps
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (id) DO NOTHING
            "#
        )
        .bind(&item.id)
        .bind(&item.sale_id)
        .bind(&item.product_id)
        .bind(&item.sku)
        .bind(&item.name)
        .bind(item.quantity)
        .bind(item.unit_price_cents)
        .bind(item.line_total_cents)
        .bind(item.tax_amount_cents)
        .bind(item.tax_rate_bps)
        .execute(&self.pool)
        .await
        .map_err(|e| CloudError::Database(e.to_string()))?;

        Ok(())
    }

    /// Insert a payment record.
    pub async fn insert_payment(&self, payment: &PaymentRecord) -> Result<(), CloudError> {
        sqlx::query(
            r#"
            INSERT INTO payments (
                id, sale_id, store_id, tenant_id, method,
                amount_cents, change_given_cents, reference, authorization_code,
                created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (id) DO NOTHING
            "#
        )
        .bind(&payment.id)
        .bind(&payment.sale_id)
        .bind(&payment.store_id)
        .bind(&payment.tenant_id)
        .bind(&payment.method)
        .bind(payment.amount_cents)
        .bind(payment.change_given_cents)
        .bind(&payment.reference)
        .bind(&payment.authorization_code)
        .bind(&payment.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| CloudError::Database(e.to_string()))?;

        Ok(())
    }

    /// Apply an inventory delta (CRDT merge).
    pub async fn apply_inventory_delta(&self, delta: &InventoryDeltaRecord) -> Result<(), CloudError> {
        // Insert the delta record
        sqlx::query(
            r#"
            INSERT INTO inventory_deltas (
                id, store_id, device_id, tenant_id, product_id,
                delta, reason, reference_id, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (id) DO NOTHING
            "#
        )
        .bind(&delta.id)
        .bind(&delta.store_id)
        .bind(&delta.device_id)
        .bind(&delta.tenant_id)
        .bind(&delta.product_id)
        .bind(delta.delta)
        .bind(&delta.reason)
        .bind(&delta.reference_id)
        .bind(&delta.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| CloudError::Database(e.to_string()))?;

        // Update the aggregate inventory (CRDT: add the delta)
        sqlx::query(
            r#"
            INSERT INTO inventory (store_id, product_id, tenant_id, current_stock, updated_at)
            VALUES ($1, $2, $3, $4, NOW())
            ON CONFLICT (store_id, product_id) DO UPDATE SET
                current_stock = inventory.current_stock + EXCLUDED.current_stock,
                updated_at = NOW()
            "#
        )
        .bind(&delta.store_id)
        .bind(&delta.product_id)
        .bind(&delta.tenant_id)
        .bind(delta.delta)
        .execute(&self.pool)
        .await
        .map_err(|e| CloudError::Database(e.to_string()))?;

        Ok(())
    }

    /// Get pending product updates for a store.
    pub async fn get_pending_product_updates(
        &self,
        store_id: &str,
        since_version: i64,
        limit: i32,
    ) -> Result<Vec<ProductRecord>, CloudError> {
        let limit = if limit <= 0 { 100 } else { limit };
        
        let results = sqlx::query_as::<_, ProductRecord>(
            r#"
            SELECT 
                id, tenant_id, sku, name, barcode,
                price_cents, cost_cents, tax_rate_id, tax_rate_bps,
                track_inventory, current_stock, low_stock_threshold,
                is_active, category, department,
                created_at, updated_at, version
            FROM products
            WHERE tenant_id = (SELECT tenant_id FROM stores WHERE id = $1)
              AND version > $2
            ORDER BY version ASC
            LIMIT $3
            "#
        )
        .bind(store_id)
        .bind(since_version)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CloudError::Database(e.to_string()))?;

        Ok(results)
    }

    /// Update sync cursor for a store.
    pub async fn update_sync_cursor(
        &self,
        store_id: &str,
        stream: &str,
        position: i64,
    ) -> Result<(), CloudError> {
        sqlx::query(
            r#"
            INSERT INTO sync_cursors (store_id, stream, position, updated_at)
            VALUES ($1, $2, $3, NOW())
            ON CONFLICT (store_id, stream) DO UPDATE SET
                position = EXCLUDED.position,
                updated_at = NOW()
            "#
        )
        .bind(store_id)
        .bind(stream)
        .bind(position)
        .execute(&self.pool)
        .await
        .map_err(|e| CloudError::Database(e.to_string()))?;

        Ok(())
    }

    /// Get sync cursor for a store and stream.
    pub async fn get_sync_cursor(
        &self,
        store_id: &str,
        stream: &str,
    ) -> Result<Option<i64>, CloudError> {
        let result = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT position FROM sync_cursors
            WHERE store_id = $1 AND stream = $2
            "#
        )
        .bind(store_id)
        .bind(stream)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CloudError::Database(e.to_string()))?;

        Ok(result)
    }

    // =========================================================================
    // Config Operations
    // =========================================================================

    /// Get store configuration.
    pub async fn get_store_config(&self, store_id: &str) -> Result<Option<StoreConfigRecord>, CloudError> {
        let result = sqlx::query_as::<_, StoreConfigRecord>(
            r#"
            SELECT 
                store_id, tenant_id, store_name, address, city, state,
                postal_code, country, timezone, currency, tax_mode,
                allow_negative_inventory, receipt_header, receipt_footer,
                sync_batch_size, sync_interval_secs
            FROM store_configs
            WHERE store_id = $1
            "#
        )
        .bind(store_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CloudError::Database(e.to_string()))?;

        Ok(result)
    }
}

// =============================================================================
// Record Types
// =============================================================================

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StoreRecord {
    pub id: String,
    pub tenant_id: String,
    pub name: String,
    pub api_key_hash: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct SaleRecord {
    pub id: String,
    pub store_id: String,
    pub device_id: String,
    pub tenant_id: String,
    pub receipt_number: String,
    pub subtotal_cents: i64,
    pub tax_amount_cents: i64,
    pub discount_amount_cents: i64,
    pub total_cents: i64,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct SaleItemRecord {
    pub id: String,
    pub sale_id: String,
    pub product_id: String,
    pub sku: String,
    pub name: String,
    pub quantity: i32,
    pub unit_price_cents: i64,
    pub line_total_cents: i64,
    pub tax_amount_cents: i64,
    pub tax_rate_bps: i32,
}

#[derive(Debug, Clone)]
pub struct PaymentRecord {
    pub id: String,
    pub sale_id: String,
    pub store_id: String,
    pub tenant_id: String,
    pub method: String,
    pub amount_cents: i64,
    pub change_given_cents: i64,
    pub reference: Option<String>,
    pub authorization_code: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct InventoryDeltaRecord {
    pub id: String,
    pub store_id: String,
    pub device_id: String,
    pub tenant_id: String,
    pub product_id: String,
    pub delta: i32,
    pub reason: String,
    pub reference_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProductRecord {
    pub id: String,
    pub tenant_id: String,
    pub sku: String,
    pub name: String,
    pub barcode: Option<String>,
    pub price_cents: i64,
    pub cost_cents: Option<i64>,
    pub tax_rate_id: Option<String>,
    pub tax_rate_bps: i32,
    pub track_inventory: bool,
    pub current_stock: Option<i64>,
    pub low_stock_threshold: Option<i64>,
    pub is_active: bool,
    pub category: Option<String>,
    pub department: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StoreConfigRecord {
    pub store_id: String,
    pub tenant_id: String,
    pub store_name: String,
    pub address: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    pub timezone: Option<String>,
    pub currency: String,
    pub tax_mode: String,
    pub allow_negative_inventory: bool,
    pub receipt_header: Option<String>,
    pub receipt_footer: Option<String>,
    pub sync_batch_size: i32,
    pub sync_interval_secs: i32,
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Verify an API key against its hash.
fn verify_api_key(api_key: &str, hash: &str) -> bool {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};
    
    let parsed_hash = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    
    Argon2::default()
        .verify_password(api_key.as_bytes(), &parsed_hash)
        .is_ok()
}

/// Hash an API key for storage.
pub fn hash_api_key(api_key: &str) -> Result<String, CloudError> {
    use argon2::{
        password_hash::{rand_core::OsRng, SaltString},
        Argon2, PasswordHasher,
    };
    
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    
    let hash = argon2
        .hash_password(api_key.as_bytes(), &salt)
        .map_err(|e| CloudError::Internal(format!("Failed to hash API key: {}", e)))?;
    
    Ok(hash.to_string())
}
