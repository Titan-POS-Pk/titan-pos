/**
 * Main Application Component
 *
 * This is the root component that sets up the POS layout.
 *
 * ## Layout Structure
 * ```
 * ┌─────────────────────────────────────────────────────────────────────────┐
 * │  Header (store name, clock, settings)                        64px      │
 * ├─────────────────────────────────────────────┬───────────────────────────┤
 * │                                             │                           │
 * │  Product Search & Grid                      │  Cart Sidebar             │
 * │  (main content area)                        │  - Items list             │
 * │                                             │  - Totals                 │
 * │  - Search bar                               │  - Checkout button        │
 * │  - Category filters                         │                           │
 * │  - Product cards                            │                           │
 * │                                             │                           │
 * │                                             │       320px width         │
 * └─────────────────────────────────────────────┴───────────────────────────┘
 * ```
 *
 * ## State Management
 * Using SolidJS signals for reactive state:
 * - `searchQuery`: Current search input
 * - `products`: Search results from Tauri backend
 * - `cart`: Current cart state (synced with Rust backend)
 */

import { Component, createSignal, onMount, Show } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';

// Components
import Header from './components/Header';
import ProductSearch from './components/ProductSearch';
import Cart from './components/Cart';
import TenderModal from './components/TenderModal';

// Types
import type { CartResponse, ConfigState } from './types';

/**
 * Root Application Component
 *
 * Manages global state and provides the main layout structure.
 */
const App: Component = () => {
  // ─────────────────────────────────────────────────────────────────────────
  // State
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * Application configuration loaded from Rust backend.
   * Contains store name, currency settings, tax rates, etc.
   */
  const [config, setConfig] = createSignal<ConfigState | null>(null);

  /**
   * Current cart state, synced with Rust backend.
   * Updated after every cart operation (add, update, remove).
   */
  const [cart, setCart] = createSignal<CartResponse>({
    items: [],
    totals: {
      itemCount: 0,
      totalQuantity: 0,
      subtotalCents: 0,
      taxCents: 0,
      totalCents: 0,
    },
  });

  /**
   * Controls visibility of the tender (payment) modal.
   * Opens when user clicks "Checkout" button.
   */
  const [showTender, setShowTender] = createSignal(false);

  /**
   * Current sale ID when in checkout flow.
   * Set after create_sale, used for add_payment and finalize_sale.
   */
  const [currentSaleId, setCurrentSaleId] = createSignal<string | null>(null);

  /**
   * Loading state for async operations.
   */
  const [loading, setLoading] = createSignal(true);

  /**
   * Error message to display (if any).
   */
  const [error, setError] = createSignal<string | null>(null);

  // ─────────────────────────────────────────────────────────────────────────
  // Initialization
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * Load initial data on mount.
   *
   * ## Startup Sequence
   * 1. Fetch configuration from backend
   * 2. Fetch current cart state (in case of app restart mid-transaction)
   * 3. Set loading to false
   */
  onMount(async () => {
    try {
      // Load configuration
      const configData = await invoke<ConfigState>('get_config');
      setConfig(configData);

      // Load cart state
      const cartData = await invoke<CartResponse>('get_cart');
      setCart(cartData);

      setLoading(false);
    } catch (err) {
      console.error('Failed to initialize app:', err);
      setError(`Failed to initialize: ${err}`);
      setLoading(false);
    }
  });

  // ─────────────────────────────────────────────────────────────────────────
  // Cart Operations
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * Refreshes cart state from the backend.
   * Called after any cart operation to ensure UI is in sync.
   */
  const refreshCart = async () => {
    try {
      const cartData = await invoke<CartResponse>('get_cart');
      setCart(cartData);
    } catch (err) {
      console.error('Failed to refresh cart:', err);
    }
  };

  /**
   * Adds a product to the cart.
   *
   * @param productId - UUID of the product to add
   * @param quantity - Quantity to add (default: 1)
   */
  const addToCart = async (productId: string, quantity = 1) => {
    try {
      const cartData = await invoke<CartResponse>('add_to_cart', {
        productId,
        quantity,
      });
      setCart(cartData);
    } catch (err) {
      console.error('Failed to add to cart:', err);
      // TODO: Show toast notification
    }
  };

  /**
   * Updates the quantity of a cart item.
   *
   * @param productId - UUID of the product
   * @param quantity - New quantity (0 to remove)
   */
  const updateCartItem = async (productId: string, quantity: number) => {
    try {
      const cartData = await invoke<CartResponse>('update_cart_item', {
        productId,
        quantity,
      });
      setCart(cartData);
    } catch (err) {
      console.error('Failed to update cart:', err);
    }
  };

  /**
   * Removes an item from the cart.
   *
   * @param productId - UUID of the product to remove
   */
  const removeFromCart = async (productId: string) => {
    try {
      const cartData = await invoke<CartResponse>('remove_from_cart', {
        productId,
      });
      setCart(cartData);
    } catch (err) {
      console.error('Failed to remove from cart:', err);
    }
  };

  /**
   * Clears all items from the cart.
   */
  const clearCart = async () => {
    try {
      const cartData = await invoke<CartResponse>('clear_cart');
      setCart(cartData);
    } catch (err) {
      console.error('Failed to clear cart:', err);
    }
  };

  // ─────────────────────────────────────────────────────────────────────────
  // Checkout Flow
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * Initiates the checkout process.
   *
   * ## Flow
   * 1. Create sale from current cart
   * 2. Store sale ID
   * 3. Open tender modal
   */
  const startCheckout = async () => {
    try {
      const response = await invoke<{ saleId: string; totalCents: number }>('create_sale');
      setCurrentSaleId(response.saleId);
      setShowTender(true);
    } catch (err) {
      console.error('Failed to create sale:', err);
      // TODO: Show error toast
    }
  };

  /**
   * Handles successful sale completion.
   * Called from TenderModal after finalize_sale succeeds.
   */
  const handleSaleComplete = () => {
    setShowTender(false);
    setCurrentSaleId(null);
    // Cart is already cleared by finalize_sale
    refreshCart();
  };

  /**
   * Handles checkout cancellation.
   * Called when user closes tender modal without completing payment.
   */
  const handleCheckoutCancel = () => {
    setShowTender(false);
    setCurrentSaleId(null);
    // TODO: Should we void the pending sale?
  };

  // ─────────────────────────────────────────────────────────────────────────
  // Render
  // ─────────────────────────────────────────────────────────────────────────

  return (
    <div class="h-screen flex flex-col bg-gray-100">
      {/* Loading State */}
      <Show when={loading()}>
        <div class="flex items-center justify-center h-screen">
          <div class="text-xl text-gray-600">Loading Titan POS...</div>
        </div>
      </Show>

      {/* Error State */}
      <Show when={error()}>
        <div class="flex items-center justify-center h-screen">
          <div class="text-xl text-red-600">{error()}</div>
        </div>
      </Show>

      {/* Main Application */}
      <Show when={!loading() && !error()}>
        {/* Header */}
        <Header storeName={config()?.storeName ?? 'Titan POS'} />

        {/* Main Content */}
        <div class="flex flex-1 overflow-hidden">
          {/* Product Search Area */}
          <main class="flex-1 overflow-auto p-4">
            <ProductSearch onAddToCart={addToCart} />
          </main>

          {/* Cart Sidebar */}
          <aside class="w-pos-sidebar bg-white border-l border-gray-200 flex flex-col">
            <Cart
              cart={cart()}
              config={config()}
              onUpdateItem={updateCartItem}
              onRemoveItem={removeFromCart}
              onClearCart={clearCart}
              onCheckout={startCheckout}
            />
          </aside>
        </div>

        {/* Tender Modal */}
        <Show when={showTender() && currentSaleId()}>
          <TenderModal
            saleId={currentSaleId()!}
            totalCents={cart().totals.totalCents}
            config={config()}
            onComplete={handleSaleComplete}
            onCancel={handleCheckoutCancel}
          />
        </Show>
      </Show>
    </div>
  );
};

export default App;
