//! # Sale Commands

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::{debug, info};
use uuid::Uuid;

use crate::error::{ApiError, ErrorCode};
use crate::state::{CartState, ConfigState, DbState};
use titan_core::{Payment, PaymentMethod, Sale, SaleItem, SaleStatus};
use titan_db::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSaleResponse {
    pub sale_id: String,
    pub total_cents: i64,
    pub item_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddPaymentResponse {
    pub payment_id: String,
    pub amount_cents: i64,
    pub total_paid_cents: i64,
    pub remaining_cents: i64,
    pub change_cents: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptResponse {
    pub sale_id: String,
    pub receipt_number: String,
    pub store_name: String,
    pub timestamp: String,
    pub items: Vec<ReceiptItem>,
    pub subtotal_cents: i64,
    pub tax_cents: i64,
    pub total_cents: i64,
    pub payments: Vec<ReceiptPayment>,
    pub change_cents: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptItem {
    pub name: String,
    pub quantity: i64,
    pub unit_price_cents: i64,
    pub line_total_cents: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptPayment {
    pub method: String,
    pub amount_cents: i64,
}

#[tauri::command]
pub async fn create_sale(
    db: State<'_, DbState>,
    cart: State<'_, CartState>,
    config: State<'_, ConfigState>,
) -> Result<CreateSaleResponse, ApiError> {
    debug!("create_sale command");

    let (items, subtotal, tax, total) = cart.with_cart(|c| {
        (
            c.items.clone(),
            c.subtotal_cents(),
            c.tax_cents(),
            c.total_cents(),
        )
    });

    if items.is_empty() {
        return Err(ApiError::validation("Cart is empty"));
    }

    let db_inner: &Database = (*db).inner();

    let sale_id = Uuid::new_v4().to_string();
    let receipt_number = generate_receipt_number();
    let now = Utc::now();

    let sale = Sale {
        id: sale_id.clone(),
        tenant_id: config.tenant_id.clone(),
        receipt_number: receipt_number.clone(),
        status: SaleStatus::Draft,
        subtotal_cents: subtotal,
        tax_cents: tax,
        discount_cents: 0,
        total_cents: total,
        user_id: "default".to_string(),
        device_id: "pos-01".to_string(),
        notes: None,
        created_at: now,
        updated_at: now,
        completed_at: None,
        sync_version: 0,
    };

    db_inner.sales().insert_sale(&sale).await?;

    for cart_item in &items {
        let sale_item = SaleItem {
            id: Uuid::new_v4().to_string(),
            sale_id: sale_id.clone(),
            product_id: cart_item.product_id.clone(),
            sku_snapshot: cart_item.sku.clone(),
            name_snapshot: cart_item.name.clone(),
            quantity: cart_item.quantity,
            unit_price_cents: cart_item.unit_price_cents,
            line_total_cents: cart_item.line_total_cents(),
            tax_cents: cart_item.tax_cents(),
            discount_cents: 0,
            created_at: now,
        };
        db_inner.sales().add_item(&sale_item).await?;
    }

    info!(sale_id = %sale_id, total = %total, items = items.len(), "Sale created");

    Ok(CreateSaleResponse {
        sale_id,
        total_cents: total,
        item_count: items.len(),
    })
}

#[tauri::command]
pub async fn add_payment(
    db: State<'_, DbState>,
    sale_id: String,
    amount_cents: i64,
    method: String,
) -> Result<AddPaymentResponse, ApiError> {
    debug!(sale_id = %sale_id, amount = %amount_cents, method = %method, "add_payment command");

    if amount_cents <= 0 {
        return Err(ApiError::validation("Payment amount must be positive"));
    }

    let payment_method = match method.to_lowercase().as_str() {
        "cash" => PaymentMethod::Cash,
        "card" | "credit" | "debit" => PaymentMethod::ExternalCard,
        _ => PaymentMethod::ExternalCard,
    };

    let db_inner: &Database = (*db).inner();

    let sale = db_inner
        .sales()
        .get_by_id(&sale_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Sale", &sale_id))?;

    if sale.status != SaleStatus::Draft {
        return Err(ApiError::new(
            ErrorCode::BusinessLogic,
            format!("Sale is {:?}, cannot add payment", sale.status),
        ));
    }

    let payment_id = Uuid::new_v4().to_string();
    let payment = Payment {
        id: payment_id.clone(),
        sale_id: sale_id.clone(),
        method: payment_method,
        amount_cents,
        tendered_cents: Some(amount_cents),
        change_cents: None,
        reference: None,
        created_at: Utc::now(),
    };

    db_inner.sales().add_payment(&payment).await?;

    let total_paid = db_inner.sales().get_total_paid(&sale_id).await?;
    let remaining = (sale.total_cents - total_paid).max(0);
    let change = (total_paid - sale.total_cents).max(0);

    info!(sale_id = %sale_id, payment_id = %payment_id, amount = %amount_cents, total_paid = %total_paid, remaining = %remaining, "Payment added");

    Ok(AddPaymentResponse {
        payment_id,
        amount_cents,
        total_paid_cents: total_paid,
        remaining_cents: remaining,
        change_cents: change,
    })
}

#[tauri::command]
pub async fn finalize_sale(
    db: State<'_, DbState>,
    cart: State<'_, CartState>,
    config: State<'_, ConfigState>,
    sale_id: String,
) -> Result<ReceiptResponse, ApiError> {
    debug!(sale_id = %sale_id, "finalize_sale command");

    let db_inner: &Database = (*db).inner();

    db_inner.sales().finalize_sale(&sale_id).await?;

    let sale = db_inner
        .sales()
        .get_by_id(&sale_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Sale", &sale_id))?;

    let payload = serde_json::to_string(&sale).unwrap_or_default();
    db_inner
        .sync_outbox()
        .queue_for_sync("SALE", &sale_id, &payload)
        .await?;

    let items = db_inner.sales().get_items(&sale_id).await?;
    let payments = db_inner.sales().get_payments(&sale_id).await?;

    cart.with_cart_mut(|c| c.clear());

    info!(sale_id = %sale_id, "Sale finalized");

    let total_change: i64 = payments.iter().filter_map(|p| p.change_cents).sum();

    let receipt = ReceiptResponse {
        sale_id: sale.id,
        receipt_number: sale.receipt_number,
        store_name: config.store_name.clone(),
        timestamp: sale.completed_at.unwrap_or(sale.created_at).to_rfc3339(),
        items: items
            .into_iter()
            .map(|i| ReceiptItem {
                name: i.name_snapshot,
                quantity: i.quantity,
                unit_price_cents: i.unit_price_cents,
                line_total_cents: i.line_total_cents,
            })
            .collect(),
        subtotal_cents: sale.subtotal_cents,
        tax_cents: sale.tax_cents,
        total_cents: sale.total_cents,
        payments: payments
            .into_iter()
            .map(|p| ReceiptPayment {
                method: format!("{:?}", p.method),
                amount_cents: p.amount_cents,
            })
            .collect(),
        change_cents: total_change,
    };

    Ok(receipt)
}

fn generate_receipt_number() -> String {
    let now = Utc::now();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let random: u16 = (nanos % 10000) as u16;
    format!("{}-{:04}", now.format("%y%m%d-%H%M%S"), random)
}
