/**
 * POS State Machine
 *
 * Manages the high-level transaction flow using XState v5.
 * UI-level state (search query, selected product) remains in SolidJS signals.
 *
 * ## State Flow
 * ```
 * ┌─────────────────────────────────────────────────────────────────────────────┐
 * │                           POS State Machine                                 │
 * │                                                                             │
 * │  ┌──────────┐   ADD_ITEM   ┌──────────┐   CHECKOUT   ┌──────────┐          │
 * │  │          │─────────────►│          │─────────────►│          │          │
 * │  │   idle   │              │  inCart  │              │  tender  │          │
 * │  │          │◄─────────────│          │◄─────────────│          │          │
 * │  └──────────┘   CLEAR      └──────────┘    CANCEL    └────┬─────┘          │
 * │       ▲                                                   │                │
 * │       │                                    PAYMENT_COMPLETE                │
 * │       │                                                   │                │
 * │       │              NEW_SALE              ┌──────────┐   │                │
 * │       └────────────────────────────────────│          │◄──┘                │
 * │                                            │ receipt  │                    │
 * │                                            │          │                    │
 * │                                            └──────────┘                    │
 * └─────────────────────────────────────────────────────────────────────────────┘
 * ```
 *
 * ## Hybrid Approach
 * - XState: Transaction state (idle, inCart, tender, receipt)
 * - SolidJS Signals: UI state (search query, loading, cart items for display)
 *
 * The machine doesn't hold cart data - that lives in Rust backend.
 * This machine only tracks WHERE we are in the transaction flow.
 */

import { setup, assign } from 'xstate';
import type { ReceiptResponse } from '../types';

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Context holds data that persists across state transitions.
 */
export interface POSContext {
  /** Current sale ID (set after create_sale, cleared after new_sale) */
  saleId: string | null;
  /** Total amount due in cents */
  totalCents: number;
  /** Receipt data after finalization */
  receipt: ReceiptResponse | null;
  /** Error message if something failed */
  error: string | null;
  /** Number of items in cart (for UI hints) */
  itemCount: number;
}

/**
 * Events that can be sent to the machine.
 */
export type POSEvent =
  | { type: 'ADD_ITEM'; itemCount: number }
  | { type: 'UPDATE_CART'; itemCount: number; totalCents: number }
  | { type: 'CLEAR' }
  | { type: 'CHECKOUT'; saleId: string; totalCents: number }
  | { type: 'CANCEL' }
  | { type: 'PAYMENT_COMPLETE'; receipt: ReceiptResponse }
  | { type: 'NEW_SALE' }
  | { type: 'ERROR'; message: string };

// ─────────────────────────────────────────────────────────────────────────────
// Machine Definition
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Creates the POS state machine.
 *
 * ## Usage with SolidJS
 * ```tsx
 * import { useMachine } from '@xstate/solid';
 * import { posMachine } from './machines/posMachine';
 *
 * const App = () => {
 *   const [state, send] = useMachine(posMachine);
 *
 *   // Check current state
 *   if (state.matches('idle')) { ... }
 *   if (state.matches('inCart')) { ... }
 *
 *   // Send events
 *   send({ type: 'ADD_ITEM', itemCount: 1 });
 *   send({ type: 'CHECKOUT', saleId: 'xxx', totalCents: 1000 });
 * };
 * ```
 */
export const posMachine = setup({
  types: {
    context: {} as POSContext,
    events: {} as POSEvent,
  },
  actions: {
    /**
     * Clears all context to initial state.
     */
    clearContext: assign({
      saleId: null,
      totalCents: 0,
      receipt: null,
      error: null,
      itemCount: 0,
    }),

    /**
     * Updates item count when cart changes.
     */
    updateItemCount: assign({
      itemCount: ({ event }) => {
        if (event.type === 'ADD_ITEM' || event.type === 'UPDATE_CART') {
          return event.itemCount;
        }
        return 0;
      },
      totalCents: ({ context, event }) => {
        if (event.type === 'UPDATE_CART') {
          return event.totalCents;
        }
        return context.totalCents;
      },
    }),

    /**
     * Sets sale ID and total when entering tender state.
     */
    setSaleInfo: assign({
      saleId: ({ event }) => {
        if (event.type === 'CHECKOUT') {
          return event.saleId;
        }
        return null;
      },
      totalCents: ({ event }) => {
        if (event.type === 'CHECKOUT') {
          return event.totalCents;
        }
        return 0;
      },
    }),

    /**
     * Stores receipt data after successful payment.
     */
    setReceipt: assign({
      receipt: ({ event }) => {
        if (event.type === 'PAYMENT_COMPLETE') {
          return event.receipt;
        }
        return null;
      },
    }),

    /**
     * Stores error message.
     */
    setError: assign({
      error: ({ event }) => {
        if (event.type === 'ERROR') {
          return event.message;
        }
        return null;
      },
    }),
  },
  guards: {
    /**
     * Checks if cart has items.
     */
    hasItems: ({ context }) => context.itemCount > 0,
  },
}).createMachine({
  id: 'pos',
  initial: 'idle',
  context: {
    saleId: null,
    totalCents: 0,
    receipt: null,
    error: null,
    itemCount: 0,
  },

  states: {
    /**
     * Idle State
     *
     * Initial state when no items are in the cart.
     * Waiting for the first item to be added.
     */
    idle: {
      on: {
        ADD_ITEM: {
          target: 'inCart',
          actions: 'updateItemCount',
        },
      },
    },

    /**
     * In Cart State
     *
     * One or more items are in the cart.
     * User can add more items, update quantities, or proceed to checkout.
     */
    inCart: {
      on: {
        ADD_ITEM: {
          // Stay in inCart, just update count
          actions: 'updateItemCount',
        },
        UPDATE_CART: [
          {
            // If cart becomes empty, go back to idle
            target: 'idle',
            guard: ({ event }) => event.itemCount === 0,
            actions: 'clearContext',
          },
          {
            // Otherwise stay in inCart
            actions: 'updateItemCount',
          },
        ],
        CLEAR: {
          target: 'idle',
          actions: 'clearContext',
        },
        CHECKOUT: {
          target: 'tender',
          actions: 'setSaleInfo',
        },
        ERROR: {
          actions: 'setError',
        },
      },
    },

    /**
     * Tender State
     *
     * Payment modal is open. User is entering payment amount.
     * Sale has been created in the database (draft status).
     */
    tender: {
      on: {
        CANCEL: {
          target: 'inCart',
          // Note: Draft sale remains in DB, will be cleaned up later
        },
        PAYMENT_COMPLETE: {
          target: 'receipt',
          actions: 'setReceipt',
        },
        ERROR: {
          actions: 'setError',
        },
      },
    },

    /**
     * Receipt State
     *
     * Sale is complete. Showing receipt to the user.
     * Cart has been cleared by finalize_sale.
     */
    receipt: {
      on: {
        NEW_SALE: {
          target: 'idle',
          actions: 'clearContext',
        },
      },
    },
  },
});

/**
 * Type helper for the POS machine state.
 */
export type POSMachineState = ReturnType<typeof posMachine.getInitialSnapshot>;
