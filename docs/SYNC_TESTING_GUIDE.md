# Titan POS - Sync Testing Guide

> **Complete guide for testing all synchronization scenarios in Titan POS**

This document covers how to test the three main sync architectures:
1. **Scenario A**: Dedicated Sync Server (PRIMARY mode)
2. **Scenario B**: POS as Master with Automatic Failover (AUTO mode)
3. **Scenario C**: Cloud Database Sync (PostgreSQL)

---

## Prerequisites

### Required Software
```bash
# macOS
brew install docker pnpm rust

# Verify installation
docker --version          # >= 24.0
pnpm --version            # >= 8.0
cargo --version           # >= 1.75
```

### Build the Project
```bash
cd /path/to/titan-pos

# Install frontend dependencies
cd apps/desktop && pnpm install && cd ../..

# Build all Rust crates
cargo build
```

---

## Architecture Overview

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                    Titan POS Sync Architecture                              │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                         CLOUD TIER                                   │   │
│  │  ┌─────────────────────────────────────────────────────────────┐    │   │
│  │  │  cloud-api (gRPC :50051)                                    │    │   │
│  │  │    ↓              ↓                                         │    │   │
│  │  │  PostgreSQL    Redis                                        │    │   │
│  │  │  (data)        (pub/sub)                                    │    │   │
│  │  └─────────────────────────────────────────────────────────────┘    │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                    ↑                                        │
│                          gRPC upload/download                               │
│                                    │                                        │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                        STORE TIER                                    │   │
│  │                                                                      │   │
│  │  ┌───────────────────────────────────────────────────────────────┐  │   │
│  │  │  PRIMARY Device (Store Hub)                                   │  │   │
│  │  │    - WebSocket server (:8080)                                 │  │   │
│  │  │    - Inventory aggregation                                    │  │   │
│  │  │    - Store-level database (store_aggregates.db)               │  │   │
│  │  │    - Cloud uplink (gRPC client)                               │  │   │
│  │  └───────────────────────────────────────────────────────────────┘  │   │
│  │                         ↑ WebSocket                                  │   │
│  │         ┌───────────────┼───────────────┐                           │   │
│  │         │               │               │                           │   │
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐                 │   │
│  │  │  SECONDARY   │ │  SECONDARY   │ │  SECONDARY   │                 │   │
│  │  │  Device 1    │ │  Device 2    │ │  Device N    │                 │   │
│  │  │  (titan.db)  │ │  (titan.db)  │ │  (titan.db)  │                 │   │
│  │  └──────────────┘ └──────────────┘ └──────────────┘                 │   │
│  │                                                                      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Environment Variables Reference

| Variable | Description | Default | Example |
|----------|-------------|---------|---------|
| `TITAN_DEVICE_ID` | Unique device identifier | Auto-generated UUID | `pos-register-1` |
| `TITAN_DEVICE_NAME` | Human-readable name | None | `"Register 1"` |
| `TITAN_SYNC_MODE` | Sync mode | `auto` | `primary`, `secondary`, `offline` |
| `TITAN_HUB_PORT` | WebSocket port (PRIMARY only) | `8080` | `8080` |
| `TITAN_HUB_URL` | Store Hub WebSocket URL | Auto-discovered | `ws://192.168.1.100:8080/sync` |
| `TITAN_DEVICE_PRIORITY` | Leader election priority | `50` | `100` (higher = more likely PRIMARY) |
| `TITAN_CLOUD_URL` | Cloud API gRPC endpoint | None | `http://localhost:50051` |
| `TITAN_STORE_ID` | Store identifier | None | `store-001` |
| `TITAN_TENANT_ID` | Tenant identifier | None | `tenant-acme` |
| `TITAN_API_KEY` | Store API key for cloud auth | None | `sk_store_xxx` |

---

## Scenario A: Dedicated Sync Server (PRIMARY Mode)

**Use Case**: A dedicated server machine acts as the Store Hub. All POS terminals connect to it.

### Setup

#### Terminal 1: Start Dedicated Server (PRIMARY)
```bash
cd apps/desktop

# Force PRIMARY mode on dedicated server
TITAN_DEVICE_ID="server-hub-01" \
TITAN_DEVICE_NAME="Store Hub Server" \
TITAN_SYNC_MODE="primary" \
TITAN_HUB_PORT="8080" \
TITAN_DEVICE_PRIORITY="100" \
pnpm tauri dev
```

You should see in logs:
```
INFO titan_sync::agent: Starting as PRIMARY, launching hub server port=8080
INFO titan_sync::hub: Hub server listening on ws://0.0.0.0:8080/sync
```

#### Terminal 2: Start POS Terminal 1 (SECONDARY)
```bash
cd apps/desktop

TITAN_DEVICE_ID="pos-register-1" \
TITAN_DEVICE_NAME="Register 1" \
TITAN_SYNC_MODE="secondary" \
TITAN_HUB_URL="ws://localhost:8080/sync" \
pnpm tauri dev
```

#### Terminal 3: Start POS Terminal 2 (SECONDARY)
```bash
cd apps/desktop

TITAN_DEVICE_ID="pos-register-2" \
TITAN_DEVICE_NAME="Register 2" \
TITAN_SYNC_MODE="secondary" \
TITAN_HUB_URL="ws://localhost:8080/sync" \
pnpm tauri dev
```

### Verification Checklist

- [ ] Server shows "Hub server listening" message
- [ ] Each POS shows "Connected to hub" message
- [ ] DevConsole (click "Dev" badge) shows:
  - Device ID: Configured value
  - Sync Mode: `primary` or `secondary`
  - Hub Port: `8080` (PRIMARY only)
- [ ] Create a sale on Register 1
- [ ] Register 2 should see inventory update within 1 second
- [ ] Server's `store_aggregates.db` should contain the sale

### Expected Data Flow
```text
Register 1                    Server Hub                    Register 2
    │                             │                             │
    │  ── inventory delta ──►     │                             │
    │                             │  ◄── aggregates delta ──►   │
    │                             │                             │
    │  ◄── broadcast update ──    │  ── broadcast update ──►    │
```

---

## Scenario B: POS as Master with Automatic Failover (AUTO Mode)

**Use Case**: No dedicated server. POS terminals elect a leader among themselves. If the leader goes down, another takes over automatically.

### Setup

#### Terminal 1: Start First POS (will become PRIMARY)
```bash
cd apps/desktop

TITAN_DEVICE_ID="pos-1" \
TITAN_DEVICE_NAME="Register 1" \
TITAN_SYNC_MODE="auto" \
TITAN_DEVICE_PRIORITY="80" \
pnpm tauri dev
```

Wait for election:
```
INFO titan_sync::election: No PRIMARY found, starting election
INFO titan_sync::election: Won election, becoming PRIMARY fencing_token=1
INFO titan_sync::hub: Hub server started on ws://0.0.0.0:8080/sync
```

#### Terminal 2: Start Second POS (will become SECONDARY)
```bash
cd apps/desktop

TITAN_DEVICE_ID="pos-2" \
TITAN_DEVICE_NAME="Register 2" \
TITAN_SYNC_MODE="auto" \
TITAN_DEVICE_PRIORITY="50" \
pnpm tauri dev
```

You should see:
```
INFO titan_sync::discovery: Found PRIMARY hub at 192.168.x.x:8080
INFO titan_sync::transport: Connected to hub ws://192.168.x.x:8080/sync
INFO titan_sync::agent: Operating as SECONDARY
```

### Failover Test

1. **Verify initial state**:
   - POS-1 shows "Role: PRIMARY" in DevConsole
   - POS-2 shows "Role: SECONDARY" in DevConsole

2. **Kill the PRIMARY** (Ctrl+C in Terminal 1)

3. **Watch POS-2 failover**:
   ```
   WARN titan_sync::transport: Connection lost, hub went away
   INFO titan_sync::election: PRIMARY not responding, starting election
   INFO titan_sync::election: Won election, becoming PRIMARY fencing_token=2
   INFO titan_sync::hub: Hub server started on ws://0.0.0.0:8080/sync
   ```

4. **Restart POS-1**:
   ```bash
   TITAN_DEVICE_ID="pos-1" \
   TITAN_SYNC_MODE="auto" \
   TITAN_DEVICE_PRIORITY="80" \
   pnpm tauri dev
   ```

5. **POS-1 should connect as SECONDARY** (POS-2 has higher fencing token):
   ```
   INFO titan_sync::discovery: Found PRIMARY hub at 192.168.x.x:8080 (fencing_token=2)
   INFO titan_sync::agent: Operating as SECONDARY (existing PRIMARY has higher token)
   ```

### Verification Checklist

- [ ] Higher priority device wins initial election
- [ ] Remaining device takes over within 5-10 seconds of PRIMARY failure
- [ ] New fencing token is higher than previous
- [ ] Restarted device correctly becomes SECONDARY
- [ ] No data loss during failover
- [ ] All devices eventually show consistent inventory

---

## Scenario C: Cloud Database Sync (PostgreSQL)

**Use Case**: Store Hub synchronizes data to a cloud PostgreSQL database for multi-store reporting, centralized inventory, and backup.

### Setup

#### Step 1: Start Cloud Infrastructure
```bash
cd /path/to/titan-pos

# Start PostgreSQL, Redis, and Cloud API
docker compose --profile cloud up -d

# Verify services are running
docker compose ps

# Expected output:
# NAME                STATUS          PORTS
# titan-cloud-api     Up (healthy)    0.0.0.0:50051->50051/tcp
# titan-postgres      Up (healthy)    0.0.0.0:5432->5432/tcp
# titan-redis         Up (healthy)    0.0.0.0:6379->6379/tcp
```

#### Step 2: Seed Cloud Database with Test Tenant/Store
```bash
# Connect to PostgreSQL
docker exec -it titan-postgres psql -U titan -d titan_pos

# Create test tenant and store
INSERT INTO tenants (id, name, currency) VALUES 
  ('tenant-demo', 'Demo Company', 'USD');

INSERT INTO stores (id, tenant_id, name, api_key_hash) VALUES 
  ('store-downtown', 'tenant-demo', 'Downtown Store', 
   '$argon2id$v=19$m=65536,t=3,p=4$dGVzdHNhbHQ$VmU1VzVlNlhfLW4xQ3JHWjN5TGtaMA');
-- API key: sk_store_test_123 (for dev only)

INSERT INTO store_configs (store_id, tenant_id, store_name) VALUES 
  ('store-downtown', 'tenant-demo', 'Downtown Store');

\q
```

#### Step 3: Start Store Hub with Cloud Uplink
```bash
cd apps/desktop

TITAN_DEVICE_ID="hub-downtown" \
TITAN_DEVICE_NAME="Downtown Hub" \
TITAN_SYNC_MODE="primary" \
TITAN_HUB_PORT="8080" \
TITAN_CLOUD_URL="http://localhost:50051" \
TITAN_STORE_ID="store-downtown" \
TITAN_TENANT_ID="tenant-demo" \
TITAN_API_KEY="sk_store_test_123" \
pnpm tauri dev
```

Expected logs:
```
INFO titan_sync::cloud_auth: Authenticating with cloud API
INFO titan_sync::cloud_auth: JWT token obtained, expires in 3600s
INFO titan_sync::cloud_uplink: Connected to cloud, starting sync
INFO titan_sync::cloud_uplink: Uploading 0 pending entries
```

#### Step 4: Start Secondary POS
```bash
cd apps/desktop

TITAN_DEVICE_ID="pos-downtown-1" \
TITAN_DEVICE_NAME="Register 1" \
TITAN_SYNC_MODE="secondary" \
TITAN_HUB_URL="ws://localhost:8080/sync" \
pnpm tauri dev
```

### Test Cloud Sync

1. **Make a sale on Register 1**

2. **Verify sale reaches cloud database**:
```bash
docker exec -it titan-postgres psql -U titan -d titan_pos -c "
  SELECT id, total_cents, status, created_at 
  FROM sales 
  WHERE store_id = 'store-downtown' 
  ORDER BY created_at DESC 
  LIMIT 5;
"
```

3. **Check cloud API logs**:
```bash
docker logs titan-cloud-api --tail 50
```

Expected:
```
INFO sync_service: Received UploadBatch store_id=store-downtown entities=1
INFO sync_service: Processed sale sale_id=xxx total_cents=1299
```

### Verification Checklist

- [ ] Cloud API starts without errors
- [ ] PostgreSQL migrations applied (check `docker logs titan-cloud-api`)
- [ ] Store Hub authenticates with cloud (JWT token message)
- [ ] Sales appear in PostgreSQL within upload interval (30s default)
- [ ] Inventory deltas are applied correctly
- [ ] Cloud-to-store product updates work (add product in DB, verify it appears)

---

## Scenario D: Combined Multi-Store Test

**Use Case**: Two stores, each with multiple registers, syncing to the same cloud.

### Setup

```text
                    Cloud API (:50051)
                         │
           ┌─────────────┴─────────────┐
           │                           │
    Downtown Store               Uptown Store
    (store-downtown)            (store-uptown)
           │                           │
    ┌──────┴──────┐             ┌──────┴──────┐
    │             │             │             │
  Hub        Register 1      Hub        Register 1
(:8080)      (SECONDARY)   (:8081)      (SECONDARY)
```

#### Start Downtown Store
```bash
# Terminal 1: Downtown Hub
TITAN_DEVICE_ID="hub-downtown" TITAN_SYNC_MODE="primary" TITAN_HUB_PORT="8080" \
TITAN_CLOUD_URL="http://localhost:50051" TITAN_STORE_ID="store-downtown" \
TITAN_TENANT_ID="tenant-demo" TITAN_API_KEY="sk_downtown_123" \
pnpm tauri dev

# Terminal 2: Downtown Register 1
TITAN_DEVICE_ID="pos-downtown-1" TITAN_SYNC_MODE="secondary" \
TITAN_HUB_URL="ws://localhost:8080/sync" \
pnpm tauri dev
```

#### Start Uptown Store
```bash
# Terminal 3: Uptown Hub (different port!)
TITAN_DEVICE_ID="hub-uptown" TITAN_SYNC_MODE="primary" TITAN_HUB_PORT="8081" \
TITAN_CLOUD_URL="http://localhost:50051" TITAN_STORE_ID="store-uptown" \
TITAN_TENANT_ID="tenant-demo" TITAN_API_KEY="sk_uptown_456" \
pnpm tauri dev

# Terminal 4: Uptown Register 1
TITAN_DEVICE_ID="pos-uptown-1" TITAN_SYNC_MODE="secondary" \
TITAN_HUB_URL="ws://localhost:8081/sync" \
pnpm tauri dev
```

### Cross-Store Test

1. Add product to cloud database (global product):
```sql
INSERT INTO products (id, tenant_id, sku, name, price_cents, is_active) VALUES
  ('prod-global-1', 'tenant-demo', 'PROMO-001', 'Holiday Special', 999, true);
```

2. Verify both stores receive the product (check DevConsole or search)

3. Make sales at both stores, verify cloud database has data from both

---

## Troubleshooting

### Common Issues

#### "Connection refused" to Hub
```
Cause: Hub not started or wrong port
Fix: Check TITAN_HUB_URL matches PRIMARY's TITAN_HUB_PORT
```

#### "Authentication failed" to Cloud
```
Cause: Wrong API key or store not registered
Fix: Verify store exists in cloud DB and API key matches
```

#### "Hub discovery failed" in AUTO mode
```
Cause: mDNS not working or firewall blocking UDP
Fix: Use explicit TITAN_HUB_URL or check firewall rules
```

#### "Fencing token rejected"
```
Cause: Stale PRIMARY trying to reconnect
Fix: This is expected behavior - wait for election or restart with fresh state
```

### Debug Mode

Enable verbose logging:
```bash
RUST_LOG="debug,titan_sync=trace,titan_db=debug" pnpm tauri dev
```

### Reset State

```bash
# Clear local database
rm -f data/titan.db data/store_aggregates.db

# Re-seed
cargo run -p titan-db --bin seed

# Clear cloud database
docker compose down -v
docker compose --profile cloud up -d
```

---

## Log File Locations

| Platform | Path |
|----------|------|
| macOS | `~/Library/Logs/com.titan.pos/titan-pos.log` |
| Linux | `~/.local/share/com.titan.pos/logs/titan-pos.log` |
| Windows | `%APPDATA%\com.titan.pos\logs\titan-pos.log` |

---

## Quick Reference Commands

```bash
# Start cloud infrastructure
docker compose --profile cloud up -d

# Stop everything
docker compose --profile cloud down

# View cloud logs
docker logs -f titan-cloud-api

# Connect to PostgreSQL
docker exec -it titan-postgres psql -U titan -d titan_pos

# Check pending sync entries (local SQLite)
sqlite3 data/titan.db "SELECT * FROM sync_outbox ORDER BY created_at DESC LIMIT 10;"

# Check store aggregates
sqlite3 data/store_aggregates.db "SELECT * FROM inventory_aggregates ORDER BY updated_at DESC LIMIT 10;"
```

---

## Next Steps

After verifying all scenarios work:

1. **Performance Testing**: Test with 1000+ products and high transaction volume
2. **Network Partition Testing**: Simulate network failures and verify recovery
3. **Concurrent Write Testing**: Multiple registers modifying same inventory simultaneously
4. **Long-Running Stability**: Run for 24+ hours and monitor memory/CPU

---

*Document Version: 1.0.0 | Last Updated: 2026-02-01*
