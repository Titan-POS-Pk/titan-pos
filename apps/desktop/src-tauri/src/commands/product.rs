//! # Product Commands

use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::debug;

use crate::error::ApiError;
use crate::state::DbState;
use titan_core::Product;
use titan_db::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductDto {
    pub id: String,
    pub sku: String,
    pub barcode: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub price_cents: i64,
    pub tax_rate_bps: u32,
    pub track_inventory: bool,
    pub current_stock: Option<i64>,
    pub is_active: bool,
}

impl From<Product> for ProductDto {
    fn from(p: Product) -> Self {
        ProductDto {
            id: p.id,
            sku: p.sku,
            barcode: p.barcode,
            name: p.name,
            description: p.description,
            price_cents: p.price_cents,
            tax_rate_bps: p.tax_rate_bps,
            track_inventory: p.track_inventory,
            current_stock: p.current_stock,
            is_active: p.is_active,
        }
    }
}

#[tauri::command]
pub async fn search_products(
    db: State<'_, DbState>,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<ProductDto>, ApiError> {
    let query = query.trim();
    let limit = limit.unwrap_or(20).min(100);
    debug!(query = %query, limit = %limit, "search_products command");
    let db_inner: &Database = (*db).inner();
    let products = db_inner.products().search(query, limit).await?;
    let dtos: Vec<ProductDto> = products.into_iter().map(ProductDto::from).collect();
    debug!(count = dtos.len(), "search_products returning");
    Ok(dtos)
}

#[tauri::command]
pub async fn get_product_by_id(
    db: State<'_, DbState>,
    id: String,
) -> Result<ProductDto, ApiError> {
    debug!(id = %id, "get_product_by_id command");
    let db_inner: &Database = (*db).inner();
    let product = db_inner.products().get_by_id(&id).await?.ok_or_else(|| ApiError::not_found("Product", &id))?;
    Ok(ProductDto::from(product))
}

#[tauri::command]
pub async fn get_product_by_sku(
    db: State<'_, DbState>,
    sku: String,
) -> Result<ProductDto, ApiError> {
    debug!(sku = %sku, "get_product_by_sku command");
    let db_inner: &Database = (*db).inner();
    let product = db_inner.products().get_by_sku(&sku).await?.ok_or_else(|| ApiError::not_found("Product", &sku))?;
    Ok(ProductDto::from(product))
}
