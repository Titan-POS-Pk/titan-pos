/**
 * Toast Notification Component
 *
 * Displays temporary notifications for success, error, warning, and info messages.
 *
 * ## Usage
 * ```tsx
 * import { ToastProvider, useToast } from './Toast';
 *
 * // Wrap your app
 * <ToastProvider>
 *   <App />
 * </ToastProvider>
 *
 * // Use in any component
 * const toast = useToast();
 * toast.success('Item added to cart');
 * toast.error('Payment failed');
 * toast.warning('Low stock warning');
 * toast.info('Sale saved as draft');
 * ```
 *
 * ## Visual Design
 * ```
 * ┌─────────────────────────────────────────────────────────────────────────────┐
 * │  Toast Container (top-right corner)                                        │
 * │                                                                             │
 * │                                    ┌─────────────────────────────────────┐  │
 * │                                    │ ✓ Item added to cart          [×]  │  │
 * │                                    └─────────────────────────────────────┘  │
 * │                                    ┌─────────────────────────────────────┐  │
 * │                                    │ ✗ Payment failed: Network error [×]│  │
 * │                                    └─────────────────────────────────────┘  │
 * │                                                                             │
 * └─────────────────────────────────────────────────────────────────────────────┘
 * ```
 */

import {
  Component,
  createContext,
  createSignal,
  For,
  JSX,
  useContext,
} from 'solid-js';

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

export type ToastType = 'success' | 'error' | 'warning' | 'info';

export interface Toast {
  id: string;
  type: ToastType;
  message: string;
  duration: number;
}

export interface ToastContextValue {
  /** Show a success toast */
  success: (message: string, duration?: number) => void;
  /** Show an error toast */
  error: (message: string, duration?: number) => void;
  /** Show a warning toast */
  warning: (message: string, duration?: number) => void;
  /** Show an info toast */
  info: (message: string, duration?: number) => void;
  /** Dismiss a specific toast */
  dismiss: (id: string) => void;
  /** Dismiss all toasts */
  dismissAll: () => void;
}

// ─────────────────────────────────────────────────────────────────────────────
// Context
// ─────────────────────────────────────────────────────────────────────────────

const ToastContext = createContext<ToastContextValue>();

/**
 * Hook to access toast functions.
 *
 * @throws Error if used outside ToastProvider
 */
export function useToast(): ToastContextValue {
  const context = useContext(ToastContext);
  if (!context) {
    throw new Error('useToast must be used within a ToastProvider');
  }
  return context;
}

// ─────────────────────────────────────────────────────────────────────────────
// Provider Component
// ─────────────────────────────────────────────────────────────────────────────

interface ToastProviderProps {
  children: JSX.Element;
}

/**
 * Toast provider component. Wrap your app to enable toast notifications.
 */
export const ToastProvider: Component<ToastProviderProps> = (props) => {
  const [toasts, setToasts] = createSignal<Toast[]>([]);

  let toastId = 0;

  const addToast = (type: ToastType, message: string, duration = 4000) => {
    const id = `toast-${++toastId}`;
    const toast: Toast = { id, type, message, duration };

    setToasts((prev) => [...prev, toast]);

    // Auto-dismiss after duration
    if (duration > 0) {
      setTimeout(() => {
        dismiss(id);
      }, duration);
    }

    return id;
  };

  const dismiss = (id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  };

  const dismissAll = () => {
    setToasts([]);
  };

  const contextValue: ToastContextValue = {
    success: (message, duration) => addToast('success', message, duration),
    error: (message, duration) => addToast('error', message, duration ?? 6000), // Errors stay longer
    warning: (message, duration) => addToast('warning', message, duration),
    info: (message, duration) => addToast('info', message, duration),
    dismiss,
    dismissAll,
  };

  return (
    <ToastContext.Provider value={contextValue}>
      {props.children}
      <ToastContainer toasts={toasts()} onDismiss={dismiss} />
    </ToastContext.Provider>
  );
};

// ─────────────────────────────────────────────────────────────────────────────
// Toast Container
// ─────────────────────────────────────────────────────────────────────────────

interface ToastContainerProps {
  toasts: Toast[];
  onDismiss: (id: string) => void;
}

const ToastContainer: Component<ToastContainerProps> = (props) => {
  return (
    <div class="fixed top-4 right-4 z-50 flex flex-col gap-2 pointer-events-none">
      <For each={props.toasts}>
        {(toast) => (
          <ToastItem toast={toast} onDismiss={() => props.onDismiss(toast.id)} />
        )}
      </For>
    </div>
  );
};

// ─────────────────────────────────────────────────────────────────────────────
// Toast Item
// ─────────────────────────────────────────────────────────────────────────────

interface ToastItemProps {
  toast: Toast;
  onDismiss: () => void;
}

const ToastItem: Component<ToastItemProps> = (props) => {
  // Determine styles based on toast type
  const styles = {
    success: {
      bg: 'bg-green-50 border-green-200',
      icon: 'text-green-500',
      text: 'text-green-800',
    },
    error: {
      bg: 'bg-red-50 border-red-200',
      icon: 'text-red-500',
      text: 'text-red-800',
    },
    warning: {
      bg: 'bg-yellow-50 border-yellow-200',
      icon: 'text-yellow-500',
      text: 'text-yellow-800',
    },
    info: {
      bg: 'bg-blue-50 border-blue-200',
      icon: 'text-blue-500',
      text: 'text-blue-800',
    },
  };

  const style = () => styles[props.toast.type];

  const icons = {
    success: (
      <svg class="w-5 h-5" fill="currentColor" viewBox="0 0 20 20">
        <path
          fill-rule="evenodd"
          d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z"
          clip-rule="evenodd"
        />
      </svg>
    ),
    error: (
      <svg class="w-5 h-5" fill="currentColor" viewBox="0 0 20 20">
        <path
          fill-rule="evenodd"
          d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z"
          clip-rule="evenodd"
        />
      </svg>
    ),
    warning: (
      <svg class="w-5 h-5" fill="currentColor" viewBox="0 0 20 20">
        <path
          fill-rule="evenodd"
          d="M8.257 3.099c.765-1.36 2.722-1.36 3.486 0l5.58 9.92c.75 1.334-.213 2.98-1.742 2.98H4.42c-1.53 0-2.493-1.646-1.743-2.98l5.58-9.92zM11 13a1 1 0 11-2 0 1 1 0 012 0zm-1-8a1 1 0 00-1 1v3a1 1 0 002 0V6a1 1 0 00-1-1z"
          clip-rule="evenodd"
        />
      </svg>
    ),
    info: (
      <svg class="w-5 h-5" fill="currentColor" viewBox="0 0 20 20">
        <path
          fill-rule="evenodd"
          d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7-4a1 1 0 11-2 0 1 1 0 012 0zM9 9a1 1 0 000 2v3a1 1 0 001 1h1a1 1 0 100-2v-3a1 1 0 00-1-1H9z"
          clip-rule="evenodd"
        />
      </svg>
    ),
  };

  return (
    <div
      class={`
        pointer-events-auto
        flex items-center gap-3 
        px-4 py-3 rounded-lg shadow-lg border
        min-w-[300px] max-w-[450px]
        animate-slide-up
        ${style().bg}
      `}
    >
      {/* Icon */}
      <div class={style().icon}>{icons[props.toast.type]}</div>

      {/* Message */}
      <p class={`flex-1 text-sm font-medium ${style().text}`}>
        {props.toast.message}
      </p>

      {/* Dismiss Button */}
      <button
        onClick={props.onDismiss}
        class={`
          p-1 rounded hover:bg-black/5 transition-colors
          ${style().text}
        `}
        title="Dismiss"
      >
        <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 20 20">
          <path
            fill-rule="evenodd"
            d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z"
            clip-rule="evenodd"
          />
        </svg>
      </button>
    </div>
  );
};

export default ToastProvider;
