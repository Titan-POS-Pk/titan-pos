/**
 * Main Application Component
 *
 * This is the root component that sets up the POS layout and manages the
 * transaction flow using a hybrid XState + SolidJS signals approach.
 *
 * ## Layout Structure
 * ```
 * ┌─────────────────────────────────────────────────────────────────────────┐
 * │  Header (store name, device ID, clock, settings)             64px      │
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
 * ┌─────────────────────────────────────────────────────────────────────────┐
 * │  DevConsole (toggleable, shows sync events and logs)       ~200px      │
 * └─────────────────────────────────────────────────────────────────────────┘
 * ```
 *
 * ## State Management (Hybrid Approach)
 *
 * ### XState (Transaction Flow)
 * The POS machine tracks high-level transaction states:
 * - idle → inCart → tender → receipt → idle
 *
 * ### SolidJS Signals (UI State)
 * Reactive signals handle transient UI state:
 * - Search query, loading states, cart data for display
 *
 * ## Event Listeners
 * The app listens for Tauri events:
 * - `sync:status` - Sync connection/status changes
 * - `sync:progress` - Sync progress updates
 * - `sync:error` - Sync errors
 * - `inventory:update` - Stock level changes (triggers product refresh)
 *
 * ## Keyboard Shortcuts
 * | Key        | Action                    | State Required |
 * |------------|---------------------------|----------------|
 * | F12        | Open Checkout / Confirm   | inCart/tender  |
 * | Escape     | Cancel / Clear Cart       | any            |
 * | Enter      | Confirm focused action    | any            |
 * | 1-9        | Quick add product         | idle/inCart    |
 * | Ctrl+`     | Toggle Dev Console        | any            |
 */

import { Component, createSignal, onMount, onCleanup, Show } from 'solid-js';
import { useMachine } from '@xstate/solid';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

// State Machine
import { posMachine } from './machines/posMachine';

// Components
import Header from './components/Header';
import ProductSearch from './components/ProductSearch';
import Cart from './components/Cart';
import TenderModal from './components/TenderModal';
import ReceiptModal from './components/ReceiptModal';
import { ToastProvider, useToast } from './components/Toast';
import DevConsole, { type LogEntry } from './components/DevConsole';

// Types
import type { 
  CartResponse, 
  ConfigState, 
  CreateSaleResponse, 
  ReceiptResponse,
  DeviceInfo,
  SyncStatus,
  SyncProgressEvent,
  SyncErrorEvent,
  InventoryUpdateEvent,
} from './types';

// ─────────────────────────────────────────────────────────────────────────────
// Log ID Generator
// ─────────────────────────────────────────────────────────────────────────────

let logIdCounter = 0;
const nextLogId = () => ++logIdCounter;

// ─────────────────────────────────────────────────────────────────────────────
// Inner App Component (uses toast context)
// ─────────────────────────────────────────────────────────────────────────────

const AppInner: Component = () => {
  // ─────────────────────────────────────────────────────────────────────────
  // XState Machine
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * POS state machine tracks the transaction flow.
   * - idle: Empty cart, waiting for first item
   * - inCart: Items in cart, can add more or checkout
   * - tender: Payment modal open, processing payment
   * - receipt: Sale complete, showing receipt
   */
  const [state, send] = useMachine(posMachine);

  // ─────────────────────────────────────────────────────────────────────────
  // SolidJS Signals (UI State)
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * Application configuration loaded from Rust backend.
   */
  const [config, setConfig] = createSignal<ConfigState | null>(null);

  /**
   * Device information (device ID, sync mode, etc.)
   */
  const [deviceInfo, setDeviceInfo] = createSignal<DeviceInfo | null>(null);

  /**
   * Current sync status.
   */
  const [syncStatus, setSyncStatus] = createSignal<SyncStatus | null>(null);

  /**
   * Dev console logs.
   */
  const [devLogs, setDevLogs] = createSignal<LogEntry[]>([]);

  /**
   * Dev console visibility.
   */
  const [devConsoleVisible, setDevConsoleVisible] = createSignal(false);

  /**
   * Current cart state, synced with Rust backend.
   * This is for display purposes - the XState machine tracks whether cart has items.
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
   * Loading state for initial app load.
   */
  const [loading, setLoading] = createSignal(true);

  /**
   * Error message for initial load failures.
   */
  const [error, setError] = createSignal<string | null>(null);

  /**
   * Refresh trigger for ProductSearch component.
   * Increment this to force product list to reload (after stock changes).
   */
  const [productRefreshTrigger, setProductRefreshTrigger] = createSignal(0);

  // Toast hook for notifications
  const toast = useToast();

  // ─────────────────────────────────────────────────────────────────────────
  // Dev Console Logging
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * Adds a log entry to the dev console.
   */
  const addLog = (level: LogEntry['level'], message: string, data?: unknown) => {
    const entry: LogEntry = {
      id: nextLogId(),
      timestamp: new Date(),
      level,
      message,
      data,
    };
    setDevLogs(prev => [...prev.slice(-499), entry]); // Keep last 500 logs
  };

  /**
   * Clears all dev console logs.
   */
  const clearLogs = () => {
    setDevLogs([]);
  };

  // ─────────────────────────────────────────────────────────────────────────
  // Event Listeners
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * Set up Tauri event listeners for sync and inventory updates.
   */
  const setupEventListeners = async (): Promise<UnlistenFn[]> => {
    const unlisteners: UnlistenFn[] = [];

    // Listen for sync status changes
    unlisteners.push(
      await listen<SyncStatus>('sync:status', (event) => {
        setSyncStatus(event.payload);
        addLog('SYNC', `Status: ${event.payload.connectionState}`, {
          mode: event.payload.syncMode,
          hub: event.payload.hubUrl,
        });
      })
    );

    // Listen for sync progress
    unlisteners.push(
      await listen<SyncProgressEvent>('sync:progress', (event) => {
        addLog('SYNC', `Progress: ${event.payload.synced} synced, ${event.payload.pending} pending`);
      })
    );

    // Listen for sync errors
    unlisteners.push(
      await listen<SyncErrorEvent>('sync:error', (event) => {
        addLog('ERROR', event.payload.message, { retryable: event.payload.retryable });
        if (!event.payload.retryable) {
          toast.error(`Sync Error: ${event.payload.message}`);
        }
      })
    );

    // Listen for inventory updates (stock changes)
    unlisteners.push(
      await listen<InventoryUpdateEvent>('inventory:update', (event) => {
        addLog('INVENTORY', event.payload.reason, {
          products: event.payload.productIds.length,
        });
        // Trigger product list refresh
        setProductRefreshTrigger(prev => prev + 1);
        toast.info(`Stock updated: ${event.payload.productIds.length} product(s)`);
      })
    );

    return unlisteners;
  };

  // ─────────────────────────────────────────────────────────────────────────
  // Initialization
  // ─────────────────────────────────────────────────────────────────────────

  let eventUnlisteners: UnlistenFn[] = [];

  onMount(async () => {
    try {
      // Set up event listeners first
      eventUnlisteners = await setupEventListeners();
      addLog('INFO', 'Event listeners initialized');

      // Load configuration
      const configData = await invoke<ConfigState>('get_config');
      setConfig(configData);
      addLog('INFO', `Store: ${configData.storeName}`);

      // Load device info
      try {
        const deviceData = await invoke<DeviceInfo>('get_device_info');
        setDeviceInfo(deviceData);
        addLog('INFO', `Device: ${deviceData.deviceId} (${deviceData.syncMode})`);
      } catch (deviceErr) {
        console.warn('Could not load device info:', deviceErr);
        addLog('DEBUG', 'Device info not available');
      }

      // Load initial sync status
      try {
        const statusData = await invoke<SyncStatus>('get_sync_status');
        setSyncStatus(statusData);
        addLog('SYNC', `Initial status: ${statusData.connectionState}`);
      } catch (syncErr) {
        console.warn('Could not load sync status:', syncErr);
        addLog('DEBUG', 'Sync status not available');
      }

      // Load cart state (in case of app restart mid-transaction)
      const cartData = await invoke<CartResponse>('get_cart');
      setCart(cartData);

      // Sync machine state with cart
      if (cartData.items.length > 0) {
        send({
          type: 'ADD_ITEM',
          itemCount: cartData.totals.itemCount,
        });
      }

      setLoading(false);
      addLog('INFO', 'Application ready');
    } catch (err) {
      console.error('Failed to initialize app:', err);
      setError(`Failed to initialize: ${err}`);
      addLog('ERROR', `Initialization failed: ${err}`);
      setLoading(false);
    }
  });

  // Cleanup event listeners
  onCleanup(() => {
    eventUnlisteners.forEach(unlisten => unlisten());
  });

  // ─────────────────────────────────────────────────────────────────────────
  // Keyboard Shortcuts
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * Global keyboard event handler.
   */
  const handleKeyDown = (e: KeyboardEvent) => {
    // Don't intercept if user is typing in an input
    const target = e.target as HTMLElement;
    if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') {
      // Allow Escape to blur input
      if (e.key === 'Escape') {
        target.blur();
      }
      return;
    }

    switch (e.key) {
      case 'F12':
        e.preventDefault();
        if (state.matches('inCart') && cart().items.length > 0) {
          startCheckout();
        }
        break;

      case 'Escape':
        e.preventDefault();
        if (state.matches('tender')) {
          handleCheckoutCancel();
        } else if (state.matches('inCart')) {
          clearCart();
        } else if (state.matches('receipt')) {
          handleNewSale();
        }
        break;

      case 'Enter':
        e.preventDefault();
        if (state.matches('inCart') && cart().items.length > 0) {
          startCheckout();
        } else if (state.matches('receipt')) {
          handleNewSale();
        }
        break;
    }
  };

  onMount(() => {
    window.addEventListener('keydown', handleKeyDown);
  });

  onCleanup(() => {
    window.removeEventListener('keydown', handleKeyDown);
  });

  // ─────────────────────────────────────────────────────────────────────────
  // Cart Operations
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * Refreshes cart state from the backend and syncs with XState.
   */
  const refreshCart = async () => {
    try {
      const cartData = await invoke<CartResponse>('get_cart');
      setCart(cartData);

      // Update XState with new cart info
      send({
        type: 'UPDATE_CART',
        itemCount: cartData.totals.itemCount,
        totalCents: cartData.totals.totalCents,
      });
    } catch (err) {
      console.error('Failed to refresh cart:', err);
      toast.error('Failed to refresh cart');
    }
  };

  /**
   * Adds a product to the cart.
   */
  const addToCart = async (productId: string, quantity = 1) => {
    try {
      const cartData = await invoke<CartResponse>('add_to_cart', {
        productId,
        quantity,
      });
      setCart(cartData);

      // Update XState - if this was first item, transitions to inCart
      send({
        type: 'ADD_ITEM',
        itemCount: cartData.totals.itemCount,
      });

      toast.success('Added to cart');
    } catch (err: unknown) {
      console.error('Failed to add to cart:', err);
      // Check for specific error types
      const errorObj = err as { code?: string; message?: string };
      if (errorObj?.code === 'INSUFFICIENT_STOCK') {
        toast.warning(errorObj.message || 'Insufficient stock');
      } else {
        toast.error(errorObj?.message || 'Failed to add to cart');
      }
    }
  };

  /**
   * Updates the quantity of a cart item.
   */
  const updateCartItem = async (productId: string, quantity: number) => {
    try {
      const cartData = await invoke<CartResponse>('update_cart_item', {
        productId,
        quantity,
      });
      setCart(cartData);

      // Update XState (may transition to idle if cart empty)
      send({
        type: 'UPDATE_CART',
        itemCount: cartData.totals.itemCount,
        totalCents: cartData.totals.totalCents,
      });
    } catch (err) {
      console.error('Failed to update cart:', err);
      toast.error('Failed to update cart');
    }
  };

  /**
   * Removes an item from the cart.
   */
  const removeFromCart = async (productId: string) => {
    try {
      const cartData = await invoke<CartResponse>('remove_from_cart', {
        productId,
      });
      setCart(cartData);

      // Update XState
      send({
        type: 'UPDATE_CART',
        itemCount: cartData.totals.itemCount,
        totalCents: cartData.totals.totalCents,
      });

      toast.info('Item removed');
    } catch (err) {
      console.error('Failed to remove from cart:', err);
      toast.error('Failed to remove item');
    }
  };

  /**
   * Clears all items from the cart.
   */
  const clearCart = async () => {
    try {
      const cartData = await invoke<CartResponse>('clear_cart');
      setCart(cartData);

      // Reset XState to idle
      send({ type: 'CLEAR' });

      toast.info('Cart cleared');
    } catch (err) {
      console.error('Failed to clear cart:', err);
      toast.error('Failed to clear cart');
    }
  };

  // ─────────────────────────────────────────────────────────────────────────
  // Checkout Flow
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * Initiates the checkout process.
   */
  const startCheckout = async () => {
    try {
      const response = await invoke<CreateSaleResponse>('create_sale');

      // Transition to tender state
      send({
        type: 'CHECKOUT',
        saleId: response.saleId,
        totalCents: response.totalCents,
      });
    } catch (err) {
      console.error('Failed to create sale:', err);
      toast.error('Failed to start checkout');
    }
  };

  /**
   * Handles successful sale completion.
   */
  const handleSaleComplete = (receipt: ReceiptResponse) => {
    // Transition to receipt state
    send({
      type: 'PAYMENT_COMPLETE',
      receipt,
    });

    // Refresh cart (should be empty now)
    refreshCart();

    // Refresh product list to show updated stock levels
    setProductRefreshTrigger(prev => prev + 1);

    toast.success('Sale completed!');
  };

  /**
   * Handles checkout cancellation.
   */
  const handleCheckoutCancel = () => {
    send({ type: 'CANCEL' });
    toast.info('Checkout cancelled');
  };

  /**
   * Starts a new sale after viewing receipt.
   */
  const handleNewSale = () => {
    send({ type: 'NEW_SALE' });
    refreshCart();
    
    // Refresh product list to ensure stock levels are current
    setProductRefreshTrigger(prev => prev + 1);
  };

  // ─────────────────────────────────────────────────────────────────────────
  // Render
  // ─────────────────────────────────────────────────────────────────────────

  return (
    <div class="h-screen flex flex-col bg-gray-100">
      {/* Loading State */}
      <Show when={loading()}>
        <div class="flex items-center justify-center h-screen">
          <div class="text-center">
            <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-primary-600 mx-auto mb-4"></div>
            <div class="text-xl text-gray-600">Loading Titan POS...</div>
          </div>
        </div>
      </Show>

      {/* Error State */}
      <Show when={error()}>
        <div class="flex items-center justify-center h-screen">
          <div class="text-center">
            <div class="text-red-500 text-6xl mb-4">⚠️</div>
            <div class="text-xl text-red-600">{error()}</div>
            <button
              onClick={() => window.location.reload()}
              class="btn btn-primary mt-4"
            >
              Retry
            </button>
          </div>
        </div>
      </Show>

      {/* Main Application */}
      <Show when={!loading() && !error()}>
        {/* Header */}
        <Header 
          storeName={config()?.storeName ?? 'Titan POS'} 
          deviceInfo={deviceInfo()}
          syncStatus={syncStatus()}
        />

        {/* Main Content - adjust padding-bottom when console is visible */}
        <div class={`flex flex-1 overflow-hidden ${devConsoleVisible() ? 'pb-64' : ''}`}>
          {/* Product Search Area */}
          <main class="flex-1 overflow-auto p-4">
            <ProductSearch 
              onAddToCart={addToCart} 
              refreshTrigger={productRefreshTrigger()}
            />
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

        {/* Dev Console (in dev mode) */}
        <Show when={(import.meta as unknown as { env: { DEV: boolean } }).env.DEV}>
          <DevConsole
            logs={devLogs()}
            onClear={clearLogs}
            isVisible={devConsoleVisible()}
            onToggle={() => setDevConsoleVisible(v => !v)}
          />
        </Show>

        {/* Tender Modal - shown when in 'tender' state */}
        <Show when={state.matches('tender') && state.context.saleId}>
          <TenderModal
            saleId={state.context.saleId!}
            totalCents={state.context.totalCents}
            config={config()}
            onComplete={handleSaleComplete}
            onCancel={handleCheckoutCancel}
          />
        </Show>

        {/* Receipt Modal - shown when in 'receipt' state */}
        <Show when={state.matches('receipt') && state.context.receipt}>
          <ReceiptModal
            receipt={state.context.receipt!}
            config={config()}
            onNewSale={handleNewSale}
          />
        </Show>
      </Show>

      {/* Keyboard Shortcuts Help */}
      <div class="fixed bottom-4 right-4 group print:hidden">
        <button class="bg-gray-200 hover:bg-gray-300 text-gray-600 w-8 h-8 rounded-full flex items-center justify-center text-sm font-bold">
          ?
        </button>
        <div class="absolute bottom-10 right-0 bg-gray-900 text-white text-xs rounded-lg p-3 opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none whitespace-nowrap">
          <div class="font-bold mb-2">Keyboard Shortcuts</div>
          <div class="space-y-1">
            <div><kbd class="bg-gray-700 px-1 rounded">F12</kbd> Checkout</div>
            <div><kbd class="bg-gray-700 px-1 rounded">Esc</kbd> Cancel / Clear</div>
            <div><kbd class="bg-gray-700 px-1 rounded">Enter</kbd> Confirm</div>
            <div><kbd class="bg-gray-700 px-1 rounded">1-9</kbd> Quick add</div>
            <div><kbd class="bg-gray-700 px-1 rounded">Ctrl+`</kbd> Dev Console</div>
          </div>
        </div>
      </div>
    </div>
  );
};

// ─────────────────────────────────────────────────────────────────────────────
// Root App Component (with Toast Provider)
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Root Application Component
 *
 * Wraps the app in ToastProvider for notification support.
 */
const App: Component = () => {
  return (
    <ToastProvider>
      <AppInner />
    </ToastProvider>
  );
};

export default App;
