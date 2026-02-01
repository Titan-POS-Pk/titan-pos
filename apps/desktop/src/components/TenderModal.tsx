/**
 * TenderModal Component
 *
 * Payment processing modal for completing a sale.
 *
 * ## User Flow
 * ```
 * ┌─────────────────────────────────────────────────────────────────────────┐
 * │                                                                         │
 * │     ┌─────────────────────────────────────────────────────────┐        │
 * │     │                    PAYMENT                              │        │
 * │     │                                                         │        │
 * │     │  Total Due:               $25.00                        │        │
 * │     │  ──────────────────────────────────                     │        │
 * │     │                                                         │        │
 * │     │  Amount Tendered:                                       │        │
 * │     │  ┌───────────────────────────────────────────────┐      │        │
 * │     │  │                                     $30.00    │      │        │
 * │     │  └───────────────────────────────────────────────┘      │        │
 * │     │                                                         │        │
 * │     │  ┌───┐ ┌───┐ ┌───┐                                     │        │
 * │     │  │ 1 │ │ 2 │ │ 3 │                                     │        │
 * │     │  └───┘ └───┘ └───┘                                     │        │
 * │     │  ┌───┐ ┌───┐ ┌───┐                                     │        │
 * │     │  │ 4 │ │ 5 │ │ 6 │                                     │        │
 * │     │  └───┘ └───┘ └───┘                                     │        │
 * │     │  ┌───┐ ┌───┐ ┌───┐                                     │        │
 * │     │  │ 7 │ │ 8 │ │ 9 │                                     │        │
 * │     │  └───┘ └───┘ └───┘                                     │        │
 * │     │  ┌───┐ ┌───┐ ┌───┐                                     │        │
 * │     │  │ . │ │ 0 │ │ ⌫ │                                     │        │
 * │     │  └───┘ └───┘ └───┘                                     │        │
 * │     │                                                         │        │
 * │     │  Change Due:              $5.00                         │        │
 * │     │                                                         │        │
 * │     │  [ Cancel ]    [ CASH ]    [ CARD ]                    │        │
 * │     │                                                         │        │
 * │     └─────────────────────────────────────────────────────────┘        │
 * │                                                                         │
 * └─────────────────────────────────────────────────────────────────────────┘
 * ```
 */

import { Component, createSignal, Show } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import type { AddPaymentResponse, ReceiptResponse, ConfigState } from '../types';
import { formatMoney } from '../utils';

interface TenderModalProps {
  saleId: string;
  totalCents: number;
  config: ConfigState | null;
  onComplete: (receipt: ReceiptResponse) => void;
  onCancel: () => void;
}

const TenderModal: Component<TenderModalProps> = (props) => {
  // ─────────────────────────────────────────────────────────────────────────
  // State
  // ─────────────────────────────────────────────────────────────────────────

  /**
   * Amount entered (in cents, stored as string for display).
   * We build this as a string to handle decimal entry properly.
   */
  const [amountStr, setAmountStr] = createSignal('');

  /**
   * Total amount paid so far.
   */
  const [paidCents, setPaidCents] = createSignal(0);

  /**
   * Processing state for payment submission.
   */
  const [processing, setProcessing] = createSignal(false);

  /**
   * Error message if payment fails.
   */
  const [error, setError] = createSignal<string | null>(null);

  // ─────────────────────────────────────────────────────────────────────────
  // Computed Values
  // ─────────────────────────────────────────────────────────────────────────

  const symbol = () => props.config?.currencySymbol ?? '$';

  /**
   * Auto-detect entry mode based on input.
   * 
   * ## Detection Logic
   * ```
   * ┌─────────────────────────────────────────────────────────────────────────┐
   * │  Auto-Detect Numpad Entry Mode                                         │
   * │                                                                         │
   * │  Input         │ Detection          │ Cents Value │ Display            │
   * │  ──────────────┼────────────────────┼─────────────┼─────────────────── │
   * │  "3000"        │ No decimal → cents │ 3000        │ $30.00             │
   * │  "30.00"       │ Has decimal → $    │ 3000        │ $30.00             │
   * │  "30"          │ No decimal → cents │ 30          │ $0.30              │
   * │  "30."         │ Has decimal → $    │ 3000        │ $30.00             │
   * │  "30.5"        │ Has decimal → $    │ 3050        │ $30.50             │
   * └─────────────────────────────────────────────────────────────────────────┘
   * ```
   */
  const enteredCents = (): number => {
    const str = amountStr();
    if (!str) return 0;

    // If contains decimal, treat as dollars (user typed 30.00 for $30.00)
    if (str.includes('.')) {
      const [whole, frac = ''] = str.split('.');
      const wholeNum = parseInt(whole || '0', 10);
      // Pad or truncate fraction to exactly 2 digits
      const fracPadded = frac.padEnd(2, '0').slice(0, 2);
      const fracNum = parseInt(fracPadded, 10);
      return wholeNum * 100 + fracNum;
    }

    // No decimal = cents (user typed 3000 for $30.00)
    return parseInt(str, 10) || 0;
  };

  /**
   * Returns the detected entry mode for UI hint.
   */
  const entryMode = (): 'cents' | 'dollars' => {
    return amountStr().includes('.') ? 'dollars' : 'cents';
  };

  /**
   * Remaining amount due.
   */
  const remainingCents = () => Math.max(0, props.totalCents - paidCents());

  /**
   * Change to give back (if entered amount > remaining).
   */
  const changeCents = () => Math.max(0, enteredCents() - remainingCents());

  /**
   * Whether the current entry is sufficient to complete the sale.
   */
  const canComplete = () => enteredCents() >= remainingCents() && enteredCents() > 0;

  // ─────────────────────────────────────────────────────────────────────────
  // Numpad Handlers
  // ─────────────────────────────────────────────────────────────────────────

  const handleNumpadPress = (key: string) => {
    setError(null);

    if (key === 'backspace') {
      setAmountStr((prev) => prev.slice(0, -1));
      return;
    }

    if (key === 'clear') {
      setAmountStr('');
      return;
    }

    if (key === '.') {
      // Only allow one decimal point
      if (amountStr().includes('.')) return;
      setAmountStr((prev) => (prev || '0') + '.');
      return;
    }

    // Numeric key
    setAmountStr((prev) => {
      // Limit decimal places
      if (prev.includes('.')) {
        const [, frac] = prev.split('.');
        if (frac && frac.length >= 2) return prev;
      }
      return prev + key;
    });
  };

  /**
   * Quick amount buttons (exact amount, +$5, +$10, +$20).
   */
  const handleQuickAmount = (cents: number) => {
    setError(null);
    // Convert cents to display string
    const dollars = (cents / 100).toFixed(2);
    setAmountStr(dollars);
  };

  // ─────────────────────────────────────────────────────────────────────────
  // Payment Processing
  // ─────────────────────────────────────────────────────────────────────────

  const processPayment = async (method: 'cash' | 'card') => {
    if (!canComplete()) return;

    setProcessing(true);
    setError(null);

    try {
      // Add payment
      const paymentResponse = await invoke<AddPaymentResponse>('add_payment', {
        saleId: props.saleId,
        amountCents: enteredCents(),
        method,
      });

      // If fully paid, finalize the sale
      if (paymentResponse.remainingCents === 0) {
        const receipt = await invoke<ReceiptResponse>('finalize_sale', {
          saleId: props.saleId,
        });

        props.onComplete(receipt);
      } else {
        // Update paid amount for split tender
        setPaidCents(paymentResponse.totalPaidCents);
        setAmountStr('');
      }
    } catch (err) {
      console.error('Payment failed:', err);
      setError(`Payment failed: ${err}`);
    } finally {
      setProcessing(false);
    }
  };

  // ─────────────────────────────────────────────────────────────────────────
  // Render
  // ─────────────────────────────────────────────────────────────────────────

  return (
    <div class="modal-backdrop" onClick={props.onCancel}>
      <div class="modal-content p-6" onClick={(e) => e.stopPropagation()}>
        {/* Header */}
        <div class="text-center mb-6">
          <h2 class="text-2xl font-bold text-gray-900">Payment</h2>
        </div>

        {/* Total Due */}
        <div class="bg-gray-100 rounded-lg p-4 mb-6">
          <div class="flex justify-between items-center">
            <span class="text-gray-600">Total Due</span>
            <span class="price-display-large">
              {formatMoney(props.totalCents, symbol())}
            </span>
          </div>

          <Show when={paidCents() > 0}>
            <div class="flex justify-between items-center mt-2 text-sm">
              <span class="text-gray-500">Paid</span>
              <span class="text-green-600 font-medium">
                -{formatMoney(paidCents(), symbol())}
              </span>
            </div>
            <div class="flex justify-between items-center mt-1 border-t pt-2">
              <span class="text-gray-600 font-medium">Remaining</span>
              <span class="font-bold">{formatMoney(remainingCents(), symbol())}</span>
            </div>
          </Show>
        </div>

        {/* Amount Entry */}
        <div class="mb-4">
          <div class="flex justify-between items-center mb-2">
            <label class="block text-sm font-medium text-gray-700">
              Amount Tendered
            </label>
            <span class="text-xs text-gray-400">
              {entryMode() === 'cents' 
                ? 'Type cents (3000 = $30.00)' 
                : 'Type dollars (30.00 = $30.00)'}
            </span>
          </div>
          <div class="input input-lg text-right font-mono text-2xl bg-white">
            {amountStr() ? formatMoney(enteredCents(), symbol()) : symbol() + '0.00'}
          </div>
          <Show when={amountStr() && !amountStr().includes('.')}>
            <p class="text-xs text-gray-400 mt-1 text-right">
              Tip: Add "." for dollar entry
            </p>
          </Show>
        </div>

        {/* Quick Amount Buttons */}
        <div class="flex gap-2 mb-4">
          <button
            onClick={() => handleQuickAmount(remainingCents())}
            class="btn btn-secondary flex-1 text-sm"
          >
            Exact
          </button>
          <button
            onClick={() => handleQuickAmount(500)}
            class="btn btn-secondary flex-1 text-sm"
          >
            $5
          </button>
          <button
            onClick={() => handleQuickAmount(1000)}
            class="btn btn-secondary flex-1 text-sm"
          >
            $10
          </button>
          <button
            onClick={() => handleQuickAmount(2000)}
            class="btn btn-secondary flex-1 text-sm"
          >
            $20
          </button>
        </div>

        {/* Numpad */}
        <div class="grid grid-cols-3 gap-2 mb-4">
          {['1', '2', '3', '4', '5', '6', '7', '8', '9', '.', '0', 'backspace'].map(
            (key) => (
              <button
                onClick={() => handleNumpadPress(key)}
                class={`numpad-btn ${key === 'backspace' ? 'bg-red-50 text-red-600' : ''}`}
              >
                {key === 'backspace' ? '⌫' : key}
              </button>
            )
          )}
        </div>

        {/* Change Display */}
        <Show when={changeCents() > 0}>
          <div class="bg-green-50 border border-green-200 rounded-lg p-4 mb-4">
            <div class="flex justify-between items-center">
              <span class="text-green-700 font-medium">Change Due</span>
              <span class="text-2xl font-bold text-green-700">
                {formatMoney(changeCents(), symbol())}
              </span>
            </div>
          </div>
        </Show>

        {/* Error Display */}
        <Show when={error()}>
          <div class="bg-red-50 border border-red-200 rounded-lg p-3 mb-4">
            <p class="text-red-700 text-sm">{error()}</p>
          </div>
        </Show>

        {/* Action Buttons */}
        <div class="flex gap-3">
          <button onClick={props.onCancel} class="btn btn-secondary flex-1">
            Cancel
          </button>
          <button
            onClick={() => processPayment('cash')}
            disabled={!canComplete() || processing()}
            class="btn btn-success flex-1"
          >
            {processing() ? 'Processing...' : 'Cash'}
          </button>
          <button
            onClick={() => processPayment('card')}
            disabled={!canComplete() || processing()}
            class="btn btn-primary flex-1"
          >
            {processing() ? 'Processing...' : 'Card'}
          </button>
        </div>
      </div>
    </div>
  );
};

export default TenderModal;
