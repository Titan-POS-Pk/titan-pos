# Multi-Instance Testing Guide

## Overview

Titan POS v0.2 Milestone 4 includes complete support for multi-device sync within a store. This guide explains how to test the sync functionality by running two POS instances on a single machine.

## Prerequisites

- Rust toolchain installed
- Node.js/pnpm installed  
- Ports 8765, 8766 available (WebSocket hubs for POS instances)
- Ports 5173, 5174, 5175 available (Vite dev servers)

## Quick Start

### 1. Initialize Test Databases

```bash
cd "$REPO"

# This creates data/pos1 and data/pos2 with seeded products
./scripts/test-multi-instance.sh
```

**Output:**
- ✅ Creates `data/pos1/titan.db` with 500 products
- ✅ Creates `data/pos2/titan.db` with 500 products  
- ✅ Creates sync config files
- Displays instructions for starting both instances

### 2. Start PRIMARY Instance (Terminal 1)

```bash
cd "$REPO"/apps/desktop

TITAN_DATA_DIR=""$REPO"/data/pos1" \
TITAN_DEVICE_ID="device-pos-1" \
TITAN_SYNC_MODE="primary" \
TITAN_HUB_PORT=8765 \
pnpm tauri dev
```

**What happens:**
- Vite dev server starts on port 5173
- Tauri app launches with PRIMARY sync mode
- WebSocket hub starts on port 8765
- Opens browser at http://localhost:5173

### 3. Start SECONDARY Instance (Terminal 2)

```bash
cd "$REPO"/apps/desktop

TITAN_DATA_DIR=""$REPO"/data/pos2" \
TITAN_DEVICE_ID="device-pos-2" \
TITAN_SYNC_MODE="secondary" \
TITAN_HUB_URL="ws://127.0.0.1:8765/ws" \
VITE_PORT=5175 \
"$REPO"/scripts/tauri-dev-multi-instance.sh 5175
```

**What happens:**
- Helper script updates tauri.conf.json to use port 5175
- Vite dev server starts on port 5175
- Tauri app launches with SECONDARY sync mode
- Connects to PRIMARY hub at ws://127.0.0.1:8765
- Opens browser at http://localhost:5175
- Config automatically reverts when you exit

### Why the Helper Script?

Tauri's `tauri.conf.json` has a hardcoded `devUrl` field that specifies which port the Tauri process expects the Vite dev server on. When running multiple instances:

1. Instance 1 (PRIMARY): Uses default port 5173 ✅
2. Instance 2 (SECONDARY): Needs port 5175, but config is hardcoded

**Solution:** The `tauri-dev-multi-instance.sh` script:
- Takes port number as argument (e.g., `5175`)
- Updates `tauri.conf.json` with the correct devUrl
- Starts `pnpm tauri dev` with `VITE_PORT` environment variable
- Automatically restores config when process exits

This ensures both Tauri and Vite are on the same port for each instance.

## Environment Variables Reference

### For PRIMARY Instance (Port 5173)

```bash
# Data directory with sqlite database
TITAN_DATA_DIR=""$REPO"/data/pos1"

# Unique device ID for this POS
TITAN_DEVICE_ID="device-pos-1"

# Force this instance to be PRIMARY (auto-elected otherwise)
TITAN_SYNC_MODE="primary"

# WebSocket hub port for other devices to connect to
TITAN_HUB_PORT=8765
```

### For SECONDARY Instance (Port 5175)

```bash
# Data directory with sqlite database
TITAN_DATA_DIR=""$REPO"/data/pos2"

# Unique device ID for this POS
TITAN_DEVICE_ID="device-pos-2"

# Force this instance to be SECONDARY
TITAN_SYNC_MODE="secondary"

# URL of the PRIMARY's WebSocket hub
TITAN_HUB_URL="ws://127.0.0.1:8765/ws"

# Vite port for this instance (different from PRIMARY)
VITE_PORT=5175
```

## Test Scenarios

Once both instances are running, try these tests:

### Test 1: Inventory Sync (Primary to Secondary)

1. On POS 1 (PRIMARY):
   - Search for any product (e.g., "coca")
   - Add 2 to cart
   - Finalize sale with cash payment

2. Check POS 2 (SECONDARY):
   - Search for same product
   - Verify stock decreased by 2
   - ✅ **Success**: Inventory delta synced in real-time

### Test 2: Inventory Sync (Secondary to Primary)

1. On POS 2 (SECONDARY):
   - Search for different product (e.g., "pepsi")
   - Add 3 to cart
   - Finalize sale with card payment

2. Check POS 1 (PRIMARY):
   - Search for same product
   - Verify stock decreased by 3
   - ✅ **Success**: Secondary delta uploaded to primary

### Test 3: Failover Detection

1. Both instances running
2. Stop POS 1 (PRIMARY):
   - Ctrl+C in Terminal 1

3. Check POS 2 (SECONDARY):
   - Should show connection error in console or UI
   - ✅ **Success**: Heartbeat timeout detected

4. Restart POS 1:
   - Run the PRIMARY command again in Terminal 1
   - After ~5-10 seconds, POS 2 should reconnect
   - ✅ **Success**: Automatic reconnection works

### Test 4: Store Aggregation (Inventory + Sales)

The PRIMARY instance runs a `StoreAggregator` service that:

1. **Real-time inventory aggregation**:
   - Tracks current stock for all products
   - Stored in `data/pos1/store_aggregates.db`

2. **Batched sales aggregation** (60-second batches, configurable):
   - Collects sales from all POS devices
   - Creates hourly/daily summaries
   - Prepares for cloud sync

View aggregation database:

```bash
# Check inventory snapshot for a product
sqlite3 data/pos1/store_aggregates.db \
  "SELECT product_id, quantity, snapshot_at FROM inventory_snapshots LIMIT 5;"

# Check today's sales summary
sqlite3 data/pos1/store_aggregates.db \
  "SELECT * FROM sales_summaries WHERE period_type = 'hour' LIMIT 5;"

# Check device activity
sqlite3 data/pos1/store_aggregates.db \
  "SELECT * FROM device_activity;"
```

## Troubleshooting

### Port Already in Use

**Error:** `Error: Port 5173 is already in use`

**Solution:**
```bash
# Kill lingering processes
pkill -9 -f "node|cargo|vite"

# Or use lsof to find and kill specific process
lsof -i :5173  # Find what's using port
kill -9 <PID>  # Kill the process
```

### SQLx Compilation Error

**Error:** `set DATABASE_URL to use query macros online`

**Solution:**  
The SQLx offline cache should be in `.sqlx/` directory. If missing:

```bash
cd "$REPO"
DATABASE_URL="sqlite:data/titan.db" cargo sqlx prepare --workspace
```

### Database Already Has Products

When running `test-multi-instance.sh` multiple times:

```bash
# Clean up and restart
rm -rf data/pos1 data/pos2
./scripts/test-multi-instance.sh
```

### Secondary Won't Connect

**Check:**
1. PRIMARY instance is running (Terminal 1)
2. WebSocket hub started (check logs for "listening on 8765")
3. Network connectivity: `curl http://127.0.0.1:8765/` should respond
4. TITAN_HUB_URL is correct: `ws://127.0.0.1:8765/ws`

## Architecture

### Instance 1 (PRIMARY)

```
┌─────────────────────────────────────────┐
│         Tauri App (PORT 5173)           │
│  ┌──────────────────────────────────┐   │
│  │    SolidJS Frontend              │   │
│  │  - Search products               │   │
│  │  - Add to cart                   │   │
│  │  - Complete sales                │   │
│  └────────────────┬─────────────────┘   │
│                   │ invoke()             │
│  ┌────────────────▼─────────────────┐   │
│  │  Rust Backend                     │   │
│  │  - Command handlers               │   │
│  │  - Database access                │   │
│  └────────────────┬─────────────────┘   │
│                   │                      │
│  ┌────────────────▼─────────────────┐   │
│  │  SyncAgent (PRIMARY MODE)         │   │
│  │  ┌──────────────────────────────┐ │   │
│  │  │ WebSocket Hub (8765)         │ │   │
│  │  │  • Accepts POS connections   │ │   │
│  │  │  • Broadcasts inventory      │ │   │
│  │  └──────────────────────────────┘ │   │
│  │ ┌──────────────────────────────┐  │   │
│  │ │ StoreAggregator              │  │   │
│  │ │  • Real-time inventory       │  │   │
│  │ │  • 60-sec sales batches      │  │   │
│  │ │  • store_aggregates.db       │  │   │
│  │ └──────────────────────────────┘  │   │
│  └─────────────────────────────────┘   │
│                   │                     │
│  ┌────────────────▼─────────────────┐  │
│  │  SQLite DB (titan.db)             │  │
│  │  - Products, sales, payments      │  │
│  │  - Sync outbox, cursors           │  │
│  └──────────────────────────────────┘  │
└─────────────────────────────────────────┘
```

### Instance 2 (SECONDARY)

```
┌─────────────────────────────────────────┐
│         Tauri App (PORT 5175)           │
│  ┌──────────────────────────────────┐   │
│  │    SolidJS Frontend              │   │
│  │  - Search products               │   │
│  │  - Add to cart                   │   │
│  │  - Complete sales                │   │
│  └────────────────┬─────────────────┘   │
│                   │ invoke()             │
│  ┌────────────────▼─────────────────┐   │
│  │  Rust Backend                     │   │
│  │  - Command handlers               │   │
│  │  - Database access                │   │
│  └────────────────┬─────────────────┘   │
│                   │                      │
│  ┌────────────────▼─────────────────┐   │
│  │  SyncAgent (SECONDARY MODE)       │   │
│  │  ┌──────────────────────────────┐ │   │
│  │  │ WebSocket Client              │ │   │
│  │  │  • Connects to PRIMARY (8765) │ │   │
│  │  │  • Receives inventory         │ │   │
│  │  │  • Sends sales batches        │ │   │
│  │  └──────────────────────────────┘ │   │
│  │ (No StoreAggregator - only PRIMARY)    │
│  └─────────────────────────────────────┘   │
│                   │                     │
│  ┌────────────────▼─────────────────┐  │
│  │  SQLite DB (titan.db)             │  │
│  │  - Products, sales, payments      │  │
│  │  - Sync outbox, cursors           │  │
│  └──────────────────────────────────┘  │
└─────────────────────────────────────────┘
           │
           │ ws://127.0.0.1:8765/ws
           ▼
    (PRIMARY WebSocket Hub)
```

## Additional Resources

- [Architecture Guide](../../docs/architecture/SYNC_ARCHITECTURE.md)
- [Progress Document](../../docs/PROGRESS.md)
- [Vite Configuration](vite.config.ts)
- [Tauri Configuration](src-tauri/tauri.conf.json)
