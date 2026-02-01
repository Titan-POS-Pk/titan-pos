//! # Product Commands
//!
//! Tauri commands for product search and retrieval.
//!
//! ## Search Flow
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Product Search Flow                                  │
//! │                                                                         │
//! │  User types "12345678901"                                              │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  Frontend debounces (150ms) + detects barcode pattern                  │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  invoke('search_products', { query: '12345678901' })                   │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  ┌───────────────────────────────────────────┐                         │
//! │  │  Is query a barcode? (8-13 digits)        │                         │
//! │  │  YES: Try exact barcode lookup first      │──► Found? Return [1]    │
//! │  │  NO:  Use FTS5 full-text search           │                         │
//! │  └───────────────────────────────────────────┘                         │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  FTS5 query with wildcard: "query*"                                    │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  Return Vec<ProductDto> to frontend                                    │
//! │                                                                         │
//! │  Performance Target: <10ms for 50,000 products                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use serde::{Deserialize, Serialize};
use std::time::Instant;
use tauri::State;
use tracing::{debug, info};

use crate::error::ApiError;
use crate::state::DbState;
use titan_core::Product;
use titan_db::Database;

/// Product DTO (Data Transfer Object) for frontend.
///
/// ## Why DTO?
/// - Decouples internal domain model from API contract
/// - Allows selective field exposure
/// - Handles serde rename to camelCase for JS consumption
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
    /// Whether selling is allowed when stock is 0 or negative.
    /// Used by frontend to show "Back-order" vs "Out of Stock".
    pub allow_negative_stock: bool,
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
            allow_negative_stock: p.allow_negative_stock,
            current_stock: p.current_stock,
            is_active: p.is_active,
        }
    }
}

/// Checks if a query looks like a barcode (8-13 numeric digits).
///
/// ## Barcode Formats Detected
/// - EAN-8: 8 digits
/// - UPC-A: 12 digits  
/// - EAN-13: 13 digits
///
/// ## Why This Matters
/// Barcode scanners "type" very fast (full barcode in <50ms).
/// If we detect a barcode pattern, we do an exact lookup first
/// for instant response, skipping the FTS5 search entirely.
fn is_barcode_query(query: &str) -> bool {
    let len = query.len();
    (8..=13).contains(&len) && query.chars().all(|c| c.is_ascii_digit())
}

/// Searches products using FTS5 full-text search.
///
/// ## User Workflow
/// ```text
/// ┌─────────────────────────────────────────────────────────────────┐
/// │  POS Screen                                                     │
/// │  ┌─────────────────────────────────────────────────────────┐   │
/// │  │ 🔍 Search: "coke"                                       │   │
/// │  └─────────────────────────────────────────────────────────┘   │
/// │           │                                                     │
/// │           ▼ (debounced 150ms, instant for barcodes)            │
/// │  ┌─────────────────────────────────────────────────────────┐   │
/// │  │ invoke('search_products', { query: 'coke' })            │   │
/// │  └─────────────────────────────────────────────────────────┘   │
/// │           │                                                     │
/// │           ▼                                                     │
/// │  THIS FUNCTION: Queries FTS5 index for matching products       │
/// │           │                                                     │
/// │           ▼                                                     │
/// │  Returns: Vec<ProductDto> displayed in product grid            │
/// └─────────────────────────────────────────────────────────────────┘
/// ```
///
/// ## Arguments
/// * `query` - Search term (searches SKU, name, barcode)
/// * `limit` - Maximum results to return (default: 20, max: 100)
///
/// ## Returns
/// Products matching the search, ordered by relevance.
///
/// ## Performance
/// - Target: <10ms for 50,000 products
/// - Uses FTS5 MATCH query, not LIKE (which would be slow)
/// - Barcode queries get instant exact lookup
#[tauri::command]
pub async fn search_products(
    db: State<'_, DbState>,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<ProductDto>, ApiError> {
    let start = Instant::now();
    let query = query.trim();
    let limit = limit.unwrap_or(20).min(100);

    debug!(query = %query, limit = %limit, "search_products command");

    let db_inner: &Database = (*db).inner();

    // Optimization: If query looks like a barcode, try exact lookup first
    // This gives instant response for barcode scanners
    if is_barcode_query(query) {
        debug!(barcode = %query, "Detected barcode pattern, trying exact lookup");
        if let Some(product) = db_inner.products().get_by_barcode(query).await? {
            let elapsed = start.elapsed();
            info!(
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                count = 1,
                "search_products barcode lookup"
            );
            return Ok(vec![ProductDto::from(product)]);
        }
        // Barcode not found, fall through to FTS search
        debug!("Barcode not found, falling back to FTS search");
    }

    // Full-text search
    let products = db_inner.products().search(query, limit).await?;
    let dtos: Vec<ProductDto> = products.into_iter().map(ProductDto::from).collect();

    let elapsed = start.elapsed();
    info!(
        elapsed_ms = elapsed.as_secs_f64() * 1000.0,
        count = dtos.len(),
        query = %query,
        "search_products FTS complete"
    );

    Ok(dtos)
}

/// Gets a single product by its UUID.
///
/// ## When To Use
/// - Fetching full product details for a modal/detail view
/// - Refreshing a specific product's data
///
/// ## Arguments
/// * `id` - Product UUID
///
/// ## Returns
/// The product if found, or ApiError::NotFound
#[tauri::command]
pub async fn get_product_by_id(db: State<'_, DbState>, id: String) -> Result<ProductDto, ApiError> {
    debug!(id = %id, "get_product_by_id command");
    let db_inner: &Database = (*db).inner();
    let product = db_inner
        .products()
        .get_by_id(&id)
        .await?
        .ok_or_else(|| ApiError::not_found("Product", &id))?;
    Ok(ProductDto::from(product))
}

/// Gets a single product by its SKU.
///
/// ## When To Use
/// - Manual SKU entry by cashier
/// - Lookup by business identifier
///
/// ## Arguments
/// * `sku` - Product SKU (e.g., "BEV-COC-001")
///
/// ## Returns
/// The product if found, or ApiError::NotFound
#[tauri::command]
pub async fn get_product_by_sku(
    db: State<'_, DbState>,
    sku: String,
) -> Result<ProductDto, ApiError> {
    debug!(sku = %sku, "get_product_by_sku command");
    let db_inner: &Database = (*db).inner();
    let product = db_inner
        .products()
        .get_by_sku(&sku)
        .await?
        .ok_or_else(|| ApiError::not_found("Product", &sku))?;
    Ok(ProductDto::from(product))
}
