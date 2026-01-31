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
 * ## Performance
 * - Search is debounced (150ms) to avoid excessive API calls
 * - FTS5 search on backend is <10ms for 50k products
 */

import { Component, createSignal, createEffect, For, Show } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import type { ProductDto } from '../types';
import { formatMoney, debounce } from '../utils';

interface ProductSearchProps {
  /** Called when user clicks a product to add to cart */
  onAddToCart: (productId: string, quantity?: number) => void;
}

const ProductSearch: Component<ProductSearchProps> = (props) => {
  // ─────────────────────────────────────────────────────────────────────────
  // State
  // ─────────────────────────────────────────────────────────────────────────

  const [searchQuery, setSearchQuery] = createSignal('');
  const [products, setProducts] = createSignal<ProductDto[]>([]);
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  // ─────────────────────────────────────────────────────────────────────────
  // Search Logic
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * Performs the actual search query to the backend.
   * Debounced to avoid excessive API calls while typing.
   */
  const performSearch = debounce(async (query: string) => {
    setLoading(true);
    setError(null);

    try {
      const results = await invoke<ProductDto[]>('search_products', {
        query,
        limit: 50,
      });
      setProducts(results);
    } catch (err) {
      console.error('Search failed:', err);
      setError('Search failed. Please try again.');
      setProducts([]);
    } finally {
      setLoading(false);
    }
  }, 150);

  // Trigger search when query changes
  createEffect(() => {
    const query = searchQuery();
    performSearch(query);
  });

  // ─────────────────────────────────────────────────────────────────────────
  // Event Handlers
  // ─────────────────────────────────────────────────────────────────────────

  const handleSearchInput = (e: Event) => {
    const input = e.target as HTMLInputElement;
    setSearchQuery(input.value);
  };

  const handleProductClick = (product: ProductDto) => {
    props.onAddToCart(product.id, 1);
  };

  const handleClearSearch = () => {
    setSearchQuery('');
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
          type="text"
          placeholder="Search products by name, SKU, or barcode..."
          value={searchQuery()}
          onInput={handleSearchInput}
          class="input input-lg pl-12 pr-12"
          autofocus
        />

        {/* Clear Button */}
        <Show when={searchQuery()}>
          <button
            onClick={handleClearSearch}
            class="absolute inset-y-0 right-0 pr-4 flex items-center"
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

      {/* Loading State */}
      <Show when={loading()}>
        <div class="flex items-center justify-center py-8">
          <div class="text-gray-500">Searching...</div>
        </div>
      </Show>

      {/* Error State */}
      <Show when={error()}>
        <div class="flex items-center justify-center py-8">
          <div class="text-red-500">{error()}</div>
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
        </div>
      </Show>

      {/* Product Grid */}
      <Show when={!loading() && products().length > 0}>
        <div class="flex-1 overflow-auto scrollbar-thin">
          <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-4">
            <For each={products()}>
              {(product) => (
                <ProductCard product={product} onClick={() => handleProductClick(product)} />
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
}

/**
 * Individual product card in the search results grid.
 */
const ProductCard: Component<ProductCardProps> = (props) => {
  const { product } = props;

  // Stock status
  const stockStatus = () => {
    if (!product.trackInventory) return null;
    if (product.currentStock === null) return null;
    if (product.currentStock <= 0) return { label: 'Out of Stock', color: 'text-red-500' };
    if (product.currentStock <= 5) return { label: `${product.currentStock} left`, color: 'text-yellow-600' };
    return null;
  };

  return (
    <button
      onClick={props.onClick}
      class="product-card text-left"
      disabled={product.trackInventory && (product.currentStock ?? 0) <= 0}
    >
      {/* Product Name */}
      <h3 class="font-semibold text-gray-900 mb-1 line-clamp-2">{product.name}</h3>

      {/* SKU */}
      <p class="text-sm text-gray-500 mb-2">{product.sku}</p>

      {/* Price */}
      <div class="flex items-center justify-between">
        <span class="price-display">{formatMoney(product.priceCents)}</span>

        {/* Stock Status Badge */}
        <Show when={stockStatus()}>
          <span class={`text-xs font-medium ${stockStatus()!.color}`}>
            {stockStatus()!.label}
          </span>
        </Show>
      </div>
    </button>
  );
};

export default ProductSearch;
