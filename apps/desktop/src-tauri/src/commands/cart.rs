//! # Cart Commands
//!
//! Tauri commands for cart manipulation.
//!
//! ## Cart Lifecycle
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Cart Lifecycle                                       │
//! │                                                                         │
//! │  ┌──────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐       │
//! │  │  Empty   │────►│ In Cart  │────►│  Tender  │────►│ Finalized│       │
//! │  │  Cart    │     │          │     │  Modal   │     │   Sale   │       │
//! │  └──────────┘     └──────────┘     └──────────┘     └──────────┘       │
//! │                        │                 │                              │
//! │                   add_to_cart       finalize_sale                      │
//! │                   update_item       (sale.rs)                          │
//! │                   remove_item                                           │
//! │                        │                                                │
//! │                        ▼                                                │
//! │                   clear_cart ──────────────────────►                   │
//! │                                                      (back to empty)   │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::debug;

use crate::error::ApiError;
use crate::state::{Cart, CartItem, CartState, CartTotals, DbState};
use titan_db::Database;

/// Cart response including items and totals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartResponse {
    pub items: Vec<CartItem>,
    pub totals: CartTotals,
}

impl From<&Cart> for CartResponse {
    fn from(cart: &Cart) -> Self {
        CartResponse {
            items: cart.items.clone(),
            totals: CartTotals::from(cart),
        }
    }
}

/// Gets the current cart contents.
///
/// ## User Workflow
/// ```text
/// ┌─────────────────────────────────────────────────────────────────────────┐
/// │  Cart Display (always visible on POS screen)                           │
/// │                                                                         │
/// │  ┌────────────────────────────────────────────────────────────────┐    │
/// │  │  CART                                              3 items     │    │
/// │  ├────────────────────────────────────────────────────────────────┤    │
/// │  │  Coca-Cola 330ml         x2              $3.98               │    │
/// │  │  Chips Lays Classic      x1              $2.49               │    │
/// │  ├────────────────────────────────────────────────────────────────┤    │
/// │  │  Subtotal                                $6.47               │    │
/// │  │  Tax (8.25%)                             $0.53               │    │
/// │  │  ──────────────────────────────────────────────────          │    │
/// │  │  TOTAL                                   $7.00               │    │
/// │  └────────────────────────────────────────────────────────────────┘    │
/// │                                                                         │
/// │  invoke('get_cart') → { items: [...], totals: {...} }                  │
/// └─────────────────────────────────────────────────────────────────────────┘
/// ```
///
/// ## Returns
/// Current cart with items and calculated totals
#[tauri::command]
pub fn get_cart(cart: State<'_, CartState>) -> CartResponse {
    debug!("get_cart command");
    cart.with_cart(|c| CartResponse::from(c))
}

/// Adds a product to the cart.
///
/// ## Behavior
/// - If product already in cart: quantity increases
/// - If product not in cart: added as new item
/// - Price is "frozen" at time of adding (won't change if product price updates)
///
/// ## User Workflow
/// ```text
/// ┌─────────────────────────────────────────────────────────────────────────┐
/// │  User clicks on product in search results                              │
/// │                    │                                                    │
/// │                    ▼                                                    │
/// │  invoke('add_to_cart', { productId: 'xxx', quantity: 1 })              │
/// │                    │                                                    │
/// │                    ▼                                                    │
/// │  ┌────────────────────────────────────────────────────────────────┐    │
/// │  │  1. Fetch product from database (get current price)           │    │
/// │  │  2. Check if already in cart                                   │    │
/// │  │     - Yes: increase quantity                                   │    │
/// │  │     - No: add new item with frozen price                       │    │
/// │  │  3. Return updated cart                                        │    │
/// │  └────────────────────────────────────────────────────────────────┘    │
/// │                    │                                                    │
/// │                    ▼                                                    │
/// │  Cart display updates with new item                                    │
/// └─────────────────────────────────────────────────────────────────────────┘
/// ```
///
/// ## Arguments
/// * `product_id` - Product UUID to add
/// * `quantity` - Quantity to add (default: 1)
///
/// ## Returns
/// Updated cart with all items and totals
#[tauri::command]
pub async fn add_to_cart(
    db: State<'_, DbState>,
    cart: State<'_, CartState>,
    product_id: String,
    quantity: Option<i64>,
) -> Result<CartResponse, ApiError> {
    let quantity = quantity.unwrap_or(1);
    debug!(product_id = %product_id, quantity = %quantity, "add_to_cart command");

    // Explicit type annotation helps Rust resolve the method chain
    // db is State<DbState>, so we dereference to get &DbState first
    let db_inner: &Database = (*db).inner();
    let product = db_inner
        .products()
        .get_by_id(&product_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Product", &product_id))?;

    // Check if product is active
    if !product.is_active {
        return Err(ApiError::validation("Product is not available for sale"));
    }

    // Add to cart (thread-safe via Mutex)
    let result = cart.with_cart_mut(|c| {
        c.add_item(&product, quantity)?;
        Ok::<CartResponse, String>(CartResponse::from(&*c))
    });

    result.map_err(ApiError::cart)
}

/// Updates the quantity of an item in the cart.
///
/// ## Behavior
/// - Quantity 0: removes the item
/// - Quantity > max: returns error
///
/// ## Arguments
/// * `product_id` - Product UUID in cart
/// * `quantity` - New quantity (0 to remove)
///
/// ## Returns
/// Updated cart
#[tauri::command]
pub fn update_cart_item(
    cart: State<'_, CartState>,
    product_id: String,
    quantity: i64,
) -> Result<CartResponse, ApiError> {
    debug!(product_id = %product_id, quantity = %quantity, "update_cart_item command");

    let result = cart.with_cart_mut(|c| {
        c.update_quantity(&product_id, quantity)?;
        Ok::<CartResponse, String>(CartResponse::from(&*c))
    });

    result.map_err(ApiError::cart)
}

/// Removes an item from the cart.
///
/// ## Arguments
/// * `product_id` - Product UUID to remove
///
/// ## Returns
/// Updated cart
#[tauri::command]
pub fn remove_from_cart(
    cart: State<'_, CartState>,
    product_id: String,
) -> Result<CartResponse, ApiError> {
    debug!(product_id = %product_id, "remove_from_cart command");

    let result = cart.with_cart_mut(|c| {
        c.remove_item(&product_id)?;
        Ok::<CartResponse, String>(CartResponse::from(&*c))
    });

    result.map_err(ApiError::cart)
}

/// Clears all items from the cart.
///
/// ## When Used
/// - User cancels the sale
/// - After sale is finalized (new transaction)
///
/// ## Returns
/// Empty cart
#[tauri::command]
pub fn clear_cart(cart: State<'_, CartState>) -> CartResponse {
    debug!("clear_cart command");

    cart.with_cart_mut(|c| {
        c.clear();
        CartResponse::from(&*c)
    })
}
