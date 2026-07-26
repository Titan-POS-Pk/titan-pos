# Sync verification checklist

Run through this to confirm the sync infrastructure is behaving after a change.

---

## 🚀 Quick Start (Recommended)

Use the simplified demo script for reliable multi-instance testing:

```bash
cd /path/to/titan-pos

# Terminal 1: Setup and start PRIMARY
./scripts/run-demo.sh setup
./scripts/run-demo.sh primary

# Terminal 2: Start SECONDARY (after PRIMARY is listening)
./scripts/run-demo.sh secondary
```

The script handles database setup, environment variables, and port configuration automatically.

---

## Pre-Flight Checks

### Environment Setup
```bash
# Navigate to project
cd /path/to/titan-pos

# Ensure dependencies are built
cargo build

# Verify docker is running (only needed for cloud sync demos)
docker info > /dev/null && echo "✓ Docker OK" || echo "✗ Docker not running"
```

### Clear Previous State (Optional)
```bash
# Reset to clean state for fresh testing
# NOTE: Also remove WAL/SHM files to avoid I/O errors
rm -f data/titan*.db data/titan*.db-shm data/titan*.db-wal data/store_aggregates.db
cargo run -p titan-db --bin seed
```

---

## ⚠️ Important: Multiple Windows

When running multiple POS instances manually (not using run-demo.sh), each needs a **unique Vite port**:

| Instance | VITE_PORT |
|----------|-----------|
| 1st window | (default 5173) |
| 2nd window | 5174 |
| 3rd window | 5175 |
| 4th window | 5176 |

---

## Demo 1: Single POS (Basic Functionality)

**Time: 2 minutes**

### Start
```bash
cd apps/desktop
pnpm tauri dev
```

> **Note**: Default mode is now `primary`, which starts the hub server automatically.
> You can customize settings by editing `apps/desktop/.env` or using environment variables.

### Verify
- [ ] App window opens
- [ ] Products grid shows items
- [ ] Click "Dev" badge to open DevConsole
- [ ] DevConsole shows Device ID: `demo-pos-1` (from .env)
- [ ] DevConsole shows Sync Mode: `primary`
- [ ] Add product to cart → Cart shows item
- [ ] Complete sale → Receipt modal appears
- [ ] New sale → Cart is empty

### Expected DevConsole State
```
Device ID: demo-pos-1
Sync Mode: primary (hub server listening)
Sync Status: listening
```

---

## Demo 2: Two POS with Auto-Election

**Time: 5 minutes**

> ⚠️ **Note**: Auto-election requires mDNS discovery which is not fully implemented yet. Use Demo 3 (explicit PRIMARY/SECONDARY) for testing.

### Start POS 1 (will become PRIMARY)
```bash
cd apps/desktop
TITAN_DEVICE_ID="pos-alpha" TITAN_DEVICE_PRIORITY="80" TITAN_SYNC_MODE="auto" pnpm tauri dev
```

Wait 3 seconds for election...

### Start POS 2 (will become SECONDARY)
```bash
cd apps/desktop
TITAN_DEVICE_ID="pos-beta" TITAN_DEVICE_PRIORITY="50" TITAN_SYNC_MODE="auto" VITE_PORT=5174 pnpm tauri dev
```

### Verify
- [ ] POS-Alpha DevConsole shows: `Role: PRIMARY`, `Hub Port: 8080`
- [ ] POS-Beta DevConsole shows: `Role: SECONDARY`, `Hub URL: ws://...`
- [ ] On POS-Beta: Add product to cart, complete sale
- [ ] Check POS-Alpha terminal logs for "Received inventory delta"
- [ ] Both devices show same inventory count for sold product

### Failover Test
1. [ ] Kill POS-Alpha (Ctrl+C)
2. [ ] Wait 5-10 seconds
3. [ ] POS-Beta DevConsole should change to: `Role: PRIMARY`
4. [ ] Restart POS-Alpha with same command
5. [ ] POS-Alpha should now show: `Role: SECONDARY`

---

## Demo 3: Dedicated Server + POS Terminals ⭐ (Recommended)

**Time: 5 minutes**

This is the most reliable demo - explicit PRIMARY/SECONDARY roles without auto-election.

### Start Server (PRIMARY) - Terminal 1
```bash
cd apps/desktop
TITAN_DEVICE_ID="server-01" TITAN_SYNC_MODE="primary" TITAN_HUB_PORT="8765" pnpm tauri dev
```

**Wait for this log message before starting terminals:**
```
INFO titan_desktop_lib: Hub server listening addr=0.0.0.0:8765
```

### Start Terminal 1 (SECONDARY) - Terminal 2
```bash
cd apps/desktop
TITAN_DEVICE_ID="term-01" TITAN_SYNC_MODE="secondary" TITAN_HUB_URL="ws://localhost:8765/sync" VITE_PORT=5174 pnpm tauri dev
```

### Start Terminal 2 (SECONDARY) - Terminal 3
```bash
cd apps/desktop
TITAN_DEVICE_ID="term-02" TITAN_SYNC_MODE="secondary" TITAN_HUB_URL="ws://localhost:8765/sync" VITE_PORT=5175 pnpm tauri dev
```

### Verify
- [ ] Server shows `Hub server listening addr=0.0.0.0:8765` in terminal
- [ ] Server logs show: `Client connected to hub` when terminals start
- [ ] Server DevConsole shows: Mode: `primary`, Hub Port: `8765`
- [ ] Both terminals DevConsole show: Mode: `secondary`, Hub URL: `ws://localhost:8765/sync`
- [ ] Both terminals show `Connected to hub` (Sync Status: Connected)
- [ ] Make sale on Terminal 1
- [ ] Terminal 2 sees inventory update
- [ ] Server aggregates the sale (check logs or `store_aggregates.db`)

---

## Demo 4: Cloud Sync (PostgreSQL)

**Time: 10 minutes**

### Start Cloud Infrastructure
```bash
docker compose --profile cloud up -d

# Wait for healthy status
docker compose --profile cloud ps
# All services should show "Up (healthy)"
```

### Seed Cloud Database
```bash
docker exec -it titan-postgres psql -U titan -d titan_pos << 'EOF'
INSERT INTO tenants (id, name) VALUES ('demo-tenant', 'Demo Company')
ON CONFLICT (id) DO NOTHING;

INSERT INTO stores (id, tenant_id, name, api_key_hash) VALUES 
  ('demo-store', 'demo-tenant', 'Demo Store', 
   'test_hash_placeholder')
ON CONFLICT (id) DO NOTHING;

INSERT INTO store_configs (store_id, tenant_id, store_name) VALUES 
  ('demo-store', 'demo-tenant', 'Demo Store')
ON CONFLICT (store_id) DO NOTHING;
EOF
```

### Start Store Hub with Cloud Uplink
```bash
cd apps/desktop

TITAN_DEVICE_ID="hub-demo" \
TITAN_SYNC_MODE="primary" \
TITAN_HUB_PORT="8080" \
TITAN_CLOUD_URL="http://localhost:50051" \
TITAN_STORE_ID="demo-store" \
TITAN_TENANT_ID="demo-tenant" \
TITAN_API_KEY="test_key" \
pnpm tauri dev
```

### Verify Cloud Connection
- [ ] Hub logs show `Authenticating with cloud API`
- [ ] Hub logs show `Connected to cloud` or `JWT token obtained`
- [ ] Make a sale
- [ ] Check cloud logs: `docker logs titan-cloud-api --tail 20`
- [ ] Query cloud database:
```bash
docker exec -it titan-postgres psql -U titan -d titan_pos -c \
  "SELECT id, total_cents FROM sales WHERE store_id = 'demo-store' LIMIT 5;"
```

---

## Demo 5: Multi-Store (Advanced)

**Time: 15 minutes**

### Prerequisites
- Cloud infrastructure running (Demo 4)
- Two stores seeded in cloud database

### Architecture
```
Cloud API (:50051)
    │
    ├── Store A (Hub :8080) ── Terminal A1
    │
    └── Store B (Hub :8081) ── Terminal B1
```

### Start Store A
```bash
# Hub A
TITAN_DEVICE_ID="hub-a" TITAN_SYNC_MODE="primary" TITAN_HUB_PORT="8080" \
TITAN_CLOUD_URL="http://localhost:50051" TITAN_STORE_ID="store-a" \
pnpm tauri dev

# Terminal A1
TITAN_DEVICE_ID="term-a1" TITAN_SYNC_MODE="secondary" \
TITAN_HUB_URL="ws://localhost:8080/sync" \
pnpm tauri dev
```

### Start Store B
```bash
# Hub B
TITAN_DEVICE_ID="hub-b" TITAN_SYNC_MODE="primary" TITAN_HUB_PORT="8081" \
TITAN_CLOUD_URL="http://localhost:50051" TITAN_STORE_ID="store-b" \
pnpm tauri dev

# Terminal B1
TITAN_DEVICE_ID="term-b1" TITAN_SYNC_MODE="secondary" \
TITAN_HUB_URL="ws://localhost:8081/sync" \
pnpm tauri dev
```

### Verify
- [ ] Both hubs connect to cloud independently
- [ ] Sales from Store A appear in cloud with `store_id='store-a'`
- [ ] Sales from Store B appear in cloud with `store_id='store-b'`
- [ ] Stores don't see each other's local data (isolation)

---

## Verification Summary

| Scenario | Status | Notes |
|----------|--------|-------|
| Single POS | ⬜ | |
| Auto-Election (2 POS) | ⬜ | |
| Failover | ⬜ | |
| Dedicated Server | ⬜ | |
| Cloud Sync | ⬜ | |
| Multi-Store | ⬜ | |

---

## Cleanup

```bash
# Stop all docker services
docker compose --profile cloud down

# Remove docker volumes (full reset)
docker compose --profile cloud down -v

# Kill any running Tauri processes
pkill -f titan-desktop || true

# Reset local database
rm -f data/titan.db data/store_aggregates.db
cargo run -p titan-db --bin seed
```

---

## Quick Troubleshooting

| Issue | Solution |
|-------|----------|
| "Port already in use" | `lsof -ti :8080 \| xargs kill -9` |
| "Connection refused" | Check hub is running, verify URL/port |
| "Database locked" | Only run one instance per data directory |
| "Cloud auth failed" | Verify store exists in PostgreSQL |
| "Docker unhealthy" | `docker compose --profile cloud restart` |

---

*Checklist Version: 1.0.0 | Last Updated: 2026-02-01*
