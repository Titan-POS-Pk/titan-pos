/**
 * ProductSearch Component
 *
 * Search bar and product grid for the main POS screen.
 *
 * ## User Flow
 * ```
 * ┌─────────────────────────────────────────────────────────────────────────┐
 * │  ┌─────────────────────────────────────────────────────────────────┐   │
 * │  │ 🔍 Search products by name, SKU, or barcode...                  │   │
 * │  └─────────────────────────────────────────────────────────────────┘   │
 * │                                                                         │
 * │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐          │
 * │  │ Product │ │ Product │ │ Product │ │ Product │ │ Product │          │
 * │  │  Card   │ │  Card   │ │  Card   │ │  Card   │ │  Card   │          │
 * │  │         │ │         │ │         │ │         │ │         │          │
 * │  │  $1.99  │ │  $2.49  │ │  $3.99  │ │  $1.49  │ │  $4.99  │          │
 * │  └─────────┘ └─────────┘ └─────────┘ └─────────┘ └─────────┘          │
 * │                                                                         │
 * │  ┌─────────┐ ┌─────────┐ ┌─────────┐                                   │
 * │  │ Product │ │ Product │ │ Product │                                   │
 * │  │  Card   │ │  Card   │ │  Card   │                                   │
 * │  └─────────┘ └─────────┘ └─────────┘                                   │
 * └─────────────────────────────────────────────────────────────────────────┘
 * ```
 *
 * ## Keyboard Navigation
 * - Arrow keys: Navigate between products
 * - Enter: Add selected product to cart
 * - Escape: Return focus to search input
 * - Numbers 1-9: Quick-add first 9 products
 *
 * ## Performance
 * - Search is debounced (150ms) to avoid excessive API calls
 * - Barcode input triggers instant search (no debounce)
 * - FTS5 search on backend is <10ms for 50k products
 */

import { Component, createSignal, For, Show, onMount, onCleanup } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import type { ProductDto } from '../types';
import { formatMoney, debounce } from '../utils';

interface ProductSearchProps {
  /** Called when user clicks a product to add to cart */
  onAddToCart: (productId: string, quantity?: number) => void;
  /** Trigger to force refresh of product list (increment to refresh) */
  refreshTrigger?: number;
}

/**
 * Number of columns in the product grid (used for keyboard navigation).
 * This should match the grid-cols-* classes in the CSS.
 */
const GRID_COLUMNS = 5;

/**
 * Checks if a string looks like a barcode (8-13 numeric digits).
 * Used to trigger instant search for barcode scanner input.
 */
function isBarcodeInput(value: string): boolean {
  const len = value.length;
  return len >= 8 && len <= 13 && /^\d+$/.test(value);
}

const ProductSearch: Component<ProductSearchProps> = (props) => {
  // ─────────────────────────────────────────────────────────────────────────
  // Refs
  // ─────────────────────────────────────────────────────────────────────────
  
  let searchInputRef: HTMLInputElement | undefined;

  // ─────────────────────────────────────────────────────────────────────────
  // State
  // ─────────────────────────────────────────────────────────────────────────

  const [searchQuery, setSearchQuery] = createSignal('');
  const [products, setProducts] = createSignal<ProductDto[]>([]);
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  
  /** Currently selected product index for keyboard navigation (-1 = none) */
  const [selectedIndex, setSelectedIndex] = createSignal(-1);

  // ─────────────────────────────────────────────────────────────────────────
  // Search Logic
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * Performs the actual search query to the backend.
   */
  const executeSearch = async (query: string) => {
    setLoading(true);
    setError(null);

    try {
      const results = await invoke<ProductDto[]>('search_products', {
        query,
        limit: 50,
      });
      setProducts(results);
      // Reset selection when results change
      setSelectedIndex(-1);
    } catch (err) {
      console.error('Search failed:', err);
      setError('Search failed. Please try again.');
      setProducts([]);
    } finally {
      setLoading(false);
    }
  };

  /**
   * Debounced search for regular typing.
   */
  const debouncedSearch = debounce((query: unknown) => executeSearch(query as string), 150);

  /**
   * Handles search input with smart debouncing:
   * - Barcode input: Instant search (no debounce)
   * - Regular text: 150ms debounce
   */
  const handleSearchInput = (e: Event) => {
    const input = e.target as HTMLInputElement;
    const value = input.value;
    setSearchQuery(value);

    // Barcode scanner input: trigger immediate search
    if (isBarcodeInput(value)) {
      executeSearch(value);
    } else {
      debouncedSearch(value);
    }
  };

  /**
   * Handles Enter key in search input for instant search.
   */
  const handleSearchKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      // If a product is selected, add it
      if (selectedIndex() >= 0 && selectedIndex() < products().length) {
        const product = products()[selectedIndex()];
        if (canAddToCart(product)) {
          props.onAddToCart(product.id, 1);
        }
      } else {
        // Otherwise, execute immediate search
        executeSearch(searchQuery());
      }
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      navigateProducts('down');
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      navigateProducts('up');
    } else if (e.key === 'ArrowRight') {
      e.preventDefault();
      navigateProducts('right');
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      navigateProducts('left');
    } else if (e.key === 'Escape') {
      setSelectedIndex(-1);
      searchInputRef?.focus();
    }
  };

  // Load initial products on mount
  onMount(() => {
    executeSearch('');
  });

  // Refresh products when refreshTrigger changes
  const prevRefreshTrigger = createSignal(props.refreshTrigger ?? 0);
  
  // Watch for refreshTrigger changes and re-run search
  const checkRefreshTrigger = () => {
    const current = props.refreshTrigger ?? 0;
    const [prev, setPrev] = prevRefreshTrigger;
    if (current !== prev()) {
      setPrev(current);
      // Re-run the current search query
      executeSearch(searchQuery());
    }
  };
  
  // Set up an interval to check for trigger changes
  const refreshInterval = setInterval(checkRefreshTrigger, 100);
  
  onCleanup(() => {
    clearInterval(refreshInterval);
  });

  // ─────────────────────────────────────────────────────────────────────────
  // Keyboard Navigation
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * Navigates through the product grid using arrow keys.
   * Uses a 5-column grid layout for left/right navigation.
   */
  const navigateProducts = (direction: 'up' | 'down' | 'left' | 'right') => {
    const totalProducts = products().length;
    if (totalProducts === 0) return;

    let newIndex = selectedIndex();

    switch (direction) {
      case 'down':
        newIndex = newIndex < 0 ? 0 : Math.min(newIndex + GRID_COLUMNS, totalProducts - 1);
        break;
      case 'up':
        newIndex = newIndex < 0 ? 0 : Math.max(newIndex - GRID_COLUMNS, 0);
        break;
      case 'right':
        newIndex = newIndex < 0 ? 0 : Math.min(newIndex + 1, totalProducts - 1);
        break;
      case 'left':
        newIndex = newIndex < 0 ? 0 : Math.max(newIndex - 1, 0);
        break;
    }

    setSelectedIndex(newIndex);
  };

  /**
   * Global keyboard shortcuts (1-9 for quick add).
   */
  const handleGlobalKeyDown = (e: KeyboardEvent) => {
    // Quick-add with number keys (1-9)
    if (e.key >= '1' && e.key <= '9' && !e.ctrlKey && !e.altKey && !e.metaKey) {
      // Only if not typing in search
      if (document.activeElement !== searchInputRef) {
        const index = parseInt(e.key) - 1;
        const productList = products();
        if (index < productList.length) {
          const product = productList[index];
          if (canAddToCart(product)) {
            props.onAddToCart(product.id, 1);
          }
        }
      }
    }
  };

  onMount(() => {
    window.addEventListener('keydown', handleGlobalKeyDown);
  });

  onCleanup(() => {
    window.removeEventListener('keydown', handleGlobalKeyDown);
  });

  // ─────────────────────────────────────────────────────────────────────────
  // Event Handlers
  // ─────────────────────────────────────────────────────────────────────────

  const handleProductClick = (product: ProductDto) => {
    if (canAddToCart(product)) {
      props.onAddToCart(product.id, 1);
    }
  };

  const handleClearSearch = () => {
    setSearchQuery('');
    executeSearch('');
    searchInputRef?.focus();
  };

  // ─────────────────────────────────────────────────────────────────────────
  // Helper Functions
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * Determines if a product can be added to cart based on stock rules.
   */
  const canAddToCart = (product: ProductDto): boolean => {
    if (!product.trackInventory) return true;
    if (product.allowNegativeStock) return true;
    return (product.currentStock ?? 0) > 0;
  };

  // ─────────────────────────────────────────────────────────────────────────
  // Render
  // ─────────────────────────────────────────────────────────────────────────

  return (
    <div class="flex flex-col h-full">
      {/* Search Bar */}
      <div class="mb-4 relative">
        <div class="absolute inset-y-0 left-0 pl-4 flex items-center pointer-events-none">
          <svg
            class="w-5 h-5 text-gray-400"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
            />
          </svg>
        </div>

        <input
          ref={searchInputRef}
          type="text"
          placeholder="Search products by name, SKU, or barcode..."
          value={searchQuery()}
          onInput={handleSearchInput}
          onKeyDown={handleSearchKeyDown}
          class="input input-lg pl-12 pr-12"
          autofocus
        />

        {/* Clear Button */}
        <Show when={searchQuery()}>
          <button
            onClick={handleClearSearch}
            class="absolute inset-y-0 right-0 pr-4 flex items-center"
            aria-label="Clear search"
          >
            <svg
              class="w-5 h-5 text-gray-400 hover:text-gray-600"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M6 18L18 6M6 6l12 12"
              />
            </svg>
          </button>
        </Show>
      </div>

      {/* Keyboard Hints */}
      <div class="mb-2 text-xs text-gray-400 flex gap-4">
        <span>↑↓←→ Navigate</span>
        <span>Enter Add to cart</span>
        <span>1-9 Quick add</span>
        <span>Esc Clear selection</span>
      </div>

      {/* Loading State */}
      <Show when={loading()}>
        <div class="flex items-center justify-center py-8">
          <div class="flex items-center gap-2 text-gray-500">
            <svg class="animate-spin h-5 w-5" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" fill="none" />
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
            </svg>
            <span>Searching...</span>
          </div>
        </div>
      </Show>

      {/* Error State */}
      <Show when={error()}>
        <div class="flex items-center justify-center py-8">
          <div class="text-danger-600 flex items-center gap-2">
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <span>{error()}</span>
          </div>
        </div>
      </Show>

      {/* Empty State */}
      <Show when={!loading() && !error() && products().length === 0}>
        <div class="flex flex-col items-center justify-center py-12 text-gray-500">
          <svg
            class="w-16 h-16 mb-4"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="1.5"
              d="M20 7l-8-4-8 4m16 0l-8 4m8-4v10l-8 4m0-10L4 7m8 4v10M4 7v10l8 4"
            />
          </svg>
          <p class="text-lg">
            {searchQuery() ? 'No products found' : 'Start typing to search products'}
          </p>
          <p class="text-sm mt-1">
            {searchQuery() ? 'Try a different search term' : 'Or scan a barcode'}
          </p>
        </div>
      </Show>

      {/* Product Grid */}
      <Show when={!loading() && products().length > 0}>
        <div class="flex-1 overflow-auto scrollbar-thin">
          <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-4">
            <For each={products()}>
              {(product, index) => (
                <ProductCard 
                  product={product} 
                  onClick={() => handleProductClick(product)}
                  isSelected={selectedIndex() === index()}
                  quickKey={index() < 9 ? index() + 1 : undefined}
                />
              )}
            </For>
          </div>
        </div>
      </Show>
    </div>
  );
};

// ─────────────────────────────────────────────────────────────────────────────
// ProductCard Sub-Component
// ─────────────────────────────────────────────────────────────────────────────

interface ProductCardProps {
  product: ProductDto;
  onClick: () => void;
  isSelected: boolean;
  quickKey?: number;
}

/**
 * Individual product card in the search results grid.
 * 
 * ## Stock Status Behavior
 * ```
 * ┌─────────────────────────────────────────────────────────────────────────┐
 * │                    Stock Status Matrix                                  │
 * │                                                                         │
 * │  trackInventory | allowNegativeStock | stock | Display                 │
 * │  ───────────────|───────────────────|───────|─────────────────────     │
 * │  false          | (ignored)          | N/A   | No badge                │
 * │  true           | false              | > 5   | No badge                │
 * │  true           | false              | 1-5   | "X left" (yellow)       │
 * │  true           | false              | <= 0  | "Out of Stock" (red)    │
 * │  true           | true               | <= 0  | "Back-order" (blue)     │
 * └─────────────────────────────────────────────────────────────────────────┘
 * ```
 */
const ProductCard: Component<ProductCardProps> = (props) => {
  // ⚠️ DO NOT destructure props in SolidJS - it breaks reactivity!
  // Access props.product directly to ensure we always get current values

  /**
   * Determines if the product can be added to cart.
   */
  const isDisabled = (): boolean => {
    if (!props.product.trackInventory) return false;
    if (props.product.allowNegativeStock) return false;
    return (props.product.currentStock ?? 0) <= 0;
  };

  /**
   * Determines stock status for badge display.
   */
  const stockStatus = (): { label: string; color: string } | null => {
    if (!props.product.trackInventory) return null;
    
    const stock = props.product.currentStock ?? 0;
    
    if (stock <= 0) {
      if (props.product.allowNegativeStock) {
        return { label: 'Back-order', color: 'bg-blue-100 text-blue-700' };
      }
      return { label: 'Out of Stock', color: 'bg-danger-100 text-danger-700' };
    }
    
    if (stock <= 5) {
      return { label: `${stock} left`, color: 'bg-warning-100 text-warning-700' };
    }
    
    return null;
  };

  return (
    <button
      onClick={props.onClick}
      class={`product-card text-left relative ${props.isSelected ? 'ring-2 ring-primary-500 border-primary-500' : ''} ${isDisabled() ? 'opacity-50 cursor-not-allowed' : ''}`}
      disabled={isDisabled()}
      aria-selected={props.isSelected}
    >
      {/* Quick Key Badge */}
      <Show when={props.quickKey}>
        <div class="absolute top-2 left-2 w-6 h-6 rounded-full bg-gray-200 text-gray-600 text-xs font-bold flex items-center justify-center">
          {props.quickKey}
        </div>
      </Show>

      {/* Product Name */}
      <h3 class="font-semibold text-gray-900 mb-1 line-clamp-2 pr-8">{props.product.name}</h3>

      {/* SKU */}
      <p class="text-sm text-gray-500 mb-2 font-mono">{props.product.sku}</p>

      {/* Price and Stock Status */}
      <div class="flex items-center justify-between gap-2">
        <span class="price-display">{formatMoney(props.product.priceCents)}</span>

        {/* Stock Status Badge */}
        <Show when={stockStatus()}>
          <span class={`text-xs font-medium px-2 py-0.5 rounded-full ${stockStatus()!.color}`}>
            {stockStatus()!.label}
          </span>
        </Show>
      </div>
    </button>
  );
};

export default ProductSearch;
