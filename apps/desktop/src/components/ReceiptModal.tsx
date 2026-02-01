/**
 * ReceiptModal Component
 *
 * Displays the receipt after a successful sale.
 *
 * ## User Flow
 * ```
 * ┌─────────────────────────────────────────────────────────────────────────────┐
 * │                                                                             │
 * │     ┌─────────────────────────────────────────────────────────┐            │
 * │     │                                                         │            │
 * │     │              🧾 RECEIPT                                 │            │
 * │     │                                                         │            │
 * │     │  ─────────────────────────────────────────────────────  │            │
 * │     │  Titan Store                                            │            │
 * │     │  123 Main Street                                        │            │
 * │     │  Receipt #: 260201-143052-1234                          │            │
 * │     │  Date: Feb 1, 2026 2:30 PM                              │            │
 * │     │  ─────────────────────────────────────────────────────  │            │
 * │     │                                                         │            │
 * │     │  Coca-Cola 330ml      x2         $3.98                  │            │
 * │     │  Chips Lays Classic   x1         $2.49                  │            │
 * │     │  ─────────────────────────────────────────────────────  │            │
 * │     │                                                         │            │
 * │     │  Subtotal                        $6.47                  │            │
 * │     │  Tax (8.25%)                     $0.53                  │            │
 * │     │  ─────────────────────────────────────────────────────  │            │
 * │     │  TOTAL                           $7.00                  │            │
 * │     │  ─────────────────────────────────────────────────────  │            │
 * │     │                                                         │            │
 * │     │  Cash                            $10.00                 │            │
 * │     │  CHANGE                          $3.00                  │            │
 * │     │                                                         │            │
 * │     │  ─────────────────────────────────────────────────────  │            │
 * │     │            Thank you for your purchase!                 │            │
 * │     │  ─────────────────────────────────────────────────────  │            │
 * │     │                                                         │            │
 * │     │       [ Print ]              [ NEW SALE ]               │            │
 * │     │                                                         │            │
 * │     └─────────────────────────────────────────────────────────┘            │
 * │                                                                             │
 * └─────────────────────────────────────────────────────────────────────────────┘
 * ```
 */

import { Component, For, Show } from 'solid-js';
import type { ReceiptResponse, ConfigState } from '../types';
import { formatMoney } from '../utils';

interface ReceiptModalProps {
  /** Receipt data from finalize_sale */
  receipt: ReceiptResponse;
  /** App configuration for currency symbol */
  config: ConfigState | null;
  /** Callback when user clicks "New Sale" */
  onNewSale: () => void;
}

const ReceiptModal: Component<ReceiptModalProps> = (props) => {
  // ─────────────────────────────────────────────────────────────────────────
  // Helpers
  // ─────────────────────────────────────────────────────────────────────────

  const symbol = () => props.config?.currencySymbol ?? '$';

  /**
   * Formats the timestamp for display.
   */
  const formatTimestamp = (isoString: string): string => {
    try {
      const date = new Date(isoString);
      return date.toLocaleString('en-US', {
        month: 'short',
        day: 'numeric',
        year: 'numeric',
        hour: 'numeric',
        minute: '2-digit',
        hour12: true,
      });
    } catch {
      return isoString;
    }
  };

  /**
   * Handles print button click.
   * For v0.1, this just logs to console. Real printing in v1.0+.
   */
  const handlePrint = () => {
    // Log receipt for debugging
    console.log('📄 RECEIPT:', JSON.stringify(props.receipt, null, 2));

    // In v1.0+, this would use the Web Print API or Tauri print plugin
    // For now, trigger browser print dialog on the receipt area
    window.print();
  };

  // ─────────────────────────────────────────────────────────────────────────
  // Render
  // ─────────────────────────────────────────────────────────────────────────

  return (
    <div class="modal-backdrop" onClick={props.onNewSale}>
      <div
        class="modal-content max-w-md p-0 overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Receipt Content - styled like a paper receipt */}
        <div class="receipt-paper bg-white p-6" id="receipt-printable">
          {/* Header */}
          <div class="text-center mb-4">
            <div class="text-2xl mb-1">🧾</div>
            <h2 class="text-xl font-bold text-gray-900">
              {props.receipt.storeName}
            </h2>
            <Show when={props.config?.storeAddress?.length}>
              <For each={props.config?.storeAddress}>
                {(line) => <p class="text-sm text-gray-600">{line}</p>}
              </For>
            </Show>
          </div>

          {/* Receipt Info */}
          <div class="border-t border-dashed border-gray-300 pt-3 mb-3">
            <div class="flex justify-between text-sm text-gray-600">
              <span>Receipt #:</span>
              <span class="font-mono">{props.receipt.receiptNumber}</span>
            </div>
            <div class="flex justify-between text-sm text-gray-600">
              <span>Date:</span>
              <span>{formatTimestamp(props.receipt.timestamp)}</span>
            </div>
          </div>

          {/* Line Items */}
          <div class="border-t border-dashed border-gray-300 py-3 space-y-2">
            <For each={props.receipt.items}>
              {(item) => (
                <div class="flex justify-between text-sm">
                  <div class="flex-1 min-w-0">
                    <span class="truncate">{item.name}</span>
                    <span class="text-gray-500 ml-2">×{item.quantity}</span>
                  </div>
                  <span class="font-mono ml-4">
                    {formatMoney(item.lineTotalCents, symbol())}
                  </span>
                </div>
              )}
            </For>
          </div>

          {/* Totals */}
          <div class="border-t border-dashed border-gray-300 py-3 space-y-1">
            <div class="flex justify-between text-sm">
              <span>Subtotal</span>
              <span class="font-mono">
                {formatMoney(props.receipt.subtotalCents, symbol())}
              </span>
            </div>
            <div class="flex justify-between text-sm text-gray-600">
              <span>Tax</span>
              <span class="font-mono">
                {formatMoney(props.receipt.taxCents, symbol())}
              </span>
            </div>
            <div class="flex justify-between font-bold text-lg mt-2 pt-2 border-t border-gray-300">
              <span>TOTAL</span>
              <span class="font-mono">
                {formatMoney(props.receipt.totalCents, symbol())}
              </span>
            </div>
          </div>

          {/* Payments */}
          <div class="border-t border-dashed border-gray-300 py-3 space-y-1">
            <For each={props.receipt.payments}>
              {(payment) => (
                <div class="flex justify-between text-sm">
                  <span>{payment.method}</span>
                  <span class="font-mono">
                    {formatMoney(payment.amountCents, symbol())}
                  </span>
                </div>
              )}
            </For>
            <Show when={props.receipt.changeCents > 0}>
              <div class="flex justify-between font-bold text-green-600 mt-2 pt-2 border-t border-gray-200">
                <span>CHANGE</span>
                <span class="font-mono">
                  {formatMoney(props.receipt.changeCents, symbol())}
                </span>
              </div>
            </Show>
          </div>

          {/* Footer */}
          <div class="border-t border-dashed border-gray-300 pt-4 text-center">
            <p class="text-sm text-gray-600">Thank you for your purchase!</p>
            <p class="text-xs text-gray-400 mt-1">
              Sale ID: {props.receipt.saleId.slice(0, 8)}...
            </p>
          </div>
        </div>

        {/* Action Buttons (not printed) */}
        <div class="p-4 bg-gray-50 border-t border-gray-200 flex gap-3 print:hidden">
          <button onClick={handlePrint} class="btn btn-secondary flex-1">
            <svg
              class="w-5 h-5 mr-2"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M17 17h2a2 2 0 002-2v-4a2 2 0 00-2-2H5a2 2 0 00-2 2v4a2 2 0 002 2h2m2 4h6a2 2 0 002-2v-4a2 2 0 00-2-2H9a2 2 0 00-2 2v4a2 2 0 002 2zm8-12V5a2 2 0 00-2-2H9a2 2 0 00-2 2v4h10z"
              />
            </svg>
            Print
          </button>
          <button onClick={props.onNewSale} class="btn btn-success flex-1 btn-lg">
            <svg
              class="w-5 h-5 mr-2"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M12 4v16m8-8H4"
              />
            </svg>
            New Sale
          </button>
        </div>
      </div>
    </div>
  );
};

export default ReceiptModal;
