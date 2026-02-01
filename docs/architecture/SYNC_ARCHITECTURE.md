# Titan POS: Multi-Store Sync Architecture

> **Status**: PROPOSAL - Awaiting Review  
> **Version**: 0.2.0 "Sync Agent"  
> **Last Updated**: February 1, 2026

---

## Executive Summary

This document describes the **complete sync architecture** for Titan POS, addressing:

1. **Single Store, Multiple Counters** - How 10 POS terminals in one Target store coordinate
2. **Multi-Store, Same Tenant** - How 1,900 Target stores across the USA sync
3. **The Sync Agent** - The component that bridges local SQLite with central infrastructure

---

## 1. Understanding the Data Hierarchy

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              DATA HIERARCHY                                      │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  ┌─────────────────────────────────────────────────────────────────────────┐    │
│  │                         TENANT (Target Corporation)                      │    │
│  │  tenant_id: "target-corp-uuid"                                          │    │
│  │  • Company-wide settings (currency, logo, fiscal config)                │    │
│  │  • Master product catalog (optional - stores can override)              │    │
│  │  • Aggregated analytics and reporting                                   │    │
│  └────────────────────────────────────┬────────────────────────────────────┘    │
│                                       │                                          │
│           ┌───────────────────────────┼───────────────────────────┐             │
│           │                           │                           │             │
│           ▼                           ▼                           ▼             │
│  ┌─────────────────┐         ┌─────────────────┐         ┌─────────────────┐    │
│  │  STORE (LA)     │         │  STORE (NYC)    │         │  STORE (Chicago)│    │
│  │  store_id: uuid │         │  store_id: uuid │         │  store_id: uuid │    │
│  │                 │         │                 │         │                 │    │
│  │ • Local prices  │         │ • Local prices  │         │ • Local prices  │    │
│  │ • Local tax     │         │ • Local tax     │         │ • Local tax     │    │
│  │ • Inventory     │         │ • Inventory     │         │ • Inventory     │    │
│  │ • Store users   │         │ • Store users   │         │ • Store users   │    │
│  └────────┬────────┘         └────────┬────────┘         └────────┬────────┘    │
│           │                           │                           │             │
│     ┌─────┴─────┐               ┌─────┴─────┐               ┌─────┴─────┐       │
│     │           │               │           │               │           │       │
│     ▼           ▼               ▼           ▼               ▼           ▼       │
│  ┌─────┐     ┌─────┐         ┌─────┐     ┌─────┐         ┌─────┐     ┌─────┐   │
│  │POS-1│     │POS-2│         │POS-1│     │POS-2│         │POS-1│     │POS-2│   │
│  │     │ ... │     │         │     │ ... │     │         │     │ ... │     │   │
│  │SQLite     │SQLite         │SQLite     │SQLite         │SQLite     │SQLite   │
│  └─────┘     └─────┘         └─────┘     └─────┘         └─────┘     └─────┘   │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### Key Insight: Three-Level Hierarchy

| Level | Scope | Database | Examples |
|-------|-------|----------|----------|
| **Tenant** | Corporation | PostgreSQL (Cloud) | Target Corp, Walmart Inc |
| **Store** | Physical Location | PostgreSQL (via Store Server OR Cloud) | Target #1234 Los Angeles |
| **Device** | Checkout Counter | SQLite (Local) | POS-001 at Store #1234 |

---

## 2. What Each SQLite Database Contains

**Current v0.1**: Each POS computer has its own SQLite with:
- ✅ Full product catalog (for offline search)
- ✅ Local sales created on this device
- ✅ Sync outbox (pending uploads)
- ✅ Device configuration

**Key Clarification**: The SQLite database is **per-device**, not per-store.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     POS-001 SQLite Database                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  config                                                              │   │
│  │  ├── tenant_id    = "target-corp-uuid"                              │   │
│  │  ├── store_id     = "store-1234-uuid"      ◄── NEW: Store identity  │   │
│  │  ├── device_id    = "POS-001"                                       │   │
│  │  └── sync_server  = "wss://store-1234.titan.local:8443"            │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  products (REPLICATED from store/cloud)                              │   │
│  │  • Full catalog for offline search                                   │   │
│  │  • Read-only on POS, updated via sync                                │   │
│  │  • ~50,000 products × ~500 bytes ≈ 25 MB                            │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  sales, sale_items, payments (OWNED by this device)                  │   │
│  │  • Created locally, synced to store server                          │   │
│  │  • Never modified by other devices                                   │   │
│  │  • 90-day rolling retention                                          │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  sync_outbox (QUEUE for uploads)                                     │   │
│  │  • Pending changes to sync                                           │   │
│  │  • Processed by Sync Agent                                           │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  inventory_deltas (NEW: CRDT operation log)                          │   │
│  │  • "Sold 3 units of SKU-123" (not "stock is now 7")                 │   │
│  │  • Synced and merged at store level                                  │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Sync Architecture Options

### Option A: Direct-to-Cloud (Simple, Higher Latency)

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                     OPTION A: DIRECT-TO-CLOUD                                │
│                     (All POS devices sync directly to cloud)                 │
└──────────────────────────────────────────────────────────────────────────────┘

Store #1234                                              Cloud
┌─────────────────────────────────────┐                 ┌─────────────────────┐
│                                     │                 │                     │
│  ┌───────┐  ┌───────┐  ┌───────┐   │  Internet       │  ┌───────────────┐  │
│  │POS-001│  │POS-002│  │POS-010│   │  ─────────────► │  │  API Gateway  │  │
│  │SQLite │  │SQLite │  │SQLite │   │  WebSocket      │  └───────┬───────┘  │
│  └───┬───┘  └───┬───┘  └───┬───┘   │  + Protobuf     │          │          │
│      │          │          │       │                 │          ▼          │
│      └──────────┴──────────┘       │                 │  ┌───────────────┐  │
│               │                     │                 │  │  PostgreSQL   │  │
│               │                     │                 │  │  (per-tenant) │  │
│               └─────────────────────┼─────────────────┤  └───────────────┘  │
│                                     │                 │                     │
└─────────────────────────────────────┘                 └─────────────────────┘

Pros:
  ✓ Simpler architecture (no store server)
  ✓ Lower infrastructure cost per store
  ✓ Centralized conflict resolution

Cons:
  ✗ Higher latency for inventory updates between POS devices
  ✗ Internet dependency for real-time inventory
  ✗ All traffic goes over WAN (bandwidth cost)
  ✗ 10 WebSocket connections per store × 1,900 stores = 19,000 connections
```

### Option B: Store Server Hub (Recommended for Enterprise)

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                     OPTION B: STORE SERVER HUB                               │
│                     (POS syncs to local server, server syncs to cloud)       │
└──────────────────────────────────────────────────────────────────────────────┘

Store #1234                                              Cloud
┌──────────────────────────────────────────────┐        ┌─────────────────────┐
│                                              │        │                     │
│  ┌───────┐  ┌───────┐  ┌───────┐            │        │  ┌───────────────┐  │
│  │POS-001│  │POS-002│  │POS-010│            │        │  │  API Gateway  │  │
│  │SQLite │  │SQLite │  │SQLite │            │        │  └───────┬───────┘  │
│  └───┬───┘  └───┬───┘  └───┬───┘            │        │          │          │
│      │          │          │                │        │          ▼          │
│      │    LAN (1-5ms)      │                │        │  ┌───────────────┐  │
│      └──────────┼──────────┘                │        │  │  PostgreSQL   │  │
│                 │                           │        │  │  (per-tenant) │  │
│                 ▼                           │        │  └───────────────┘  │
│  ┌───────────────────────────────────────┐  │        │         ▲          │
│  │         STORE SERVER                   │  │        │         │          │
│  │  ┌─────────────┐  ┌─────────────────┐  │  │ WAN    │         │          │
│  │  │ PostgreSQL  │  │   Sync Agent    │──┼──┼────────┼─────────┘          │
│  │  │  (Store)    │  │ (Cloud Uplink)  │  │  │        │                     │
│  │  └─────────────┘  └─────────────────┘  │  │        │                     │
│  │         ▲                              │  │        │                     │
│  │         │                              │  │        │                     │
│  │  ┌──────┴────────┐                     │  │        │                     │
│  │  │  WebSocket    │                     │  │        │                     │
│  │  │  Server       │                     │  │        │                     │
│  │  │ (POS Ingest)  │                     │  │        │                     │
│  │  └───────────────┘                     │  │        │                     │
│  └───────────────────────────────────────┘  │        │                     │
│                                              │        │                     │
└──────────────────────────────────────────────┘        └─────────────────────┘

Pros:
  ✓ Sub-5ms inventory sync between POS devices in same store
  ✓ Store operates fully if internet is down
  ✓ Only 1 WAN connection per store (not 10)
  ✓ Can run reports locally
  ✓ Supports strict inventory (real-time stock checks)

Cons:
  ✗ Requires hardware at each store
  ✗ More complex deployment
  ✗ Store server maintenance
```

### Option C: Hybrid (Cloud-First with Optional Store Server)

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                     OPTION C: HYBRID                                          │
│                     (Small stores → cloud, Large stores → local server)       │
└──────────────────────────────────────────────────────────────────────────────┘

Small Store (1-3 POS)              Large Store (10+ POS)
┌──────────────────────┐           ┌──────────────────────────────┐
│                      │           │                              │
│  ┌───────┐           │           │  ┌───────┐     ┌───────┐    │
│  │POS-001│───────────┼───────►   │  │POS-001│ ─┬─ │POS-010│    │
│  └───────┘           │   Cloud   │  └───────┘  │  └───────┘    │
│                      │           │             │                │
└──────────────────────┘           │        ┌────▼────┐          │
                                   │        │ Store   │────► Cloud
                                   │        │ Server  │          │
                                   │        └─────────┘          │
                                   └──────────────────────────────┘
```

---

## 4. Recommended Architecture: Option B (Store Server Hub)

For a retailer like Target with:
- 1,900+ stores
- 10-30 checkout lanes per store
- Strict inventory requirements
- High transaction volume

**Option B (Store Server Hub)** is the right choice.

---

## 5. Detailed Component Design

### 5.1 Store Server Components

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          STORE SERVER                                        │
│                    (Runs on a server/PC in the back office)                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                    STORE SYNC SERVICE (Rust)                           │ │
│  │                                                                        │ │
│  │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────┐│ │
│  │  │ WebSocket       │  │ Inventory       │  │ Cloud Uplink            ││ │
│  │  │ Server          │  │ Aggregator      │  │ Agent                   ││ │
│  │  │                 │  │                 │  │                         ││ │
│  │  │ • Accept POS    │  │ • Merge deltas  │  │ • Batch uploads         ││ │
│  │  │   connections   │  │ • Calculate     │  │ • Handle conflicts      ││ │
│  │  │ • Authenticate  │  │   real stock    │  │ • Download updates      ││ │
│  │  │ • Route msgs    │  │ • Broadcast     │  │ • Apply tenant changes  ││ │
│  │  └────────┬────────┘  └────────┬────────┘  └────────────┬────────────┘│ │
│  │           │                    │                        │              │ │
│  │           └────────────────────┼────────────────────────┘              │ │
│  │                                │                                       │ │
│  │                                ▼                                       │ │
│  │           ┌─────────────────────────────────────────────────┐         │ │
│  │           │              PostgreSQL (Store DB)               │         │ │
│  │           │                                                  │         │ │
│  │           │  • products (store-level catalog + prices)      │         │ │
│  │           │  • inventory (real-time stock levels)           │         │ │
│  │           │  • sales (aggregated from all POS)             │         │ │
│  │           │  • users (cashiers for this store)              │         │ │
│  │           │  • sync_state (per-device sync cursors)         │         │ │
│  │           │                                                  │         │ │
│  │           └─────────────────────────────────────────────────┘         │ │
│  │                                                                        │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.2 POS Sync Agent Design

The Sync Agent runs as a **background thread** in the Tauri application:

```rust
/// Sync Agent Architecture (titan-sync crate)
/// 
/// The Sync Agent is responsible for:
/// 1. Uploading local changes to the Store Server (or Cloud)
/// 2. Downloading updates (products, prices, inventory)
/// 3. Maintaining sync cursors for incremental sync
/// 
/// ```
/// ┌─────────────────────────────────────────────────────────────────────┐
/// │                        SYNC AGENT                                    │
/// │                   (Background Tokio Task)                            │
/// ├─────────────────────────────────────────────────────────────────────┤
/// │                                                                      │
/// │  ┌─────────────┐    ┌─────────────┐    ┌─────────────────────────┐  │
/// │  │  Outbox     │    │  WebSocket  │    │  Inbound Processor      │  │
/// │  │  Processor  │    │  Client     │    │                         │  │
/// │  │             │    │             │    │  • Product updates      │  │
/// │  │  Reads      │───►│  Send/Recv  │───►│  • Price changes        │  │
/// │  │  sync_outbox│    │  Protobuf   │    │  • Inventory deltas     │  │
/// │  │  Batches    │    │  Messages   │    │  • Config updates       │  │
/// │  │  Uploads    │    │             │    │                         │  │
/// │  └─────────────┘    └─────────────┘    └─────────────────────────┘  │
/// │         │                 ▲                       │                  │
/// │         │                 │                       │                  │
/// │         ▼                 │                       ▼                  │
/// │  ┌─────────────────────────────────────────────────────────────┐    │
/// │  │                    SQLite Database                           │    │
/// │  │  • Read from sync_outbox (pending uploads)                  │    │
/// │  │  • Write to products (downloaded updates)                   │    │
/// │  │  • Update sync cursors                                      │    │
/// │  └─────────────────────────────────────────────────────────────┘    │
/// │                                                                      │
/// └─────────────────────────────────────────────────────────────────────┘
/// ```

pub struct SyncAgent {
    /// WebSocket connection to Store Server or Cloud
    connection: Option<WebSocketConnection>,
    
    /// Local database handle
    db: Database,
    
    /// Configuration
    config: SyncConfig,
    
    /// Sync state (cursors, last sync times)
    state: SyncState,
}

pub struct SyncConfig {
    /// WebSocket endpoint (e.g., "wss://store-server:8443/sync")
    endpoint: String,
    
    /// Device authentication token
    device_token: String,
    
    /// Batch size for outbox processing
    batch_size: usize,  // Default: 50
    
    /// Sync interval when idle
    idle_interval: Duration,  // Default: 30 seconds
    
    /// Retry backoff configuration
    retry_config: RetryConfig,
}
```

### 5.3 Sync Protocol Messages

```protobuf
// proto/sync.proto

syntax = "proto3";
package titan.sync;

// ============================================================================
// Handshake & Authentication
// ============================================================================

message DeviceHandshake {
    string tenant_id = 1;
    string store_id = 2;
    string device_id = 3;
    string device_token = 4;
    int64 schema_version = 5;
    int64 last_sync_cursor = 6;  // Resume from where we left off
}

message HandshakeResponse {
    bool success = 1;
    string error_message = 2;
    int64 server_time = 3;
}

// ============================================================================
// Outbound (POS → Server)
// ============================================================================

message SyncBatch {
    repeated SyncEntry entries = 1;
}

message SyncEntry {
    string entity_type = 1;  // "SALE", "PAYMENT", "INVENTORY_DELTA"
    string entity_id = 2;
    bytes payload = 3;       // JSON or Protobuf
    int64 created_at = 4;
    int64 device_sequence = 5;  // Monotonic sequence per device
}

message SyncAck {
    repeated string synced_ids = 1;  // IDs that were successfully processed
    repeated SyncError errors = 2;   // IDs that failed
    int64 server_cursor = 3;         // New cursor to store
}

message SyncError {
    string entity_id = 1;
    string error_code = 2;
    string error_message = 3;
    bool retryable = 4;
}

// ============================================================================
// Inbound (Server → POS)
// ============================================================================

message ServerPush {
    oneof payload {
        ProductUpdate product_update = 1;
        InventoryUpdate inventory_update = 2;
        PriceUpdate price_update = 3;
        ConfigUpdate config_update = 4;
    }
}

message ProductUpdate {
    string product_id = 1;
    string sku = 2;
    string name = 3;
    int64 price_cents = 4;
    int32 tax_rate_bps = 5;
    bool is_active = 6;
    // ... other fields
}

message InventoryUpdate {
    string product_id = 1;
    int64 current_stock = 2;  // Absolute value (after CRDT merge)
    int64 sync_version = 3;
}
```

---

## 6. How Counters Coordinate Within a Store

### Real-Time Inventory Flow (Option B Architecture)

```
┌──────────────────────────────────────────────────────────────────────────────┐
│            SCENARIO: Customer buys last Coke at POS-001                      │
└──────────────────────────────────────────────────────────────────────────────┘

Timeline:
─────────────────────────────────────────────────────────────────────────────►

T+0ms: Sale completes on POS-001
       ┌─────────────────────────────────────────────────────────────────────┐
       │  POS-001 SQLite:                                                    │
       │  INSERT INTO inventory_deltas (product_id, delta) VALUES ('coke', -1)
       │  INSERT INTO sync_outbox (entity_type, entity_id, payload)          │
       │  COMMIT                                                              │
       └─────────────────────────────────────────────────────────────────────┘

T+5ms: Sync Agent reads outbox, sends to Store Server
       ┌─────────────────────────────────────────────────────────────────────┐
       │  WebSocket Message: SyncEntry { type: "INVENTORY_DELTA", delta: -1 }│
       └─────────────────────────────────────────────────────────────────────┘

T+10ms: Store Server processes delta
       ┌─────────────────────────────────────────────────────────────────────┐
       │  Store PostgreSQL:                                                   │
       │  UPDATE inventory                                                    │
       │  SET current_stock = current_stock - 1,                              │
       │      sync_version = sync_version + 1                                 │
       │  WHERE product_id = 'coke'                                          │
       │                                                                      │
       │  Result: current_stock = 0                                          │
       └─────────────────────────────────────────────────────────────────────┘

T+15ms: Store Server broadcasts to ALL connected POS devices
       ┌─────────────────────────────────────────────────────────────────────┐
       │  ServerPush { InventoryUpdate { product_id: 'coke', stock: 0 } }    │
       │                                                                      │
       │  Recipients: POS-001, POS-002, POS-003, ... POS-010                 │
       └─────────────────────────────────────────────────────────────────────┘

T+20ms: All POS devices update local cache
       ┌─────────────────────────────────────────────────────────────────────┐
       │  POS-002 SQLite:                                                    │
       │  UPDATE products SET current_stock = 0 WHERE id = 'coke'            │
       │                                                                      │
       │  UI updates: "Coca-Cola" shows "Out of Stock" badge                 │
       └─────────────────────────────────────────────────────────────────────┘

Total latency: ~20ms (LAN speed)
```

### Conflict Resolution with CRDT

```
┌──────────────────────────────────────────────────────────────────────────────┐
│            SCENARIO: Two cashiers sell the "last" Coke simultaneously        │
└──────────────────────────────────────────────────────────────────────────────┘

Initial State: Store Server inventory.current_stock = 1

T+0ms:  POS-001 sells 1 Coke (locally: stock goes from 1 to 0)
T+0ms:  POS-002 sells 1 Coke (locally: stock goes from 1 to 0)  ◄── CONFLICT!

T+5ms:  POS-001 sends: InventoryDelta { product: 'coke', delta: -1 }
T+6ms:  POS-002 sends: InventoryDelta { product: 'coke', delta: -1 }

T+10ms: Store Server receives POS-001's delta
        ┌────────────────────────────────────────────────────────────────────┐
        │  current_stock = 1 + (-1) = 0                                      │
        └────────────────────────────────────────────────────────────────────┘

T+11ms: Store Server receives POS-002's delta
        ┌────────────────────────────────────────────────────────────────────┐
        │  current_stock = 0 + (-1) = -1  ◄── Negative stock!                │
        │                                                                     │
        │  CRDT guarantees: Both deltas are applied, order doesn't matter    │
        │  Mathematical result: 1 - 1 - 1 = -1                               │
        └────────────────────────────────────────────────────────────────────┘

T+15ms: Store Server broadcasts: InventoryUpdate { stock: -1 }

T+20ms: All POS devices see stock = -1
        UI shows: "Coca-Cola: -1 (Back Order)"

Result:
  ✓ No data loss (both sales recorded)
  ✓ Deterministic outcome (same result regardless of message order)
  ✓ Negative stock triggers alert for reorder
```

---

## 7. Multi-Store Cloud Sync

### Store-to-Cloud Data Flow

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                     CLOUD ARCHITECTURE                                        │
└──────────────────────────────────────────────────────────────────────────────┘

                    ┌─────────────────────────────────────────┐
                    │           AWS / GCP / Azure             │
                    │                                         │
                    │  ┌───────────────────────────────────┐  │
                    │  │         API Gateway               │  │
                    │  │  (WebSocket + REST)               │  │
                    │  └──────────────┬────────────────────┘  │
                    │                 │                       │
                    │    ┌────────────┼────────────┐          │
                    │    │            │            │          │
                    │    ▼            ▼            ▼          │
                    │  ┌────┐      ┌────┐      ┌────┐        │
                    │  │Svc1│      │Svc2│      │Svc3│        │ ◄── Ingest Services
                    │  └─┬──┘      └─┬──┘      └─┬──┘        │     (Horizontal Scale)
                    │    │           │           │            │
                    │    └───────────┼───────────┘            │
                    │                │                        │
                    │                ▼                        │
                    │  ┌────────────────────────────────────┐ │
                    │  │         Apache Kafka               │ │ ◄── Event Stream
                    │  │  (Topics per tenant/store)         │ │
                    │  └────────────┬───────────────────────┘ │
                    │               │                         │
                    │    ┌──────────┼──────────┐              │
                    │    │          │          │              │
                    │    ▼          ▼          ▼              │
                    │  ┌────┐    ┌────┐    ┌────────────────┐ │
                    │  │CRDT│    │Aggr│    │   Analytics    │ │
                    │  │Mrgr│    │ega │    │   Pipeline     │ │
                    │  └─┬──┘    └─┬──┘    └────────────────┘ │
                    │    │         │                          │
                    │    └─────┬───┘                          │
                    │          │                              │
                    │          ▼                              │
                    │  ┌────────────────────────────────────┐ │
                    │  │    PostgreSQL (per-tenant)         │ │
                    │  │                                    │ │
                    │  │  Schema: titan_target_corp         │ │
                    │  │  ├── stores (1,900 rows)          │ │
                    │  │  ├── products (50,000 rows)       │ │
                    │  │  ├── inventory (per-store-product)│ │
                    │  │  └── sales (partitioned by date)  │ │
                    │  └────────────────────────────────────┘ │
                    │                                         │
                    └─────────────────────────────────────────┘
                                      ▲
                                      │
                    ┌─────────────────┼─────────────────┐
                    │                 │                 │
            ┌───────┴───────┐ ┌───────┴───────┐ ┌───────┴───────┐
            │  Store #1234  │ │  Store #5678  │ │  Store #9999  │
            │  Los Angeles  │ │  New York     │ │  Chicago      │
            │               │ │               │ │               │
            │  Store Server │ │  Store Server │ │  Store Server │
            └───────────────┘ └───────────────┘ └───────────────┘
```

### What Syncs Store → Cloud?

| Data Type | Sync Direction | Frequency | Use Case |
|-----------|----------------|-----------|----------|
| Sales | Store → Cloud | Real-time (batched) | Revenue reporting, tax compliance |
| Payments | Store → Cloud | Real-time | Financial reconciliation |
| Inventory Deltas | Store → Cloud | Real-time | Cross-store visibility, reorder |
| Product Catalog | Cloud → Store | On-demand | Corporate updates prices |
| Config Changes | Cloud → Store | On-change | New tax rates, promotions |
| User Mgmt | Cloud → Store | On-change | Add/remove cashiers |

### What Syncs Between Stores?

**Generally, stores don't sync directly with each other.** All cross-store data flows through the cloud:

```
Store A ──► Cloud ──► Store B
           (Central Hub)
```

Exception: **Stock Transfer** (when implemented):
- Store A ships 100 units to Store B
- Store A: InventoryDelta -100
- Store B: InventoryDelta +100 (after physical receipt)
- Both sync to cloud for auditing

---

## 8. Database Schema Changes for Multi-Store

### New `store_id` Column

```sql
-- Migration: 003_add_store_id.sql

-- Add store_id to products (prices can vary by store)
ALTER TABLE products ADD COLUMN store_id TEXT 
    DEFAULT '00000000-0000-0000-0000-000000000001';

-- Add store_id to sales
ALTER TABLE sales ADD COLUMN store_id TEXT 
    DEFAULT '00000000-0000-0000-0000-000000000001';

-- Add store_id to config
INSERT OR REPLACE INTO config (key, value) VALUES 
    ('store_id', '00000000-0000-0000-0000-000000000001');

-- New table: Inventory deltas (CRDT operation log)
CREATE TABLE IF NOT EXISTS inventory_deltas (
    id TEXT PRIMARY KEY NOT NULL,
    tenant_id TEXT NOT NULL,
    store_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    product_id TEXT NOT NULL,
    
    -- The delta value (positive = received, negative = sold)
    delta INTEGER NOT NULL,
    
    -- Reason for the change
    reason TEXT NOT NULL,  -- 'SALE', 'VOID', 'ADJUSTMENT', 'TRANSFER_IN', 'TRANSFER_OUT'
    
    -- Reference to related entity (e.g., sale_id)
    reference_id TEXT,
    
    -- Timestamps
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    synced_at TEXT,  -- NULL = pending sync
    
    -- Device-local sequence for ordering
    device_sequence INTEGER NOT NULL
);

-- Index for pending sync
CREATE INDEX idx_inventory_deltas_pending 
    ON inventory_deltas(synced_at) WHERE synced_at IS NULL;

-- New table: Sync cursors (track where we are in sync)
CREATE TABLE IF NOT EXISTS sync_cursors (
    stream_name TEXT PRIMARY KEY NOT NULL,  -- 'products', 'inventory', 'config'
    cursor_value TEXT NOT NULL,             -- Server's cursor/timestamp
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

---

## 9. Implementation Roadmap

### Phase 1: titan-sync Crate Foundation (v0.2)

```
crates/
└── titan-sync/
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        ├── agent.rs        # SyncAgent struct
        ├── outbox.rs       # Outbox processor
        ├── transport.rs    # WebSocket client
        ├── protocol.rs     # Protobuf messages
        └── crdt.rs         # Delta-state CRDT
```

**Deliverables:**
- [ ] WebSocket client with reconnection
- [ ] Outbox processor (read, batch, send)
- [ ] Sync acknowledgment handling
- [ ] Basic inbound product updates

### Phase 2: Store Server (v0.3)

```
apps/
└── store-server/           # New Rust service
    ├── Cargo.toml
    └── src/
        ├── main.rs
        ├── websocket.rs    # WebSocket server
        ├── inventory.rs    # CRDT aggregation
        └── uplink.rs       # Cloud connection
```

**Deliverables:**
- [ ] WebSocket server accepting POS connections
- [ ] PostgreSQL store database
- [ ] Real-time inventory aggregation
- [ ] Broadcast inventory changes to connected POS

### Phase 3: Cloud Infrastructure (v0.4)

**Deliverables:**
- [ ] API Gateway setup
- [ ] Ingest service (receive from store servers)
- [ ] Multi-tenant PostgreSQL schema
- [ ] Kafka event streaming

### Phase 4: Full Sync Loop (v0.5)

**Deliverables:**
- [ ] Product catalog push from cloud → stores → POS
- [ ] Price updates propagation
- [ ] Cross-store inventory visibility
- [ ] Store-to-store transfer workflow

---

## 10. Decision Points for You

Before proceeding, please confirm:

### Q1: Store Server vs Direct-to-Cloud

| Option | Best For | Our Choice |
|--------|----------|------------|
| A: Direct-to-Cloud | Small retailers (1-5 POS) | |
| B: Store Server Hub | Enterprise (10+ POS) | ⭐ Recommended |
| C: Hybrid | Mixed deployment | |

**Your Decision**: _____________

### Q2: Store Server Hardware

| Option | Description |
|--------|-------------|
| A: Dedicated PC | Small form-factor PC in back office |
| B: Raspberry Pi | Low cost, ARM-based |
| C: Cloud VM (per-store) | Virtual private server in nearby region |
| D: One POS acts as server | Designate one checkout as "master" |

**My Recommendation**: Option A or D (depending on budget)

**Your Decision**: _____________

### Q3: Sync Protocol

| Option | Description |
|--------|-------------|
| A: WebSocket + JSON | Simple, human-readable, larger payloads |
| B: WebSocket + Protobuf | Binary, typed, smaller payloads |
| C: gRPC | Full RPC framework, bidirectional streaming |

**My Recommendation**: Option B (WebSocket + Protobuf)

**Your Decision**: _____________

### Q4: When Does POS Talk to Cloud Directly?

| Scenario | Via Store Server | Direct to Cloud |
|----------|------------------|-----------------|
| Sales sync | ✓ | |
| Inventory updates | ✓ | |
| Product catalog | ✓ | |
| Licensing/activation | | ✓ |
| Software updates | | ✓ |
| Telemetry/analytics | | ✓ (optional) |

**Your Decision**: Confirm or modify this split

---

## Summary

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          TITAN POS SYNC ARCHITECTURE                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  Level 1: DEVICE (SQLite)                                                   │
│  • Each checkout counter has its own SQLite database                        │
│  • Owns: Local sales, payments, sync outbox                                 │
│  • Caches: Products (from store), inventory (real-time updates)             │
│                                                                              │
│  Level 2: STORE (PostgreSQL on Store Server)                                │
│  • One server per physical store location                                   │
│  • Owns: Store-level inventory, aggregated sales, cashier users             │
│  • Real-time sync: Sub-20ms inventory updates across all POS                │
│                                                                              │
│  Level 3: CLOUD (PostgreSQL + Kafka)                                        │
│  • Multi-tenant architecture                                                │
│  • Owns: Master product catalog, cross-store analytics, configuration      │
│  • Batch sync: Store servers upload periodically                            │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

*Document maintained by: AI Architect*  
*Next review: After decision confirmation*
