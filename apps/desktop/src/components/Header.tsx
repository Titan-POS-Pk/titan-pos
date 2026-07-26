/**
 * Header Component
 *
 * Displays the store name, device info, current time, and action buttons.
 *
 * ## Layout
 * ```
 * ┌─────────────────────────────────────────────────────────────────────────┐
 * │  🏪 Store Name    📱 device-pos-1 (PRIMARY)      12:34 PM     ⚙️      │
 * └─────────────────────────────────────────────────────────────────────────┘
 * ```
 */

import { Component, createSignal, onMount, onCleanup, Show } from 'solid-js';
import type { DeviceInfo, SyncStatus } from '../types';

interface HeaderProps {
  storeName: string;
  deviceInfo: DeviceInfo | null;
  syncStatus: SyncStatus | null;
}

/**
 * Get sync mode badge color.
 */
const getSyncModeColor = (mode: string): string => {
  switch (mode.toLowerCase()) {
    case 'primary':
      return 'bg-green-500 text-white';
    case 'secondary':
      return 'bg-blue-500 text-white';
    case 'auto':
      return 'bg-yellow-500 text-black';
    case 'offline':
    default:
      return 'bg-gray-500 text-white';
  }
};

/**
 * Get connection state indicator.
 */
const getConnectionIndicator = (state: string | undefined): { color: string; pulse: boolean } => {
  switch (state?.toLowerCase()) {
    case 'connected':
    case 'listening':  // PRIMARY hub is listening for connections
      return { color: 'bg-green-400', pulse: false };
    case 'connecting':
    case 'reconnecting':
    case 'discovering':  // AUTO mode election in progress
      return { color: 'bg-yellow-400', pulse: true };
    case 'backoff':
      return { color: 'bg-orange-400', pulse: true };
    case 'error':
      return { color: 'bg-red-400', pulse: false };
    case 'disconnected':
    case 'offline':
    default:
      return { color: 'bg-gray-400', pulse: false };
  }
};

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

  // Derive connection indicator
  const connectionIndicator = () => 
    getConnectionIndicator(props.syncStatus?.connectionState);

  return (
    <header class="h-pos-header bg-primary-700 text-white flex items-center justify-between px-6 shadow-md">
      {/* Store Name */}
      <div class="flex items-center gap-3">
        <span class="text-2xl">🏪</span>
        <h1 class="text-xl font-bold">{props.storeName}</h1>
      </div>

      {/* Device Info - Center Section */}
      <Show when={props.deviceInfo}>
        <div class="flex items-center gap-3 bg-primary-800/50 px-4 py-2 rounded-lg">
          {/* Connection Status Indicator */}
          <div class="flex items-center gap-2">
            <div 
              class={`w-2.5 h-2.5 rounded-full ${connectionIndicator().color} ${connectionIndicator().pulse ? 'animate-pulse' : ''}`}
              title={`Connection: ${props.syncStatus?.connectionState || 'unknown'}`}
            />
          </div>

          {/* Device ID */}
          <div class="flex items-center gap-2">
            <span class="text-lg">📱</span>
            <span class="font-mono text-sm font-semibold">
              {props.deviceInfo!.deviceId}
            </span>
          </div>

          {/* Sync Mode Badge */}
          <span 
            class={`px-2 py-0.5 rounded text-xs font-bold uppercase ${getSyncModeColor(props.deviceInfo!.syncMode)}`}
          >
            {props.deviceInfo!.syncMode}
          </span>

          {/* Pending Sync Count */}
          <Show when={(props.syncStatus?.pendingOutboxCount ?? 0) > 0}>
            <span 
              class="px-2 py-0.5 bg-orange-500 text-white rounded text-xs font-bold"
              title="Pending sync items"
            >
              ⏳ {props.syncStatus!.pendingOutboxCount}
            </span>
          </Show>
        </div>
      </Show>

      {/* Clock & Settings */}
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
