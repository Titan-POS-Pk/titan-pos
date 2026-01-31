/**
 * Header Component
 *
 * Displays the store name, current time, and action buttons.
 *
 * ## Layout
 * ```
 * ┌─────────────────────────────────────────────────────────────────────────┐
 * │  🏪 Store Name                              12:34 PM     ⚙️  Settings  │
 * └─────────────────────────────────────────────────────────────────────────┘
 * ```
 */

import { Component, createSignal, onMount, onCleanup } from 'solid-js';

interface HeaderProps {
  storeName: string;
}

const Header: Component<HeaderProps> = (props) => {
  // Current time, updated every second
  const [time, setTime] = createSignal(new Date());

  // Update clock every second
  onMount(() => {
    const interval = setInterval(() => {
      setTime(new Date());
    }, 1000);

    // Cleanup on unmount
    onCleanup(() => clearInterval(interval));
  });

  // Format time for display
  const formattedTime = () =>
    time().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

  const formattedDate = () =>
    time().toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' });

  return (
    <header class="h-pos-header bg-primary-700 text-white flex items-center justify-between px-6 shadow-md">
      {/* Store Name */}
      <div class="flex items-center gap-3">
        <span class="text-2xl">🏪</span>
        <h1 class="text-xl font-bold">{props.storeName}</h1>
      </div>

      {/* Clock */}
      <div class="flex items-center gap-4">
        <div class="text-right">
          <div class="text-lg font-mono font-semibold">{formattedTime()}</div>
          <div class="text-sm text-primary-200">{formattedDate()}</div>
        </div>

        {/* Settings Button (placeholder) */}
        <button
          class="p-2 rounded-lg hover:bg-primary-600 transition-colors"
          title="Settings"
        >
          <svg
            class="w-6 h-6"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"
            />
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
            />
          </svg>
        </button>
      </div>
    </header>
  );
};

export default Header;
