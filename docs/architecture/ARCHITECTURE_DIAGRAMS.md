# Titan POS: System Architecture Diagrams

> **Version**: 0.1.0  
> **Last Updated**: January 31, 2026

---

## 1. High-Level System Overview

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              TITAN POS ECOSYSTEM                                │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  ┌─────────────────────────────────────────┐    ┌─────────────────────────────┐ │
│  │         ZONE A: THE EDGE                │    │    ZONE C: THE CLOUD        │ │
│  │         (In-Store POS)                  │    │    (Sync & Analytics)       │ │
│  │                                         │    │                             │ │
│  │  ┌───────────────────────────────────┐  │    │  ┌───────────────────────┐  │ │
│  │  │         Tauri Application         │  │    │  │   Ingestion Service   │  │ │
│  │  │  ┌─────────────────────────────┐  │  │    │  │       (Rust)          │  │ │
│  │  │  │     SolidJS UI Layer        │  │  │    │  └───────────┬───────────┘  │ │
│  │  │  │  • Omni-Search Bar          │  │  │    │              │              │ │
│  │  │  │  • Cart Display             │  │  │    │  ┌───────────▼───────────┐  │ │
│  │  │  │  • Tender Screen            │  │  │    │  │   Conflict Resolver   │  │ │
│  │  │  │  • Receipt View             │  │  │    │  │   (CRDT Merge)        │  │ │
│  │  │  └──────────┬──────────────────┘  │  │    │  └───────────┬───────────┘  │ │
│  │  │             │ Tauri Commands      │  │    │              │              │ │
│  │  │  ┌──────────▼──────────────────┐  │  │    │  ┌───────────▼───────────┐  │ │
│  │  │  │     Rust Core Engine        │  │  │    │  │     PostgreSQL        │  │ │
│  │  │  │  • Cart Logic               │  │  │    │  │  (Partitioned by      │  │ │
│  │  │  │  • Tax Calculator           │  │◄─┼────┼──┤   tenant_id)          │  │ │
│  │  │  │  • Transaction Manager      │  │  │    │  │                       │  │ │
│  │  │  └──────────┬──────────────────┘  │  │    │  └───────────────────────┘  │ │
│  │  │             │                     │  │    │                             │ │
│  │  │  ┌──────────▼──────────────────┐  │  │    │  ┌───────────────────────┐  │ │
│  │  │  │     SQLite (Local DB)       │  │  │    │  │   Async Workers       │  │ │
│  │  │  │  • Products (FTS)           │  │  │    │  │  • Email Receipts     │  │ │
│  │  │  │  • Sales                    │  │  │    │  │  • Webhooks           │  │ │
│  │  │  │  • Payments                 │  │  │    │  │  • Low Stock Alerts   │  │ │
│  │  │  │  • Sync Outbox              │  │  │    │  └───────────────────────┘  │ │
│  │  │  └──────────┬──────────────────┘  │  │    │                             │ │
│  │  │             │                     │  │    └─────────────────────────────┘ │
│  │  │  ┌──────────▼──────────────────┐  │  │                                    │
│  │  │  │     Sync Agent              │  │  │                                    │
│  │  │  │  (Background Thread)        │──┼──┼───────► WebSocket + Protobuf       │
│  │  │  └─────────────────────────────┘  │  │                                    │
│  │  └───────────────────────────────────┘  │    ┌─────────────────────────────┐ │
│  │                                         │    │    ZONE B: THE BRIDGE       │ │
│  │  ┌───────────────────────────────────┐  │    │                             │ │
│  │  │     Hardware I/O (v0.5+)          │  │    │  • TLS Termination          │ │
│  │  │  • Receipt Printer (ESC/POS)      │  │    │  • JWT Validation           │ │
│  │  │  • Barcode Scanner (HID/Serial)   │  │    │  • Tenant Routing           │ │
│  │  │  • Cash Drawer (Pulse)            │  │    │  • Rate Limiting            │ │
│  │  │  • Card Terminal (Semi-Integrated)│  │    │                             │ │
│  │  └───────────────────────────────────┘  │    └─────────────────────────────┘ │
│  └─────────────────────────────────────────┘                                    │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Rust Crate Dependency Graph

```
                                    ┌─────────────────┐
                                    │   titan-tauri   │
                                    │  (Application)  │
                                    └────────┬────────┘
                                             │
                    ┌────────────────────────┼────────────────────────┐
                    │                        │                        │
                    ▼                        ▼                        ▼
          ┌─────────────────┐      ┌─────────────────┐      ┌─────────────────┐
          │   titan-sync    │      │    titan-db     │      │    titan-hal    │
          │  (Sync Engine)  │      │  (Persistence)  │      │   (Hardware)    │
          └────────┬────────┘      └────────┬────────┘      └────────┬────────┘
                   │                        │                        │
                   │                        │                        │
                   └────────────────────────┼────────────────────────┘
                                            │
                                            ▼
                                  ┌─────────────────┐
                                  │   titan-core    │
                                  │ (Business Logic)│
                                  │   ZERO I/O      │
                                  └─────────────────┘

Legend:
  ─────► Depends on
  
Feature Flags:
  titan-hal:    [feature = "hardware"]    - Disabled by default
  titan-fiscal: [feature = "fiscal_de"]   - Germany fiscal module
                [feature = "fiscal_it"]   - Italy fiscal module
```

---

## 3. Data Flow: Sale Transaction

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                        SALE TRANSACTION FLOW                                 │
└──────────────────────────────────────────────────────────────────────────────┘

  User Action          Frontend              Rust Core            SQLite
      │                   │                      │                   │
      │  Scan/Search      │                      │                   │
      ├──────────────────►│                      │                   │
      │                   │  search_products()   │                   │
      │                   ├─────────────────────►│                   │
      │                   │                      │  FTS Query        │
      │                   │                      ├──────────────────►│
      │                   │                      │◄─────────────────┤│
      │                   │◄─────────────────────┤                   │
      │  Display Results  │                      │                   │
      │◄──────────────────┤                      │                   │
      │                   │                      │                   │
      │  Select Item      │                      │                   │
      ├──────────────────►│                      │                   │
      │                   │  add_to_cart()       │                   │
      │                   ├─────────────────────►│                   │
      │                   │                      │  Validate Stock   │
      │                   │                      │  Calculate Tax    │
      │                   │                      │  Update Cart      │
      │                   │◄─────────────────────┤                   │
      │  Cart Updated     │                      │                   │
      │◄──────────────────┤                      │                   │
      │                   │                      │                   │
      │  Click "Pay"      │                      │                   │
      ├──────────────────►│                      │                   │
      │                   │  initiate_tender()   │                   │
      │                   ├─────────────────────►│                   │
      │                   │                      │  Lock Cart        │
      │                   │                      │  Generate Sale ID │
      │                   │◄─────────────────────┤                   │
      │  Show Tender UI   │                      │                   │
      │◄──────────────────┤                      │                   │
      │                   │                      │                   │
      │  Enter Amount     │                      │                   │
      ├──────────────────►│                      │                   │
      │                   │  process_payment()   │                   │
      │                   ├─────────────────────►│                   │
      │                   │                      │  BEGIN TRANSACTION│
      │                   │                      ├──────────────────►│
      │                   │                      │  INSERT sales     │
      │                   │                      ├──────────────────►│
      │                   │                      │  INSERT items     │
      │                   │                      ├──────────────────►│
      │                   │                      │  INSERT payment   │
      │                   │                      ├──────────────────►│
      │                   │                      │  UPDATE inventory │
      │                   │                      ├──────────────────►│
      │                   │                      │  INSERT outbox    │
      │                   │                      ├──────────────────►│
      │                   │                      │  COMMIT           │
      │                   │                      ├──────────────────►│
      │                   │◄─────────────────────┤                   │
      │  Show Receipt     │                      │                   │
      │◄──────────────────┤                      │                   │
      │                   │                      │                   │
```

---

## 4. Sync Protocol: Outbox Pattern

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                          SYNC OUTBOX PATTERN                                 │
└──────────────────────────────────────────────────────────────────────────────┘

   Local SQLite                 Sync Agent                    Cloud Server
        │                           │                              │
        │                           │  (On network available)      │
        │                           │                              │
        │  SELECT * FROM            │                              │
        │  sync_outbox              │                              │
        │  WHERE status='PENDING'   │                              │
        │  LIMIT 50                 │                              │
        │◄──────────────────────────┤                              │
        │                           │                              │
        │                           │  Serialize to Protobuf       │
        │                           │                              │
        │                           │  WebSocket SEND              │
        │                           ├─────────────────────────────►│
        │                           │                              │
        │                           │                              │ Validate JWT
        │                           │                              │ Route to Tenant
        │                           │                              │ Apply CRDT
        │                           │                              │ Store in Postgres
        │                           │                              │
        │                           │  ACK (with server_timestamp) │
        │                           │◄─────────────────────────────┤
        │                           │                              │
        │  UPDATE sync_outbox       │                              │
        │  SET status='SYNCED'      │                              │
        │◄──────────────────────────┤                              │
        │                           │                              │
        │  DELETE FROM sync_outbox  │                              │
        │  WHERE synced_at < X      │                              │
        │◄──────────────────────────┤                              │
        │                           │                              │

Error Handling:
  • Network Failure: Exponential backoff (1s, 2s, 4s, 8s... max 5min)
  • Server Error (5xx): Retry with same payload
  • Client Error (4xx): Mark as FAILED, alert user
  • Validation Error: Log and skip (data corruption)
```

---

## 5. CRDT Inventory Merge

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                    CRDT DELTA-STATE MERGE (INVENTORY)                        │
└──────────────────────────────────────────────────────────────────────────────┘

                    Server State                 
                    Stock: 10                    
                         │                       
          ┌──────────────┴──────────────┐        
          │                             │        
          ▼                             ▼        
     ┌─────────┐                   ┌─────────┐   
     │ POS A   │                   │ POS B   │   
     │ Stock:10│                   │ Stock:10│   
     └────┬────┘                   └────┬────┘   
          │                             │        
          │ Sell 3 units                │ Sell 2 units
          │ (Offline)                   │ (Offline)
          │                             │        
          ▼                             ▼        
     ┌─────────┐                   ┌─────────┐   
     │ Local:7 │                   │ Local:8 │   
     │ Delta:-3│                   │ Delta:-2│   
     └────┬────┘                   └────┬────┘   
          │                             │        
          │  Come Online                │ Come Online
          │                             │        
          └──────────────┬──────────────┘        
                         │                       
                         ▼                       
              ┌─────────────────────┐            
              │   Server Merge      │            
              │                     │            
              │  Current: 10        │            
              │  Apply: -3          │            
              │  Apply: -2          │            
              │  ─────────          │            
              │  New: 5             │            
              └─────────────────────┘            

Key Insight:
  We don't sync "Stock = 7"
  We sync "Stock Change = -3"
  This makes merge commutative and associative
```

---

## 6. State Machine: POS Flow

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                         POS STATE MACHINE (XState)                           │
└──────────────────────────────────────────────────────────────────────────────┘

                              ┌─────────────┐
                              │   LOCKED    │◄───────────────────────┐
                              │(Subscription│                        │
                              │  Expired)   │                        │
                              └──────┬──────┘                        │
                                     │ subscription_renewed          │
                                     ▼                               │
        ┌───────────────────► ┌─────────────┐ ◄──────────────────────┤
        │                     │    IDLE     │                        │
        │                     │  (Ready)    │                        │
        │                     └──────┬──────┘                        │
        │                            │                               │
        │                            │ add_item / scan               │
        │                            ▼                               │
        │                     ┌─────────────┐                        │
        │  clear_cart         │   IN_CART   │◄────────┐              │
        │                     │ (Shopping)  │         │              │
        │                     └──────┬──────┘         │              │
        │                            │                │              │
        │                            │ checkout       │ back         │
        │                            ▼                │              │
        │                     ┌─────────────┐         │              │
        └─────────────────────┤   TENDER    ├─────────┘              │
                              │ (Payment)   │                        │
                              └──────┬──────┘                        │
                                     │                               │
                                     │ payment_complete              │
                                     ▼                               │
                              ┌─────────────┐                        │
                              │  RECEIPT    │                        │
                              │ (Complete)  │                        │
                              └──────┬──────┘                        │
                                     │                               │
                                     │ new_sale / timeout            │
                                     │                               │
                                     └───────────────────────────────┘

Parallel States:
  • SYNC_STATE: idle | connecting | syncing | error
  • DRAWER_STATE: closed | open | unknown (future)
```

---

## 7. Directory Structure (Final)

```
titan-pos/
├── .github/
│   ├── workflows/
│   │   ├── ci.yml                    # Build & Test
│   │   ├── release.yml               # Build installers
│   │   └── security.yml              # Dependency audit
│   └── copilot-instructions.md       # AI coding guidelines
│
├── docs/
│   ├── architecture/
│   │   ├── ARCHITECTURE_DECISIONS.md # ADRs
│   │   ├── ARCHITECTURE_DIAGRAMS.md  # This file
│   │   └── CRATE_GUIDE.md            # Crate responsibilities
│   ├── api/
│   │   └── TAURI_COMMANDS.md         # Command reference
│   └── CONTRIBUTING.md               # Dev setup guide
│
├── crates/
│   ├── titan-core/                   # Pure business logic
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── cart.rs               # Cart operations
│   │       ├── pricing.rs            # Tax & discount calc
│   │       ├── transaction.rs        # Transaction state
│   │       ├── types.rs              # Money, Qty, etc.
│   │       └── validation.rs         # Business rules
│   │
│   ├── titan-db/                     # Database layer
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── sqlite/
│   │       │   ├── mod.rs
│   │       │   ├── connection.rs
│   │       │   └── repository.rs
│   │       ├── migrations.rs
│   │       └── models.rs
│   │
│   └── titan-sync/                   # Sync engine
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── crdt.rs               # CRDT implementations
│           ├── outbox.rs             # Outbox manager
│           └── transport.rs          # WebSocket client
│
├── apps/
│   └── desktop/                      # Tauri application
│       ├── src-tauri/
│       │   ├── Cargo.toml
│       │   ├── tauri.conf.json
│       │   ├── capabilities/
│       │   └── src/
│       │       ├── main.rs
│       │       ├── commands/         # Tauri command handlers
│       │       │   ├── mod.rs
│       │       │   ├── inventory.rs
│       │       │   └── transaction.rs
│       │       ├── state.rs          # App state management
│       │       └── error.rs          # Error types
│       │
│       ├── src/                      # SolidJS frontend
│       │   ├── index.html
│       │   ├── index.tsx
│       │   ├── App.tsx
│       │   ├── components/
│       │   │   ├── OmniSearch.tsx
│       │   │   ├── Cart.tsx
│       │   │   ├── TenderModal.tsx
│       │   │   └── Receipt.tsx
│       │   ├── machines/             # XState machines
│       │   │   └── posMachine.ts
│       │   ├── stores/               # SolidJS stores
│       │   ├── styles/
│       │   └── types/
│       │
│       ├── package.json
│       ├── tsconfig.json
│       ├── vite.config.ts
│       └── tailwind.config.js
│
├── proto/                            # Protobuf definitions
│   ├── sync.proto
│   └── messages.proto
│
├── migrations/
│   └── sqlite/
│       ├── 001_initial_schema.sql
│       └── 002_add_fts.sql
│
├── scripts/
│   ├── seed-data.ts                  # Generate test data
│   └── dev-setup.sh                  # Dev environment setup
│
├── Cargo.toml                        # Workspace root
├── rust-toolchain.toml
├── .rustfmt.toml
├── clippy.toml
├── package.json                      # pnpm workspace
├── pnpm-workspace.yaml
└── README.md
```

---

*Diagrams maintained by: AI Architect*  
*Review cycle: On major structural changes*
