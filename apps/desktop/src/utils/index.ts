/**
 * Utility Functions
 *
 * Helper functions for common operations throughout the app.
 */

/**
 * Formats a cent amount as a currency string.
 *
 * ## Why Cents?
 * All monetary values are stored as integers (cents) to avoid
 * floating-point precision issues. This function converts to
 * a display string.
 *
 * ## Examples
 * ```typescript
 * formatMoney(1234)      // "$12.34"
 * formatMoney(1234, '€') // "€12.34"
 * formatMoney(-500)      // "-$5.00"
 * formatMoney(100, '$', 0) // "$100" (no decimals)
 * ```
 *
 * @param cents - Amount in cents (integer)
 * @param symbol - Currency symbol (default: "$")
 * @param decimals - Decimal places (default: 2)
 * @returns Formatted currency string
 */
export function formatMoney(cents: number, symbol = '$', decimals = 2): string {
  const divisor = Math.pow(10, decimals);
  const whole = Math.floor(Math.abs(cents) / divisor);
  const frac = Math.abs(cents) % divisor;

  const sign = cents < 0 ? '-' : '';
  const fracStr = decimals > 0 ? '.' + frac.toString().padStart(decimals, '0') : '';

  return `${sign}${symbol}${whole}${fracStr}`;
}

/**
 * Formats a tax rate from basis points to percentage string.
 *
 * ## Basis Points
 * Tax rates are stored as basis points (1 bp = 0.01%).
 * - 825 bps = 8.25%
 * - 1000 bps = 10%
 * - 0 bps = 0% (tax exempt)
 *
 * @param bps - Rate in basis points
 * @returns Percentage string (e.g., "8.25%")
 */
export function formatTaxRate(bps: number): string {
  const percent = bps / 100;
  return `${percent.toFixed(2)}%`;
}

/**
 * Debounces a function call.
 *
 * ## Usage
 * Perfect for search input to avoid excessive API calls:
 * ```typescript
 * const debouncedSearch = debounce(async (query: string) => {
 *   const results = await invoke('search_products', { query });
 *   setProducts(results);
 * }, 150);
 *
 * <input onInput={(e) => debouncedSearch(e.target.value)} />
 * ```
 *
 * @param fn - Function to debounce
 * @param delay - Delay in milliseconds
 * @returns Debounced function
 */
export function debounce<T extends (...args: unknown[]) => unknown>(
  fn: T,
  delay: number
): (...args: Parameters<T>) => void {
  let timeoutId: ReturnType<typeof setTimeout>;

  return (...args: Parameters<T>) => {
    clearTimeout(timeoutId);
    timeoutId = setTimeout(() => fn(...args), delay);
  };
}

/**
 * Truncates text to a maximum length with ellipsis.
 *
 * @param text - Text to truncate
 * @param maxLength - Maximum length (including ellipsis)
 * @returns Truncated text
 */
export function truncate(text: string, maxLength: number): string {
  if (text.length <= maxLength) return text;
  return text.slice(0, maxLength - 3) + '...';
}

/**
 * Formats a date string for display.
 *
 * @param isoString - ISO 8601 date string
 * @returns Formatted date/time string
 */
export function formatDateTime(isoString: string): string {
  const date = new Date(isoString);
  return date.toLocaleString();
}

/**
 * Formats a date string for receipt (shorter format).
 *
 * @param isoString - ISO 8601 date string
 * @returns Short date/time string
 */
export function formatReceiptDate(isoString: string): string {
  const date = new Date(isoString);
  return date.toLocaleDateString() + ' ' + date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

/**
 * Generates a unique key for list rendering.
 * Combines multiple values into a single string key.
 *
 * @param parts - Parts to combine
 * @returns Combined key string
 */
export function makeKey(...parts: (string | number)[]): string {
  return parts.join('-');
}
