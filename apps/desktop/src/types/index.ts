/**
 * TypeScript Types for Titan POS Frontend
 *
 * These types mirror the Rust DTOs from the backend.
 * They are used for type-safe Tauri invoke calls.
 *
 * ## Naming Convention
 * - Rust uses snake_case, TypeScript uses camelCase
 * - Serde's #[serde(rename_all = "camelCase")] handles conversion
 *
 * ## Money Values
 * All monetary values are in CENTS (integer).
 * Use formatMoney() utility to display as currency.
 */

// ─────────────────────────────────────────────────────────────────────────────
// Product Types
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Product data from the backend.
 * 
 * ## Stock Behavior
 * The combination of `trackInventory` and `allowNegativeStock` determines
 * how out-of-stock products are handled:
 * 
 * | trackInventory | allowNegativeStock | Behavior |
 * |----------------|-------------------|----------|
 * | false          | (ignored)         | Always sellable |
 * | true           | false             | Cannot sell when stock <= 0 |
 * | true           | true              | Can sell (back-order), shows warning |
 */
export interface ProductDto {
  /** UUID */
  id: string;
  /** Stock Keeping Unit (e.g., "COKE-330") */
  sku: string;
  /** Barcode (EAN-13, UPC, etc.) */
  barcode: string | null;
  /** Display name */
  name: string;
  /** Optional description */
  description: string | null;
  /** Price in cents (integer!) */
  priceCents: number;
  /** Tax rate in basis points (825 = 8.25%) */
  taxRateBps: number;
  /** Whether inventory is tracked */
  trackInventory: boolean;
  /** Whether selling is allowed when stock is 0 or negative (back-order) */
  allowNegativeStock: boolean;
  /** Current stock level (if tracked) */
  currentStock: number | null;
  /** Whether product is available for sale */
  isActive: boolean;
}

// ─────────────────────────────────────────────────────────────────────────────
// Cart Types
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Cart item (product snapshot in cart).
 */
export interface CartItem {
  /** Product UUID */
  productId: string;
  /** SKU (frozen at time of adding) */
  sku: string;
  /** Name (frozen at time of adding) */
  name: string;
  /** Unit price in cents (frozen at time of adding) */
  unitPriceCents: number;
  /** Tax rate in basis points (frozen) */
  taxRateBps: number;
  /** Quantity in cart */
  quantity: number;
  /** When added to cart */
  addedAt: string;
}

/**
 * Cart totals calculated by the backend.
 */
export interface CartTotals {
  /** Number of unique items */
  itemCount: number;
  /** Total quantity of all items */
  totalQuantity: number;
  /** Subtotal before tax (cents) */
  subtotalCents: number;
  /** Total tax amount (cents) */
  taxCents: number;
  /** Grand total (cents) */
  totalCents: number;
}

/**
 * Full cart response from backend.
 */
export interface CartResponse {
  items: CartItem[];
  totals: CartTotals;
}

// ─────────────────────────────────────────────────────────────────────────────
// Sale Types
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Response after creating a sale.
 */
export interface CreateSaleResponse {
  saleId: string;
  totalCents: number;
  itemCount: number;
}

/**
 * Response after adding a payment.
 */
export interface AddPaymentResponse {
  paymentId: string;
  amountCents: number;
  totalPaidCents: number;
  remainingCents: number;
  changeCents: number;
}

/**
 * Receipt item for display.
 */
export interface ReceiptItem {
  name: string;
  quantity: number;
  unitPriceCents: number;
  lineTotalCents: number;
}

/**
 * Receipt payment entry.
 */
export interface ReceiptPayment {
  method: string;
  amountCents: number;
}

/**
 * Full receipt data after finalizing sale.
 */
export interface ReceiptResponse {
  saleId: string;
  receiptNumber: string;
  storeName: string;
  timestamp: string;
  items: ReceiptItem[];
  subtotalCents: number;
  taxCents: number;
  totalCents: number;
  payments: ReceiptPayment[];
  changeCents: number;
}

// ─────────────────────────────────────────────────────────────────────────────
// Config Types
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Tax calculation mode.
 */
export type TaxMode = 'exclusive' | 'inclusive';

/**
 * Application configuration.
 */
export interface ConfigState {
  tenantId: string;
  storeName: string;
  storeAddress: string[];
  currencyCode: string;
  currencySymbol: string;
  currencyDecimals: number;
  defaultTaxRateBps: number;
  taxMode: TaxMode;
  soundEnabled: boolean;
}

// ─────────────────────────────────────────────────────────────────────────────
// Error Types
// ─────────────────────────────────────────────────────────────────────────────

/**
 * API error from Tauri commands.
 */
export interface ApiError {
  code: ErrorCode;
  message: string;
}

/**
 * Error codes returned by the backend.
 */
export type ErrorCode =
  | 'NOT_FOUND'
  | 'VALIDATION_ERROR'
  | 'DATABASE_ERROR'
  | 'BUSINESS_LOGIC'
  | 'INTERNAL'
  | 'CART_ERROR'
  | 'INSUFFICIENT_STOCK'
  | 'PAYMENT_ERROR';
