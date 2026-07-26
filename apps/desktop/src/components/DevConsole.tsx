/**
 * DevConsole Component
 *
 * A toggleable developer console panel that shows sync events, inventory updates,
 * and other system logs in real-time.
 *
 * ## Layout
 * ```text
 * ┌─────────────────────────────────────────────────────────────────────────┐
 * │                          Main Application                               │
 * │                                                                         │
 * └─────────────────────────────────────────────────────────────────────────┘
 * ┌─────────────────────────────────────────────────────────────────────────┐
 * │  📋 Dev Console                                     [Clear] [▼ Hide]   │
 * ├─────────────────────────────────────────────────────────────────────────┤
 * │  12:34:05 [SYNC]     Connected to hub ws://localhost:8765              │
 * │  12:34:06 [INVENTORY] Stock updated: 3 products                        │
 * │  12:34:10 [SALE]     Sale RCP-123 synced                               │
 * │  12:34:15 [INFO]     Heartbeat OK, latency: 12ms                       │
 * └─────────────────────────────────────────────────────────────────────────┘
 * ```
 *
 * ## Features
 * - Collapsible panel (toggle with button or keyboard shortcut)
 * - Auto-scroll to latest logs
 * - Log level filtering (INFO, SYNC, INVENTORY, ERROR)
 * - Clear all logs button
 * - Colored log levels
 * - Timestamps for each entry
 */

import { Component, For, Show, createEffect, onCleanup } from 'solid-js';

/**
 * Single log entry in the console.
 */
export interface LogEntry {
  /** Unique ID for this log */
  id: number;
  /** Timestamp when this log was created */
  timestamp: Date;
  /** Log level/category */
  level: 'INFO' | 'SYNC' | 'INVENTORY' | 'SALE' | 'ERROR' | 'DEBUG';
  /** The log message */
  message: string;
  /** Optional additional data */
  data?: unknown;
}

interface DevConsoleProps {
  /** Current log entries */
  logs: LogEntry[];
  /** Called when user clears logs */
  onClear: () => void;
  /** Whether the console is visible */
  isVisible: boolean;
  /** Called when visibility is toggled */
  onToggle: () => void;
}

/**
 * Get the CSS class for a log level.
 */
const getLevelClass = (level: LogEntry['level']): string => {
  switch (level) {
    case 'ERROR':
      return 'text-red-500 bg-red-50';
    case 'SYNC':
      return 'text-blue-500 bg-blue-50';
    case 'INVENTORY':
      return 'text-green-500 bg-green-50';
    case 'SALE':
      return 'text-purple-500 bg-purple-50';
    case 'DEBUG':
      return 'text-gray-400 bg-gray-50';
    case 'INFO':
    default:
      return 'text-gray-600 bg-gray-50';
  }
};

/**
 * Format timestamp for display.
 */
const formatTime = (date: Date): string => {
  return date.toLocaleTimeString('en-US', {
    hour12: false,
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
};

const DevConsole: Component<DevConsoleProps> = (props) => {
  // Reference to the log container for auto-scroll
  let logContainerRef: HTMLDivElement | undefined;

  // Auto-scroll when new logs are added
  createEffect(() => {
    // Access logs length to track changes - this creates a reactive dependency
    void props.logs.length;
    
    // Scroll to bottom when new logs are added
    if (logContainerRef && props.isVisible) {
      requestAnimationFrame(() => {
        logContainerRef!.scrollTop = logContainerRef!.scrollHeight;
      });
    }
  });

  // Keyboard shortcut: Ctrl/Cmd + ` to toggle console
  const handleKeyDown = (e: KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && e.key === '`') {
      e.preventDefault();
      props.onToggle();
    }
  };

  // Add keyboard listener
  if (typeof window !== 'undefined') {
    window.addEventListener('keydown', handleKeyDown);
    onCleanup(() => window.removeEventListener('keydown', handleKeyDown));
  }

  return (
    <>
      {/* Toggle Button (always visible at bottom) */}
      <Show when={!props.isVisible}>
        <button
          onClick={props.onToggle}
          class="fixed bottom-4 left-1/2 -translate-x-1/2 bg-gray-800 text-white px-4 py-2 rounded-full text-sm font-mono shadow-lg hover:bg-gray-700 transition-colors flex items-center gap-2 z-50"
          title="Open Dev Console (Ctrl+`)"
        >
          <span class="text-green-400">▶</span>
          Dev Console ({props.logs.length})
        </button>
      </Show>

      {/* Console Panel */}
      <Show when={props.isVisible}>
        <div class="fixed bottom-0 left-0 right-0 h-64 bg-gray-900 border-t-2 border-gray-700 shadow-2xl z-50 flex flex-col font-mono text-sm">
          {/* Header */}
          <div class="flex items-center justify-between px-4 py-2 bg-gray-800 border-b border-gray-700">
            <div class="flex items-center gap-2 text-gray-300">
              <span class="text-green-400">📋</span>
              <span class="font-semibold">Dev Console</span>
              <span class="text-gray-500">({props.logs.length} entries)</span>
            </div>

            <div class="flex items-center gap-2">
              {/* Clear Button */}
              <button
                onClick={props.onClear}
                class="px-3 py-1 text-xs bg-gray-700 hover:bg-gray-600 text-gray-300 rounded transition-colors"
              >
                Clear
              </button>

              {/* Hide Button */}
              <button
                onClick={props.onToggle}
                class="px-3 py-1 text-xs bg-gray-700 hover:bg-gray-600 text-gray-300 rounded transition-colors flex items-center gap-1"
                title="Hide Console (Ctrl+`)"
              >
                <span>▼</span> Hide
              </button>
            </div>
          </div>

          {/* Log Content */}
          <div
            ref={logContainerRef}
            class="flex-1 overflow-auto p-2 space-y-1"
          >
            <Show when={props.logs.length === 0}>
              <div class="text-gray-500 text-center py-8">
                No logs yet. Events will appear here as they occur.
              </div>
            </Show>

            <For each={props.logs}>
              {(log) => (
                <div class="flex items-start gap-2 px-2 py-1 hover:bg-gray-800 rounded">
                  {/* Timestamp */}
                  <span class="text-gray-500 shrink-0">
                    {formatTime(log.timestamp)}
                  </span>

                  {/* Level Badge */}
                  <span
                    class={`px-2 py-0.5 rounded text-xs font-bold shrink-0 ${getLevelClass(log.level)}`}
                  >
                    {log.level}
                  </span>

                  {/* Message */}
                  <span class="text-gray-300 break-all">
                    {log.message}
                  </span>

                  {/* Optional Data */}
                  <Show when={log.data}>
                    <span class="text-gray-500 text-xs">
                      {JSON.stringify(log.data)}
                    </span>
                  </Show>
                </div>
              )}
            </For>
          </div>

          {/* Footer with hints */}
          <div class="px-4 py-1 bg-gray-800 border-t border-gray-700 text-xs text-gray-500">
            <span class="mr-4">Ctrl+` to toggle</span>
            <span>Logs: sync:status, sync:progress, sync:error, inventory:update</span>
          </div>
        </div>
      </Show>
    </>
  );
};

export default DevConsole;
