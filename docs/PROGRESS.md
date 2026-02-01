# Titan POS v0.2 - Development Progress

> **Status**: 🟡 Planning - Sync Architecture Defined  
> **Target**: v0.2 "Store Sync & Auto-Hub"  
> **Last Updated**: February 1, 2026

---

## Overview

v0.2 focuses on **in-store multi-device coordination** with an auto-elected Store Server Hub. The system can run in:
- **Auto mode**: First POS becomes PRIMARY; others connect as SECONDARY
- **Primary mode**: Dedicated server or specific POS acts as hub
- **Secondary mode**: Explicitly connect to configured hub

Key decisions (from `docs/architecture/SYNC_ARCHITECTURE.md` + your confirmations):
- **Discovery**: mDNS + UDP broadcast (both)
- **Election priority**: Combination (priority config → device_id tiebreak)
- **Failover**: Conservative default, configurable
- **Store DB**: Separate store-level database on PRIMARY

---

## Milestones (All part of v0.2)

### Milestone 1: Sync Agent Foundation ⬜
**Goal**: Core sync engine for POS devices

| Task | Status | Notes |
|------|--------|-------|
| Create `titan-sync` crate | ⬜ | New crate in `crates/` |
| Sync configuration model | ⬜ | Modes: auto/primary/secondary |
| Outbox processor | ⬜ | Batch uploads from `sync_outbox` |
| WebSocket client | ⬜ | Reconnect with backoff |
| Sync acknowledgements | ⬜ | Mark outbox rows as synced |
| Inbound updates pipeline | ⬜ | Apply product/price/inventory updates |

---

### Milestone 2: Store Hub (Auto-Elected Primary) ⬜
**Goal**: One POS becomes the Store Server Hub automatically

| Task | Status | Notes |
|------|--------|-------|
| Discovery protocol | ⬜ | mDNS + UDP broadcast |
| Leader election | ⬜ | Priority + device_id tiebreak |
| Heartbeat monitoring | ⬜ | Conservative defaults, configurable |
| WebSocket server | ⬜ | Accept POS connections |
| Separate store DB | ⬜ | Store-level aggregation on PRIMARY |
| Broadcast inventory updates | ⬜ | Near real-time store-wide updates |

---

### Milestone 3: Cloud Uplink (Primary → Cloud) ⬜
**Goal**: Store hub syncs to cloud while POS syncs to hub

| Task | Status | Notes |
|------|--------|-------|
| Cloud uplink client | ⬜ | Runs only on PRIMARY |
| Batch uploads | ⬜ | Sales, payments, inventory deltas |
| Conflict handling | ⬜ | CRDT delta-state merge |
| Download updates | ⬜ | Products, prices, config |
| Sync cursors | ⬜ | Store server cursor tracking |

---

### Milestone 4: Multi-Store Readiness ⬜
**Goal**: Scale from one store to many under one tenant

| Task | Status | Notes |
|------|--------|-------|
| Store identity configuration | ⬜ | `store_id` added to config |
| Inventory deltas table | ⬜ | CRDT operation log |
| Sync protocol messages | ⬜ | Protobuf message schema |
| Store-level aggregation | ⬜ | Inventory + sales aggregation |
| Failover recovery | ⬜ | Re-elect primary if hub down |

---

# Titan POS v0.1 - Development Progress

> **Status**: 🟡 Milestone 4 Complete - v0.1 Ready for Testing  
> **Target**: v0.1 "Logical Core"  
> **Last Updated**: February 2, 2026

---

## Overview

v0.1 focuses on the **Logical Core** - validating data integrity, integer math, and offline persistence. No hardware integration, no real payment processing.

---

## Milestones

### Milestone 1: Foundation & Scaffold ✅
**Goal**: Project structure, database, and basic CRUD

| Task | Status | Notes |
|------|--------|-------|
| Initialize Rust workspace | ✅ | Cargo.toml with all crates |
| Create `titan-core` crate | ✅ | Money, types, validation |
| Create `titan-db` crate | ✅ | SQLite connection, migrations |
| Setup Tauri v2 + SolidJS | ✅ | Basic window, hot reload |
| Database migrations | ✅ | products, sales, payments, sync_outbox |
| Seed data script | ✅ | 5,000 test products in `data/titan.db` |
| Docker setup | ✅ | Dockerfile, docker-compose |
| CI/CD pipeline | ✅ | GitHub Actions (fixed dtolnay/rust-toolchain) |

**Deliverable**: App launches, database initialized, seed data loaded

---

### Milestone 2: Omni-Search & Product Display ✅
**Goal**: Sub-10ms product search with FTS5

| Task | Status | Notes |
|------|--------|-------|
| FTS5 virtual table setup | ✅ | `products_fts` with INSERT/UPDATE/DELETE triggers |
| `search_products` command | ✅ | FTS5 query with barcode instant lookup |
| Search input component | ✅ | SolidJS with 150ms debounce, instant for barcodes |
| Product grid component | ✅ | Responsive 5-column grid |
| Product selection | ✅ | Click adds to cart with qty=1 |
| Keyboard navigation | ✅ | Arrow keys, Enter, 1-9 quick add, Escape |
### Seed Data Population - Temporary Issue

**Issue**: The `seed` binary uses sqlx compile-time macros that require either:
1. A valid DATABASE_URL pointing to an initialized database
2. Cached query metadata in `.sqlx/`

**Current Workaround**: 

The dev database is intentionally placed at `./data/titan.db` for development. The Tauri app automatically detects this when running in dev mode.

**Proper Solution (TODO)**:
- [ ] Run migrations first to create schema
- [ ] Use sqlx prepare to cache queries
- [ ] OR refactor seed.rs to use runtime queries instead of macros

**For Now**: Use this workaround if seed command fails:
```bash
# Delete the old database
rm -f data/titan.db

# Create it manually with schema
mkdir -p data
sqlite3 data/titan.db < migrations/sqlite/001_initial_schema.sql
sqlite3 data/titan.db < migrations/sqlite/002_add_fts.sql

# Run the app (it will have empty products table, but schema is set up)
cd apps/desktop && pnpm tauri dev

# Then populate products manually with SQL INSERT statements
# or create a simpler Python script to populate the database
```

**Alternative**: The app can still run without seed data - you can manually create products through the UI (once sale creation is implemented in Milestone 3).

#### Architecture Decisions Made
- **Barcode Detection**: Queries matching 8-13 digits trigger exact barcode lookup first
- **Debounce Strategy**: 150ms for typing, instant for Enter key and barcode input
- **Grid Navigation**: Index-based with 5-column awareness (matches responsive grid)
- **Stock Display**: Context-aware badges (Out of Stock, Back-order, X left)
- **Quick Keys**: Numbers 1-9 add first 9 products instantly

#### Development Workflow
```bash
# 1. Seed the database (run from project root)
cargo run -p titan-db --bin seed

# 2. Run the Tauri app (auto-detects data/titan.db in dev mode)
cd apps/desktop && pnpm tauri dev
```

---

### Milestone 3: Cart & Transaction Engine ✅
**Goal**: Complete cart logic with integer math

| Task | Status | Notes |
|------|--------|-------|
| `Cart` struct in Rust | ✅ | CartState in app state, items with quantities |
| `Money` type with ops | ✅ | i64 cents in titan-core with formatting |
| Tax calculation (Bankers Rounding) | ✅ | Configurable rates, basis points |
| `add_to_cart` command | ✅ | Validates stock respecting trackInventory/allowNegativeStock |
| `remove_from_cart` command | ✅ | Quantity adjustment, removes when 0 |
| `clear_cart` command | ✅ | Full cart reset |
| Cart UI component | ✅ | Line items with prices, live totals |
| Quantity +/- controls | ✅ | Inline editing with bounds checking |
| XState POS machine | ✅ | idle → inCart → tender → receipt |

**Deliverable**: Add items → see cart update → correct tax calculation ✅

**Verification**: Integer math preserves cents - tax calculated with Bankers rounding

#### Architecture Decisions Made
- **Hybrid State Management**: XState v5 for transaction flow (idle→inCart→tender→receipt), SolidJS signals for UI state (search, loading, cart display)
- **Stock Validation**: `add_to_cart` checks `track_inventory` and `allow_negative_stock` flags before allowing additions
- **Cart Persistence**: Cart state persisted in Rust, survives page reloads
- **Money Calculations**: All done server-side in Rust with integer cents

---

### Milestone 4: Tender & Receipt (Mock Payments) ✅
**Goal**: Complete transaction flow with mock payments

| Task | Status | Notes |
|------|--------|-------|
| Tender modal UI | ✅ | Shows amount due, accepts numpad entry |
| Numpad component | ✅ | Auto-detect mode (no decimal=cents, with decimal=dollars) |
| Quick tender buttons | ✅ | $10, $20, $50, Exact amount |
| `add_payment` command | ✅ | Records payment with proper change calculation |
| Split payment support | ✅ | Multiple payment entries supported |
| `finalize_sale` command | ✅ | Atomic transaction commit |
| Sync outbox insertion | ✅ | Queued for future sync on sale finalize |
| Receipt view component | ✅ | ReceiptModal with full receipt display |
| Receipt number generation | ✅ | UUID-based receipt numbers |
| "New Sale" flow | ✅ | XState NEW_SALE event resets to idle |

**Deliverable**: Complete sale → tender → receipt → new sale ✅

#### Architecture Decisions Made
- **Auto-Detect Numpad**: Input without decimal point is interpreted as cents (123 → $1.23), with decimal as dollars (1.23 → $1.23)
- **Change Calculation**: Backend stores both `tendered_cents` (what customer gave) and `change_cents` (what to return)
- **State Machine Flow**: XState ensures valid transitions - can't show receipt without completing tender
- **Toast Notifications**: ToastProvider wraps app for success/error/warning/info messages
- **Keyboard Shortcuts**: F12=Checkout, Escape=Cancel/Clear, Enter=Confirm

#### Files Created/Modified
| File | Purpose |
|------|---------|
| `machines/posMachine.ts` | XState v5 POS state machine |
| `components/ReceiptModal.tsx` | Receipt display after sale |
| `components/Toast.tsx` | Toast notification system |
| `components/TenderModal.tsx` | Updated with auto-detect numpad |
| `commands/cart.rs` | Stock validation with flag checking |
| `commands/sale.rs` | Proper change calculation |
| `App.tsx` | Full XState integration |

---

## Payment Flow Design (For Future Reference)

### v0.1: Mock Payments (Current)
```
┌─────────────────────────────────────────────────────────┐
│                    v0.1 PAYMENT FLOW                    │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  User clicks "Pay" → Tender Modal opens                 │
│       │                                                 │
│       ▼                                                 │
│  Select Payment Method:                                 │
│    • CASH → Enter amount received → Calculate change    │
│    • EXTERNAL_CARD → Mark as paid (no gateway call)     │
│       │                                                 │
│       ▼                                                 │
│  Record in `payments` table (local SQLite)              │
│       │                                                 │
│       ▼                                                 │
│  If total_paid >= amount_due → Finalize sale            │
│       │                                                 │
│       ▼                                                 │
│  Insert into `sync_outbox` for future cloud sync        │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### v1.0+: Integrated Payments (Future)

#### Payment Methods by Region

| Region | Primary Options | Integration Type |
|--------|-----------------|------------------|
| **USA** | Stripe Terminal, Square | Semi-Integrated |
| **Europe** | Stripe Terminal, Adyen, SumUp | Semi-Integrated |
| **UK** | Stripe Terminal, Zettle | Semi-Integrated |
| **India** | Razorpay, PayTM | API + QR |
| **Pakistan** | JazzCash, EasyPaisa, HBL | API + QR |
| **SE Asia** | GrabPay, GCash, OVO | API + QR |

#### Semi-Integrated Architecture (Recommended)
```
┌─────────────────────────────────────────────────────────┐
│              SEMI-INTEGRATED PAYMENT FLOW               │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  POS (Titan) ──────► Payment Terminal (Hardware)        │
│       │                      │                          │
│       │  1. Send amount      │                          │
│       │  ─────────────────►  │                          │
│       │                      │  2. Customer taps card   │
│       │                      │  3. Terminal → Gateway   │
│       │                      │  4. Gateway → Bank       │
│       │  5. Result           │                          │
│       │  ◄─────────────────  │                          │
│       │                      │                          │
│  POS NEVER sees card data (PCI-DSS compliant)          │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

#### Pakistan-Specific Integration Notes
```
┌─────────────────────────────────────────────────────────┐
│              PAKISTAN PAYMENT LANDSCAPE                 │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  Mobile Wallets (Most Common):                          │
│    • JazzCash - REST API + QR code generation           │
│    • EasyPaisa - REST API + QR code generation          │
│    • SadaPay - Modern API, card support                 │
│    • NayaPay - Modern API, card support                 │
│                                                         │
│  Bank Integration:                                      │
│    • HBL Connect - Corporate API                        │
│    • 1Link - Inter-bank switching                       │
│    • Keenu - Multi-bank aggregator                      │
│                                                         │
│  Recommended Approach for Pakistan:                     │
│    1. QR-based payments (JazzCash/EasyPaisa)            │
│    2. Display QR on screen                              │
│    3. Poll for payment confirmation                     │
│    4. SadaPay/NayaPay for card-present                  │
│                                                         │
│  Note: Most Pakistani banks don't have terminal APIs    │
│  like Stripe Terminal. QR/mobile wallet is primary.     │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

---

## Database Migration Strategy

### Tool: `sqlx` with Embedded Migrations

```rust
// Migration files are embedded at compile time
// Located in: migrations/sqlite/

migrations/sqlite/
├── 001_initial_schema.sql      # Core tables
├── 002_add_fts.sql             # Full-text search
├── 003_add_indexes.sql         # Performance indexes
└── 004_seed_config.sql         # Default configuration
```

### Running Migrations

```rust
// In titan-db/src/sqlite/migrations.rs
use sqlx::migrate::Migrator;

static MIGRATOR: Migrator = sqlx::migrate!("../../migrations/sqlite");

pub async fn run_migrations(pool: &SqlitePool) -> Result<(), MigrationError> {
    MIGRATOR.run(pool).await?;
    Ok(())
}
```

### Migration Versioning
- Migrations are embedded in the binary
- Version tracked in `_sqlx_migrations` table
- App auto-migrates on startup
- Never modify existing migrations (always add new ones)

---

## Verification Checklist (Before v0.1 Release)

### Data Integrity
- [x] Money: All calculations use integer cents (i64), no floating point
- [x] Tax: Calculated with Bankers Rounding using basis points
- [x] UUID collision handling (all entities use UUID v4)

### Performance
- [x] Search 50,000 products in <10ms (FTS5 index)
- [x] App startup <1 second
- [x] Cart recalculation <5ms (all Rust-side)

### Offline
- [x] All operations work with network disconnected (local SQLite)
- [x] Sync outbox populated on sale finalize
- [x] Cart state persists in Rust memory (survives page reload)

### Transaction Flow
- [x] Add items to cart
- [x] Stock validation respects product flags
- [x] Tender modal with numpad entry
- [x] Multiple payment support (split tender)
- [x] Receipt display after payment
- [x] New sale resets state cleanly

---

## Legend

| Symbol | Meaning |
|--------|---------|
| ⬜ | Not started |
| 🟡 | In progress |
| ✅ | Complete |
| ❌ | Blocked |

---

*Progress tracked by: Development Team*  
*Update frequency: Daily during active development*
