/**
 * Cart Component
 *
 * Displays the current shopping cart in the sidebar.
 *
 * ## Layout
 * ```
 * ┌─────────────────────────────────────────┐
 * │  CART                         3 items   │
 * ├─────────────────────────────────────────┤
 * │                                         │
 * │  Coca-Cola 330ml              x2        │
 * │  $1.99 each                    $3.98    │
 * │  [−] [+] [🗑]                           │
 * │                                         │
 * │  Chips Lays Classic           x1        │
 * │  $2.49 each                    $2.49    │
 * │  [−] [+] [🗑]                           │
 * │                                         │
 * ├─────────────────────────────────────────┤
 * │  Subtotal                      $6.47    │
 * │  Tax (8.25%)                   $0.53    │
 * │  ─────────────────────────────────────  │
 * │  TOTAL                         $7.00    │
 * ├─────────────────────────────────────────┤
 * │        [ Clear ]  [ CHECKOUT ]          │
 * └─────────────────────────────────────────┘
 * ```
 */

import { Component, For, Show } from 'solid-js';
import type { CartResponse, ConfigState, CartItem } from '../types';
import { formatMoney, formatTaxRate } from '../utils';

interface CartProps {
  cart: CartResponse;
  config: ConfigState | null;
  onUpdateItem: (productId: string, quantity: number) => void;
  onRemoveItem: (productId: string) => void;
  onClearCart: () => void;
  onCheckout: () => void;
}

const Cart: Component<CartProps> = (props) => {
  const { cart, config } = props;
  const symbol = () => config?.currencySymbol ?? '$';

  return (
    <div class="flex flex-col h-full">
      {/* Header */}
      <div class="p-4 border-b border-gray-200 flex items-center justify-between">
        <h2 class="text-lg font-bold text-gray-900">Cart</h2>
        <span class="text-sm text-gray-500">
          {cart.totals.itemCount} {cart.totals.itemCount === 1 ? 'item' : 'items'}
        </span>
      </div>

      {/* Cart Items */}
      <div class="flex-1 overflow-auto p-4">
        <Show when={cart.items.length === 0}>
          <div class="flex flex-col items-center justify-center h-full text-gray-400">
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
                d="M3 3h2l.4 2M7 13h10l4-8H5.4M7 13L5.4 5M7 13l-2.293 2.293c-.63.63-.184 1.707.707 1.707H17m0 0a2 2 0 100 4 2 2 0 000-4zm-8 2a2 2 0 11-4 0 2 2 0 014 0z"
              />
            </svg>
            <p class="text-lg">Cart is empty</p>
            <p class="text-sm mt-1">Add products to get started</p>
          </div>
        </Show>

        <For each={cart.items}>
          {(item) => (
            <CartItemRow
              item={item}
              symbol={symbol()}
              onUpdateQuantity={(qty) => props.onUpdateItem(item.productId, qty)}
              onRemove={() => props.onRemoveItem(item.productId)}
            />
          )}
        </For>
      </div>

      {/* Totals */}
      <Show when={cart.items.length > 0}>
        <div class="border-t border-gray-200 p-4 space-y-2">
          {/* Subtotal */}
          <div class="flex justify-between text-sm text-gray-600">
            <span>Subtotal</span>
            <span>{formatMoney(cart.totals.subtotalCents, symbol())}</span>
          </div>

          {/* Tax */}
          <div class="flex justify-between text-sm text-gray-600">
            <span>Tax ({formatTaxRate(config?.defaultTaxRateBps ?? 825)})</span>
            <span>{formatMoney(cart.totals.taxCents, symbol())}</span>
          </div>

          {/* Divider */}
          <div class="border-t border-gray-300 my-2" />

          {/* Total */}
          <div class="flex justify-between items-center">
            <span class="text-lg font-bold text-gray-900">Total</span>
            <span class="price-display-large">
              {formatMoney(cart.totals.totalCents, symbol())}
            </span>
          </div>
        </div>

        {/* Action Buttons */}
        <div class="p-4 border-t border-gray-200 flex gap-3">
          <button onClick={props.onClearCart} class="btn btn-secondary flex-1">
            Clear
          </button>
          <button onClick={props.onCheckout} class="btn btn-primary flex-1 btn-lg">
            Checkout
          </button>
        </div>
      </Show>
    </div>
  );
};

// ─────────────────────────────────────────────────────────────────────────────
// CartItemRow Sub-Component
// ─────────────────────────────────────────────────────────────────────────────

interface CartItemRowProps {
  item: CartItem;
  symbol: string;
  onUpdateQuantity: (quantity: number) => void;
  onRemove: () => void;
}

/**
 * Individual cart item row with quantity controls.
 */
const CartItemRow: Component<CartItemRowProps> = (props) => {
  const { item, symbol } = props;

  const lineTotal = () => item.unitPriceCents * item.quantity;

  const handleDecrement = () => {
    if (item.quantity > 1) {
      props.onUpdateQuantity(item.quantity - 1);
    } else {
      props.onRemove();
    }
  };

  const handleIncrement = () => {
    props.onUpdateQuantity(item.quantity + 1);
  };

  return (
    <div class="cart-item">
      <div class="flex-1 min-w-0">
        {/* Product Name */}
        <h4 class="font-medium text-gray-900 truncate">{item.name}</h4>

        {/* Unit Price */}
        <p class="text-sm text-gray-500">
          {formatMoney(item.unitPriceCents, symbol)} each
        </p>

        {/* Quantity Controls */}
        <div class="flex items-center gap-2 mt-2">
          <button
            onClick={handleDecrement}
            class="w-8 h-8 flex items-center justify-center rounded-md bg-gray-100 hover:bg-gray-200 text-gray-700"
          >
            −
          </button>
          <span class="w-8 text-center font-medium">{item.quantity}</span>
          <button
            onClick={handleIncrement}
            class="w-8 h-8 flex items-center justify-center rounded-md bg-gray-100 hover:bg-gray-200 text-gray-700"
          >
            +
          </button>
          <button
            onClick={props.onRemove}
            class="ml-2 w-8 h-8 flex items-center justify-center rounded-md hover:bg-red-50 text-red-500"
            title="Remove item"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
              />
            </svg>
          </button>
        </div>
      </div>

      {/* Line Total */}
      <div class="text-right ml-4">
        <span class="font-mono font-semibold text-gray-900">
          {formatMoney(lineTotal(), symbol)}
        </span>
      </div>
    </div>
  );
};

export default Cart;
