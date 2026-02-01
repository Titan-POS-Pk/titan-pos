//! # Cart State
//!
//! Manages the current shopping cart state.
//!
//! ## Thread Safety
//! The cart is wrapped in `Arc<Mutex<T>>` because:
//! 1. Multiple commands may access/modify the cart
//! 2. Only one command should modify the cart at a time
//! 3. Tauri commands can run concurrently
//!
//! ## Cart Operations Flow
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Cart State Operations                                │
//! │                                                                         │
//! │  Frontend Action          Tauri Command           Cart State Change     │
//! │  ───────────────          ─────────────           ─────────────────     │
//! │                                                                         │
//! │  Click Product ──────────► add_to_cart() ───────► items.push(item)     │
//! │                                                                         │
//! │  Change Quantity ────────► update_cart_item() ──► items[i].qty = n     │
//! │                                                                         │
//! │  Click Remove ───────────► remove_from_cart() ──► items.remove(i)      │
//! │                                                                         │
//! │  Click Clear ────────────► clear_cart() ────────► items.clear()        │
//! │                                                                         │
//! │  View Cart ──────────────► get_cart() ──────────► (read only)          │
//! │                                                                         │
//! │  NOTE: All write operations acquire the Mutex lock exclusively.         │
//! │        Read operations also acquire the lock but release it quickly.    │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use titan_core::{Money, Product, TaxRate};

/// An item in the shopping cart.
///
/// ## Design Notes
/// - `product_id`: Reference to the product (for database lookup)
/// - `product_snapshot`: Frozen copy of product data at time of adding
///   This ensures the cart displays consistent data even if the product
///   is updated in the database after being added to cart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartItem {
    /// Product ID (UUID)
    pub product_id: String,

    /// SKU at time of adding (frozen)
    pub sku: String,

    /// Product name at time of adding (frozen)
    pub name: String,

    /// Price in cents at time of adding (frozen)
    /// This is critical: we lock in the price when added to cart
    pub unit_price_cents: i64,

    /// Tax rate in basis points at time of adding (frozen)
    pub tax_rate_bps: u32,

    /// Quantity in cart
    pub quantity: i64,

    /// When this item was added to cart
    pub added_at: DateTime<Utc>,
}

impl CartItem {
    /// Creates a new cart item from a product and quantity.
    ///
    /// ## Price Freezing
    /// The price is captured at this moment. If the product price
    /// changes in the database, this cart item retains the original price.
    pub fn from_product(product: &Product, quantity: i64) -> Self {
        CartItem {
            product_id: product.id.clone(),
            sku: product.sku.clone(),
            name: product.name.clone(),
            unit_price_cents: product.price_cents,
            tax_rate_bps: product.tax_rate_bps,
            quantity,
            added_at: Utc::now(),
        }
    }

    /// Calculates the line total (unit price × quantity).
    pub fn line_total_cents(&self) -> i64 {
        self.unit_price_cents * self.quantity
    }

    /// Calculates the tax amount for this line item.
    ///
    /// Uses Bankers Rounding (round half to even) for financial accuracy.
    pub fn tax_cents(&self) -> i64 {
        let line_total = Money::from_cents(self.line_total_cents());
        line_total
            .calculate_tax(TaxRate::from_bps(self.tax_rate_bps))
            .cents()
    }

    /// Calculates line total including tax.
    pub fn line_total_with_tax_cents(&self) -> i64 {
        self.line_total_cents() + self.tax_cents()
    }
}

/// The shopping cart.
///
/// ## Invariants
/// - Items are unique by `product_id` (adding same product increases quantity)
/// - Quantity must be > 0 (removing sets qty to 0 removes the item)
/// - Maximum items: 100 (configured in titan-core)
/// - Maximum quantity per item: 999 (configured in titan-core)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Cart {
    /// Items in the cart
    pub items: Vec<CartItem>,

    /// When the cart was created/last cleared
    pub created_at: DateTime<Utc>,
}

impl Cart {
    /// Creates a new empty cart.
    pub fn new() -> Self {
        Cart {
            items: Vec::new(),
            created_at: Utc::now(),
        }
    }

    /// Adds a product to the cart or increases quantity if already present.
    ///
    /// ## Behavior
    /// - If product already in cart: increases quantity
    /// - If product not in cart: adds new item
    ///
    /// ## Returns
    /// - `Ok(())` on success
    /// - `Err(String)` if quantity would exceed maximum
    pub fn add_item(&mut self, product: &Product, quantity: i64) -> Result<(), String> {
        // Check if product already in cart
        if let Some(item) = self.items.iter_mut().find(|i| i.product_id == product.id) {
            let new_qty = item.quantity + quantity;
            if new_qty > titan_core::MAX_ITEM_QUANTITY {
                return Err(format!(
                    "Quantity would exceed maximum of {}",
                    titan_core::MAX_ITEM_QUANTITY
                ));
            }
            item.quantity = new_qty;
            return Ok(());
        }

        // Check max items
        if self.items.len() >= titan_core::MAX_CART_ITEMS {
            return Err(format!(
                "Cart cannot have more than {} items",
                titan_core::MAX_CART_ITEMS
            ));
        }

        // Add new item
        self.items.push(CartItem::from_product(product, quantity));
        Ok(())
    }

    /// Updates the quantity of an item in the cart.
    ///
    /// ## Behavior
    /// - If quantity is 0: removes the item
    /// - If product not found: returns error
    pub fn update_quantity(&mut self, product_id: &str, quantity: i64) -> Result<(), String> {
        if quantity == 0 {
            return self.remove_item(product_id);
        }

        if quantity > titan_core::MAX_ITEM_QUANTITY {
            return Err(format!(
                "Quantity cannot exceed {}",
                titan_core::MAX_ITEM_QUANTITY
            ));
        }

        if let Some(item) = self.items.iter_mut().find(|i| i.product_id == product_id) {
            item.quantity = quantity;
            Ok(())
        } else {
            Err(format!("Product {} not in cart", product_id))
        }
    }

    /// Removes an item from the cart by product ID.
    pub fn remove_item(&mut self, product_id: &str) -> Result<(), String> {
        let initial_len = self.items.len();
        self.items.retain(|i| i.product_id != product_id);

        if self.items.len() == initial_len {
            Err(format!("Product {} not in cart", product_id))
        } else {
            Ok(())
        }
    }

    /// Clears all items from the cart.
    pub fn clear(&mut self) {
        self.items.clear();
        self.created_at = Utc::now();
    }

    /// Returns the number of unique items in the cart.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Returns the total quantity of all items.
    pub fn total_quantity(&self) -> i64 {
        self.items.iter().map(|i| i.quantity).sum()
    }

    /// Calculates the subtotal (before tax).
    pub fn subtotal_cents(&self) -> i64 {
        self.items.iter().map(|i| i.line_total_cents()).sum()
    }

    /// Calculates the total tax.
    pub fn tax_cents(&self) -> i64 {
        self.items.iter().map(|i| i.tax_cents()).sum()
    }

    /// Calculates the grand total (subtotal + tax).
    pub fn total_cents(&self) -> i64 {
        self.subtotal_cents() + self.tax_cents()
    }

    /// Checks if the cart is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Cart totals summary for API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartTotals {
    pub item_count: usize,
    pub total_quantity: i64,
    pub subtotal_cents: i64,
    pub tax_cents: i64,
    pub total_cents: i64,
}

impl From<&Cart> for CartTotals {
    fn from(cart: &Cart) -> Self {
        CartTotals {
            item_count: cart.item_count(),
            total_quantity: cart.total_quantity(),
            subtotal_cents: cart.subtotal_cents(),
            tax_cents: cart.tax_cents(),
            total_cents: cart.total_cents(),
        }
    }
}

/// Tauri-managed cart state.
///
/// ## Thread Safety
/// Uses `Arc<Mutex<Cart>>` because:
/// - `Arc`: Allows shared ownership across threads
/// - `Mutex`: Ensures only one thread modifies the cart at a time
///
/// ## Why Not RwLock?
/// Cart operations are typically quick, and most operations modify state.
/// A RwLock would add complexity with minimal benefit.
#[derive(Debug)]
pub struct CartState {
    cart: Arc<Mutex<Cart>>,
}

impl CartState {
    /// Creates a new empty cart state.
    pub fn new() -> Self {
        CartState {
            cart: Arc::new(Mutex::new(Cart::new())),
        }
    }

    /// Executes a function with read access to the cart.
    ///
    /// ## Usage
    /// ```rust,ignore
    /// let totals = cart_state.with_cart(|cart| CartTotals::from(cart));
    /// ```
    pub fn with_cart<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Cart) -> R,
    {
        let cart = self.cart.lock().expect("Cart mutex poisoned");
        f(&cart)
    }

    /// Executes a function with write access to the cart.
    ///
    /// ## Usage
    /// ```rust,ignore
    /// cart_state.with_cart_mut(|cart| cart.add_item(&product, 1))?;
    /// ```
    pub fn with_cart_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Cart) -> R,
    {
        let mut cart = self.cart.lock().expect("Cart mutex poisoned");
        f(&mut cart)
    }
}

impl Default for CartState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use titan_core::DEFAULT_TENANT_ID;

    fn test_product(id: &str, price_cents: i64) -> Product {
        Product {
            id: id.to_string(),
            tenant_id: DEFAULT_TENANT_ID.to_string(),
            sku: format!("SKU-{}", id),
            barcode: None,
            name: format!("Product {}", id),
            description: None,
            price_cents,
            cost_cents: None,
            tax_rate_bps: 825, // 8.25%
            track_inventory: false,
            allow_negative_stock: false,
            current_stock: None,
            is_active: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            sync_version: 0,
        }
    }

    #[test]
    fn test_cart_add_item() {
        let mut cart = Cart::new();
        let product = test_product("1", 999); // $9.99

        cart.add_item(&product, 2).unwrap();

        assert_eq!(cart.item_count(), 1);
        assert_eq!(cart.total_quantity(), 2);
        assert_eq!(cart.subtotal_cents(), 1998); // $19.98
    }

    #[test]
    fn test_cart_add_same_product_increases_quantity() {
        let mut cart = Cart::new();
        let product = test_product("1", 999);

        cart.add_item(&product, 2).unwrap();
        cart.add_item(&product, 3).unwrap();

        assert_eq!(cart.item_count(), 1); // Still one unique item
        assert_eq!(cart.total_quantity(), 5);
    }

    #[test]
    fn test_cart_tax_calculation() {
        let mut cart = Cart::new();
        let product = test_product("1", 1000); // $10.00, 8.25% tax

        cart.add_item(&product, 1).unwrap();

        // Tax: $10.00 × 8.25% = $0.825 → $0.83 (standard rounding with +5000)
        assert_eq!(cart.tax_cents(), 83);
        assert_eq!(cart.total_cents(), 1083); // $10.83
    }

    #[test]
    fn test_cart_clear() {
        let mut cart = Cart::new();
        let product = test_product("1", 999);

        cart.add_item(&product, 2).unwrap();
        assert!(!cart.is_empty());

        cart.clear();
        assert!(cart.is_empty());
    }
}
