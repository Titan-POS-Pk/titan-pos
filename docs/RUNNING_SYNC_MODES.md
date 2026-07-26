# Running the sync modes

> Verified 2026-02-02 on macOS. Both peer-to-peer modes work end to end;
> cloud uplink is partially wired (see Mode 3).

Step-by-step commands to bring up a multi-device store and observe sync.
Each mode below was run and the output quoted is what it actually printed.

---

## Architecture Overview

### Mode 1: PRIMARY/SECONDARY (Peer-to-Peer) - ✅ WORKING
One POS acts as PRIMARY (hub), others connect as SECONDARY.

```
┌─────────────────────────────────────────────────────────────────────┐
│                    PRIMARY/SECONDARY Mode                           │
│                                                                     │
│  ┌──────────────┐         ┌──────────────┐      ┌──────────────┐   │
│  │   PRIMARY    │◄───────►│  SECONDARY   │      │  SECONDARY   │   │
│  │  (Hub Mode)  │         │   POS #2     │      │   POS #3     │   │
│  │              │◄────────┼──────────────┼──────┤              │   │
│  │  Port 8765   │         │              │      │              │   │
│  └──────────────┘         └──────────────┘      └──────────────┘   │
│         │                        │                     │           │
│         │    WebSocket @ ws://localhost:8765/ws        │           │
│         └────────────────────────┴─────────────────────┘           │
└─────────────────────────────────────────────────────────────────────┘
```

### Mode 2: Standalone Hub Server - ✅ WORKING
Dedicated hub server, all POS terminals connect as clients.

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Standalone Hub Mode                              │
│                                                                     │
│                    ┌──────────────────────┐                         │
│                    │   hub-server         │                         │
│                    │   (standalone bin)   │                         │
│                    │   Port 8765          │                         │
│                    └──────────┬───────────┘                         │
│                               │                                     │
│         ┌─────────────────────┼─────────────────────┐               │
│         ▼                     ▼                     ▼               │
│  ┌──────────────┐     ┌──────────────┐      ┌──────────────┐       │
│  │    POS #1    │     │    POS #2    │      │    POS #3    │       │
│  │ (secondary)  │     │ (secondary)  │      │ (secondary)  │       │
│  └──────────────┘     └──────────────┘      └──────────────┘       │
└─────────────────────────────────────────────────────────────────────┘
```

### Mode 3: Cloud Database Sync - 🚧 IN PROGRESS
Hub server connects to Cloud API (gRPC) which persists all data to PostgreSQL.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Cloud Sync Mode                                     │
│                                                                             │
│  LOCAL NETWORK                           CLOUD                              │
│  ┌─────────────────────────────────────┐ ┌────────────────────────────────┐│
│  │  ┌──────────────────────┐          │ │   ┌─────────────────────────┐  ││
│  │  │   hub-server         │          │ │   │   Cloud API (gRPC)      │  ││
│  │  │   + CloudUplink      │──────────┼─┼──▶│   Port 50051            │  ││
│  │  │   Port 8765          │          │ │   │                         │  ││
│  │  └──────────┬───────────┘          │ │   └───────────┬─────────────┘  ││
│  │             │                       │ │               │                ││
│  │   ┌─────────┼─────────┐            │ │               ▼                ││
│  │   ▼         ▼         ▼            │ │   ┌─────────────────────────┐  ││
│  │ ┌─────┐  ┌─────┐  ┌─────┐         │ │   │   PostgreSQL           │  ││
│  │ │POS 1│  │POS 2│  │POS 3│         │ │   │   Port 5432            │  ││
│  │ └─────┘  └─────┘  └─────┘         │ │   └─────────────────────────┘  ││
│  └─────────────────────────────────────┘ └────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Mode 1: PRIMARY/SECONDARY Demo

### Prerequisites
```bash
cd "$REPO"

# Ensure database exists with seed data
rm -f data/titan.db data/store_aggregates.db
cargo run -p titan-db --bin seed

# Build the desktop app
cargo build -p titan-desktop
```

### Step 1: Start PRIMARY Instance
```bash
# Terminal 1 - PRIMARY (acts as hub on port 8765)
cd "$REPO"/apps/desktop
TITAN_DEVICE_ID=primary \
TITAN_SYNC_MODE=primary \
TITAN_HUB_PORT=8765 \
RUST_LOG=info,titan_sync=debug \
pnpm tauri dev
```

Wait for:
```
Hub server listening addr=0.0.0.0:8765
```

### Step 2: Start SECONDARY Instance
```bash
# Terminal 2 - SECONDARY (connects to PRIMARY hub)
cd "$REPO"/apps/desktop
TITAN_DEVICE_ID=pos-secondary-1 \
TITAN_SYNC_MODE=secondary \
TITAN_HUB_URL="ws://127.0.0.1:8765/ws" \
RUST_LOG=info,titan_sync=debug \
VITE_PORT=5174 \
pnpm tauri dev -- --port 5174
```

Wait for:
```
Handshake complete store_id=local-store
```

### Step 3: Test Sync Flow
1. Make a sale on SECONDARY instance
2. Watch PRIMARY logs for: `Received InventoryDelta`
3. PRIMARY broadcasts `InventoryUpdate` to all clients

---

## Mode 2: Standalone Hub Server Demo

This mode runs a dedicated hub server binary. All POS instances connect as clients.

### Prerequisites
```bash
cd "$REPO"

# Build the hub server
cargo build -p titan-sync --bin hub-server

# Build the desktop app
cargo build -p titan-desktop
```

### Step 1: Start Hub Server
```bash
# Terminal 1 - Standalone Hub Server
./target/debug/hub-server --port 8765 --store-id demo-store
```

Expected output:
```
Starting Titan Hub Server, port: 8765, store_id: demo-store, bind_addr: 0.0.0.0
Hub server listening, addr: 0.0.0.0:8765
WebSocket endpoint: ws://0.0.0.0:8765/ws
Health endpoint: http://0.0.0.0:8765/health
Stats endpoint: http://0.0.0.0:8765/stats
```

Verify the server is running:
```bash
curl http://localhost:8765/stats
# {"clients":[],"connected_clients":0,"status":"running","store_id":"demo-store"}
```

### Step 2: Start POS Terminal 1
```bash
# Terminal 2 - POS Terminal 1
cd "$REPO"/apps/desktop
TITAN_SYNC_MODE=secondary \
TITAN_HUB_URL="ws://127.0.0.1:8765/ws" \
TITAN_DEVICE_ID="pos-terminal-1" \
TITAN_DEVICE_NAME="Register 1" \
RUST_LOG=info,titan_sync=debug,tungstenite=warn,tokio_tungstenite=warn \
pnpm tauri dev
```

Wait for:
```
Handshake complete store_id=demo-store
connection_state: "connected"
```

Check hub stats - should show 1 client:
```bash
curl http://localhost:8765/stats
# {"clients":["pos-terminal-1"],"connected_clients":1,"status":"running","store_id":"demo-store"}
```

### Step 3: Start POS Terminal 2
```bash
# Terminal 3 - POS Terminal 2
cd "$REPO"/apps/desktop
TITAN_SYNC_MODE=secondary \
TITAN_HUB_URL="ws://127.0.0.1:8765/ws" \
TITAN_DEVICE_ID="pos-terminal-2" \
TITAN_DEVICE_NAME="Register 2" \
RUST_LOG=info,titan_sync=debug,tungstenite=warn,tokio_tungstenite=warn \
VITE_PORT=5174 \
pnpm tauri dev -- --port 5174
```

Check hub stats - should show 2 clients:
```bash
curl http://localhost:8765/stats
# {"clients":["pos-terminal-1","pos-terminal-2"],"connected_clients":2,"status":"running","store_id":"demo-store"}
```

### Step 4: Test Sync Flow
1. Make a sale on POS Terminal 1
2. Hub server logs show: `Received InventoryDelta` → `Broadcasting InventoryUpdate`
3. POS Terminal 2 receives: `Received InventoryUpdate`
4. Stock is synced across all terminals

---

## Mode 3: Cloud Database Sync Demo (In Progress)

This mode adds cloud sync - the hub server uploads all sync data to a PostgreSQL database via gRPC.

### Prerequisites
```bash
cd "$REPO"

# Start PostgreSQL and Cloud API with Docker
docker compose --profile cloud up -d

# Verify services are running
docker compose ps
# Should show: postgres (5432), redis (6379), cloud-api (50051)

# Check cloud-api health
curl http://localhost:50051/health 2>/dev/null || echo "Cloud API may use gRPC only"
```

### Step 1: Start Hub Server with Cloud Uplink
```bash
# Terminal 1 - Hub Server with Cloud connection
# NOTE: Cloud uplink integration is in progress
# For now, use Mode 2 (standalone hub without cloud)
./target/debug/hub-server --port 8765 --store-id demo-store
```

### Step 2: Start POS Terminals
Same as Mode 2 (see above).

### Step 3: Verify Cloud Sync
```bash
# Connect to PostgreSQL and check for synced data
docker exec -it titan-pos-postgres-1 psql -U titan -d titan_cloud

# List sales
SELECT * FROM sales LIMIT 5;

# List inventory deltas
SELECT * FROM inventory_deltas LIMIT 5;
```

### Cloud Services

| Service | Port | Description |
|---------|------|-------------|
| PostgreSQL | 5432 | Cloud database |
| Redis | 6379 | Caching & rate limiting |
| Cloud API | 50051 | gRPC server |

### Docker Commands
```bash
# Start cloud services
docker compose --profile cloud up -d

# View logs
docker compose logs -f cloud-api

# Stop services
docker compose --profile cloud down

# Reset database (removes all data)
docker compose --profile cloud down -v
docker compose --profile cloud up -d
```

---

## Hub Server Options

```
USAGE:
    hub-server [OPTIONS]

OPTIONS:
    -p, --port <PORT>       Port to listen on [default: 8765]
    -s, --store-id <ID>     Store ID to accept [default: local-store]
    -b, --bind <ADDR>       Bind address [default: 0.0.0.0]
    -h, --help              Print help
```

### Endpoints

| Endpoint | Description |
|----------|-------------|
| `/ws` | WebSocket connection for POS clients |
| `/health` | Health check - returns "OK" |
| `/stats` | JSON stats: connected clients, store_id |

---

## Verified Features

### ✅ Bidirectional Inventory Sync
- POS makes sale → sends `InventoryDelta` to hub
- Hub receives → broadcasts `InventoryUpdate` to ALL clients
- Other POS terminals apply the delta to their local DB

### ✅ Self-Echo Prevention
When a POS receives its own inventory update echoed back:
```rust
if update.source_device_id == my_device_id {
    debug!("Skipping self-echoed inventory update (already applied locally)");
    return Ok(());
}
```

### ✅ Outbox Pattern
Sales are queued in `sync_outbox` table and sent to hub:
- Every 5 seconds, outbox processor checks for pending entries
- Entries are batched and sent via WebSocket
- Acknowledged entries are marked as synced

### ✅ Store ID Validation
Hub validates that connecting clients have matching store_id:
```
STORE_MISMATCH: Expected store 'demo-store', got 'other-store'
```

---

## Environment Variables Reference

| Variable | Description | Example |
|----------|-------------|---------|
| `TITAN_DEVICE_ID` | Unique device identifier | `pos-terminal-1` |
| `TITAN_DEVICE_NAME` | Human-readable device name | `Register 1` |
| `TITAN_SYNC_MODE` | Sync role: `primary`, `secondary`, `auto` | `secondary` |
| `TITAN_HUB_PORT` | Port for PRIMARY hub server | `8765` |
| `TITAN_HUB_URL` | WebSocket URL for connection | `ws://127.0.0.1:8765/ws` |
| `RUST_LOG` | Log level (filter noisy crates) | `info,titan_sync=debug,tungstenite=warn,tokio_tungstenite=warn` |
| `VITE_PORT` | Frontend dev server port | `5174` |

---

## Troubleshooting

### Port Already in Use
```bash
# Kill processes on ports
lsof -ti:5173,5174,8765 | xargs kill -9 2>/dev/null

# Kill all titan processes
pkill -f "titan-desktop" 2>/dev/null
pkill -f "hub-server" 2>/dev/null
pkill -f "vite" 2>/dev/null
```

### Store ID Mismatch
If you see `STORE_MISMATCH` errors:
1. POS uses `demo-store` by default in SyncConfig
2. Start hub with `--store-id demo-store`
3. Or set `TITAN_STORE_ID=local-store` when starting POS

### WebSocket Connection Failed
1. Ensure hub server is running: `curl http://localhost:8765/health`
2. Check TITAN_HUB_URL matches hub's address
3. Verify firewall allows connections

### Connection Reset After Hello
Check store_id matches between hub and POS.
Hub rejects connections with mismatched store_id.

---

## Log Messages to Watch

### Hub Server Logs
```
New WebSocket connection addr=127.0.0.1:XXXXX   # Client connecting
Client authenticated device_id=pos-terminal-1   # Handshake success
Received InventoryDelta product_id=...          # Sale from POS
Broadcasting InventoryUpdate to all clients     # Sent to all
```

### POS Client Logs
```
WebSocket connected                              # Connected to hub
Hello message sent successfully                  # Sent authentication
Received message msg_type=Welcome                # Hub accepted
Handshake complete store_id=demo-store           # Ready to sync
Sent InventoryDelta to hub product_id=...        # Sale synced
Received InventoryUpdate                         # Update from hub
```

---

## Code References

- Hub server binary: `crates/titan-sync/src/bin/hub_server.rs`
- Hub core (shared logic): `crates/titan-sync/src/hub_core.rs`
- Self-echo prevention: `crates/titan-sync/src/inbound.rs` @ `process_inventory_update()`
- Embedded hub: `crates/titan-sync/src/hub.rs`
- Protocol messages: `crates/titan-sync/src/protocol.rs`
- Cloud uplink (gRPC): `crates/titan-sync/src/cloud_uplink.rs`
- Cloud uplink adapter: `crates/titan-sync/src/cloud_uplink_adapter.rs`
- Cloud API server: `apps/cloud-api/src/main.rs`
- Frontend events: `apps/desktop/src/App.tsx` @ `setupEventListeners()`

---

## Architecture Comparison

| Feature | PRIMARY/SECONDARY | Standalone Hub | Cloud Sync |
|---------|-------------------|----------------|------------|
| Hub process | Embedded in PRIMARY POS | Separate `hub-server` binary | Hub + Cloud API |
| Hub availability | Depends on PRIMARY POS uptime | Can run 24/7 independently | 24/7 + cloud backup |
| Data persistence | Local SQLite only | Local SQLite only | SQLite + PostgreSQL |
| Failover | SECONDARY promotes to PRIMARY | Manual restart required | Auto-failover possible |
| Multi-store | Single store | Single store | Multi-store aggregation |
| Use case | Small stores | Medium stores | Enterprise/chains |
| Deployment | Simpler | More flexible | Most complex |
