//! # Sale Repository
//!
//! Database operations for sales and sale items.
//!
//! ## Sale Lifecycle
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                       Sale Lifecycle                                    │
//! │                                                                         │
//! │  1. CREATE DRAFT                                                       │
//! │     └── create_sale() → Sale { status: Draft }                         │
//! │                                                                         │
//! │  2. ADD ITEMS                                                          │
//! │     └── add_item() → SaleItem                                          │
//! │     └── add_item() → SaleItem                                          │
//! │     └── update_totals() → Recalculate subtotal, tax, total             │
//! │                                                                         │
//! │  3. FINALIZE                                                           │
//! │     └── finalize_sale() → Sale { status: Completed }                   │
//! │     └── (Also inserts into sync_outbox in same transaction)            │
//! │                                                                         │
//! │  4. (OPTIONAL) VOID                                                    │
//! │     └── void_sale() → Sale { status: Voided }                          │
//! │                                                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use chrono::Utc;
use sqlx::SqlitePool;
use tracing::debug;
use uuid::Uuid;

use crate::error::{DbError, DbResult};
use titan_core::{Payment, Sale, SaleItem, SaleStatus, DEFAULT_TENANT_ID};

/// Repository for sale database operations.
#[derive(Debug, Clone)]
pub struct SaleRepository {
    pool: SqlitePool,
}

impl SaleRepository {
    /// Creates a new SaleRepository.
    pub fn new(pool: SqlitePool) -> Self {
        SaleRepository { pool }
    }

    /// Gets a sale by ID.
    pub async fn get_by_id(&self, id: &str) -> DbResult<Option<Sale>> {
        let sale: Option<Sale> = sqlx::query_as!(
            Sale,
            r#"
            SELECT 
                id,
                tenant_id,
                receipt_number,
                status as "status: SaleStatus",
                subtotal_cents,
                tax_cents,
                discount_cents,
                total_cents,
                user_id,
                device_id,
                notes,
                created_at as "created_at: chrono::DateTime<Utc>",
                updated_at as "updated_at: chrono::DateTime<Utc>",
                completed_at as "completed_at: chrono::DateTime<Utc>",
                sync_version
            FROM sales
            WHERE id = ?1
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(sale)
    }

    /// Inserts a sale directly (used by commands layer).
    ///
    /// ## Arguments
    /// * `sale` - Complete sale object to insert
    pub async fn insert_sale(&self, sale: &Sale) -> DbResult<()> {
        debug!(id = %sale.id, receipt_number = %sale.receipt_number, "Inserting sale");

        sqlx::query!(
            r#"
            INSERT INTO sales (
                id, tenant_id, receipt_number, status,
                subtotal_cents, tax_cents, discount_cents, total_cents,
                user_id, device_id, notes,
                created_at, updated_at, completed_at, sync_version
            ) VALUES (
                ?1, ?2, ?3, ?4,
                ?5, ?6, ?7, ?8,
                ?9, ?10, ?11,
                ?12, ?13, ?14, ?15
            )
            "#,
            sale.id,
            sale.tenant_id,
            sale.receipt_number,
            sale.status,
            sale.subtotal_cents,
            sale.tax_cents,
            sale.discount_cents,
            sale.total_cents,
            sale.user_id,
            sale.device_id,
            sale.notes,
            sale.created_at,
            sale.updated_at,
            sale.completed_at,
            sale.sync_version
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Creates a new draft sale.
    ///
    /// ## Arguments
    /// * `user_id` - ID of the cashier creating the sale
    /// * `device_id` - ID of the POS terminal
    ///
    /// ## Returns
    /// The created sale with generated ID and receipt number.
    pub async fn create_sale(&self, user_id: &str, device_id: &str) -> DbResult<Sale> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let receipt_number = generate_receipt_number(device_id);

        debug!(id = %id, receipt_number = %receipt_number, "Creating sale");

        let sale = Sale {
            id: id.clone(),
            tenant_id: DEFAULT_TENANT_ID.to_string(),
            receipt_number,
            status: SaleStatus::Draft,
            subtotal_cents: 0,
            tax_cents: 0,
            discount_cents: 0,
            total_cents: 0,
            user_id: user_id.to_string(),
            device_id: device_id.to_string(),
            notes: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
            sync_version: 0,
        };

        sqlx::query!(
            r#"
            INSERT INTO sales (
                id, tenant_id, receipt_number, status,
                subtotal_cents, tax_cents, discount_cents, total_cents,
                user_id, device_id, notes,
                created_at, updated_at, completed_at, sync_version
            ) VALUES (
                ?1, ?2, ?3, ?4,
                ?5, ?6, ?7, ?8,
                ?9, ?10, ?11,
                ?12, ?13, ?14, ?15
            )
            "#,
            sale.id,
            sale.tenant_id,
            sale.receipt_number,
            sale.status,
            sale.subtotal_cents,
            sale.tax_cents,
            sale.discount_cents,
            sale.total_cents,
            sale.user_id,
            sale.device_id,
            sale.notes,
            sale.created_at,
            sale.updated_at,
            sale.completed_at,
            sale.sync_version
        )
        .execute(&self.pool)
        .await?;

        Ok(sale)
    }

    /// Adds an item to a sale.
    ///
    /// ## Snapshot Pattern
    /// Product details (sku, name, price) are copied to the sale item.
    /// This preserves the sale history even if product details change later.
    pub async fn add_item(&self, item: &SaleItem) -> DbResult<()> {
        debug!(sale_id = %item.sale_id, product_id = %item.product_id, "Adding sale item");

        sqlx::query!(
            r#"
            INSERT INTO sale_items (
                id, sale_id, product_id,
                sku_snapshot, name_snapshot, unit_price_cents,
                quantity, line_total_cents, tax_cents, discount_cents,
                created_at
            ) VALUES (
                ?1, ?2, ?3,
                ?4, ?5, ?6,
                ?7, ?8, ?9, ?10,
                ?11
            )
            "#,
            item.id,
            item.sale_id,
            item.product_id,
            item.sku_snapshot,
            item.name_snapshot,
            item.unit_price_cents,
            item.quantity,
            item.line_total_cents,
            item.tax_cents,
            item.discount_cents,
            item.created_at
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Gets all items for a sale.
    pub async fn get_items(&self, sale_id: &str) -> DbResult<Vec<SaleItem>> {
        let items: Vec<SaleItem> = sqlx::query_as!(
            SaleItem,
            r#"
            SELECT 
                id,
                sale_id,
                product_id,
                sku_snapshot,
                name_snapshot,
                unit_price_cents,
                quantity,
                line_total_cents,
                tax_cents,
                discount_cents,
                created_at as "created_at: chrono::DateTime<Utc>"
            FROM sale_items
            WHERE sale_id = ?1
            ORDER BY created_at
            "#,
            sale_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(items)
    }

    /// Updates sale totals.
    ///
    /// ## When To Call
    /// After adding/removing items from a sale.
    pub async fn update_totals(
        &self,
        sale_id: &str,
        subtotal_cents: i64,
        tax_cents: i64,
        discount_cents: i64,
        total_cents: i64,
    ) -> DbResult<()> {
        let now = Utc::now();

        let result: sqlx::sqlite::SqliteQueryResult = sqlx::query!(
            r#"
            UPDATE sales SET
                subtotal_cents = ?2,
                tax_cents = ?3,
                discount_cents = ?4,
                total_cents = ?5,
                updated_at = ?6
            WHERE id = ?1 AND status = 'draft'
            "#,
            sale_id,
            subtotal_cents,
            tax_cents,
            discount_cents,
            total_cents,
            now
        )
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::not_found("Sale", sale_id));
        }

        Ok(())
    }

    /// Finalizes a sale (marks as completed).
    ///
    /// ## What This Does
    /// 1. Updates sale status to Completed
    /// 2. Sets completed_at timestamp
    /// 3. Increments sync_version
    pub async fn finalize_sale(&self, sale_id: &str) -> DbResult<()> {
        let now = Utc::now();

        let result: sqlx::sqlite::SqliteQueryResult = sqlx::query!(
            r#"
            UPDATE sales SET
                status = 'completed',
                completed_at = ?2,
                updated_at = ?2,
                sync_version = sync_version + 1
            WHERE id = ?1 AND status = 'draft'
            "#,
            sale_id,
            now
        )
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::not_found("Sale (draft)", sale_id));
        }

        Ok(())
    }

    /// Voids a sale.
    pub async fn void_sale(&self, sale_id: &str) -> DbResult<()> {
        let now = Utc::now();

        let result: sqlx::sqlite::SqliteQueryResult = sqlx::query!(
            r#"
            UPDATE sales SET
                status = 'voided',
                updated_at = ?2,
                sync_version = sync_version + 1
            WHERE id = ?1 AND status IN ('draft', 'completed')
            "#,
            sale_id,
            now
        )
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::not_found("Sale", sale_id));
        }

        Ok(())
    }

    /// Records a payment for a sale.
    pub async fn add_payment(&self, payment: &Payment) -> DbResult<()> {
        debug!(sale_id = %payment.sale_id, amount = %payment.amount_cents, "Recording payment");

        sqlx::query!(
            r#"
            INSERT INTO payments (
                id, sale_id, method,
                amount_cents, tendered_cents, change_cents,
                reference, created_at
            ) VALUES (
                ?1, ?2, ?3,
                ?4, ?5, ?6,
                ?7, ?8
            )
            "#,
            payment.id,
            payment.sale_id,
            payment.method,
            payment.amount_cents,
            payment.tendered_cents,
            payment.change_cents,
            payment.reference,
            payment.created_at
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Gets all payments for a sale.
    pub async fn get_payments(&self, sale_id: &str) -> DbResult<Vec<Payment>> {
        let payments: Vec<Payment> = sqlx::query_as!(
            Payment,
            r#"
            SELECT 
                id,
                sale_id,
                method as "method: titan_core::PaymentMethod",
                amount_cents,
                tendered_cents,
                change_cents,
                reference,
                created_at as "created_at: chrono::DateTime<Utc>"
            FROM payments
            WHERE sale_id = ?1
            ORDER BY created_at
            "#,
            sale_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(payments)
    }

    /// Gets total amount paid for a sale.
    pub async fn get_total_paid(&self, sale_id: &str) -> DbResult<i64> {
        let total: Option<i64> = sqlx::query_scalar!(
            r#"
            SELECT SUM(amount_cents) as "total: i64"
            FROM payments
            WHERE sale_id = ?1
            "#,
            sale_id
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(total.unwrap_or(0))
    }
}

/// Generates a receipt number in format: YYYYMMDD-DD-NNNN
///
/// ## Format
/// - YYYYMMDD: Date
/// - DD: Device code (last 2 chars of device_id)
/// - NNNN: Sequential number (padded to 4 digits)
///
/// ## Example
/// `20260131-01-0001`
fn generate_receipt_number(device_id: &str) -> String {
    let now = Utc::now();
    let date_part = now.format("%Y%m%d");

    // Extract last 2 characters of device ID, or use "00"
    let device_code: String = device_id
        .chars()
        .rev()
        .take(2)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let device_code = if device_code.len() < 2 {
        "00".to_string()
    } else {
        device_code
    };

    // For now, use timestamp milliseconds as sequence
    // TODO: In production, this should be a proper daily counter
    let seq = (now.timestamp_millis() % 10000) as u32;

    format!("{}-{}-{:04}", date_part, device_code, seq)
}

/// Generates a new sale item ID.
pub fn generate_sale_item_id() -> String {
    Uuid::new_v4().to_string()
}

/// Generates a new payment ID.
pub fn generate_payment_id() -> String {
    Uuid::new_v4().to_string()
}
