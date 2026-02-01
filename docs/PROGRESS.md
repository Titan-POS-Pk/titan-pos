# Titan POS v0.1 - Development Progress

> **Status**: 🟡 Milestone 2 Complete - In Development  
> **Target**: v0.1 "Logical Core"  
> **Last Updated**: February 1, 2026

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

### Milestone 3: Cart & Transaction Engine ⬜
**Goal**: Complete cart logic with integer math

| Task | Status | Notes |
|------|--------|-------|
| `Cart` struct in Rust | ⬜ | Items, quantities, totals |
| `Money` type with ops | ⬜ | Add, multiply, tax calc |
| Tax calculation (Bankers Rounding) | ⬜ | Configurable rates |
| `add_to_cart` command | ⬜ | Validate stock, update totals |
| `remove_from_cart` command | ⬜ | Quantity adjustment |
| `clear_cart` command | ⬜ | Reset state |
| Cart UI component | ⬜ | Line items, totals display |
| Quantity +/- controls | ⬜ | Inline editing |
| XState POS machine | ⬜ | idle → inCart → tender |

**Deliverable**: Add items → see cart update → correct tax calculation

**Verification**: `100 / 3 * 3` must not lose cents

---

### Milestone 4: Tender & Receipt (Mock Payments) ⬜
**Goal**: Complete transaction flow with mock payments

| Task | Status | Notes |
|------|--------|-------|
| Tender modal UI | ⬜ | Amount due, payment entry |
| Numpad component | ⬜ | Manual amount entry |
| Quick tender buttons | ⬜ | $10, $20, $50, Exact |
| `process_payment` command | ⬜ | Record payment, calc change |
| Split payment support | ⬜ | Multiple payment entries |
| `finalize_sale` command | ⬜ | Atomic transaction commit |
| Sync outbox insertion | ⬜ | Queue for future sync |
| Receipt view component | ⬜ | HTML receipt display |
| Receipt number generation | ⬜ | YYYYMMDD-Device-Seq format |
| "New Sale" flow | ⬜ | Reset and return to idle |

**Deliverable**: Complete sale → tender → receipt → new sale

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
- [ ] Money: `$10.00 / 3 * 3 = $9.99` (not $10.00 - intentional precision loss documented)
- [ ] Tax: 8.25% of $10.00 = $0.83 (Bankers Rounding)
- [ ] UUID collision handling (retry on unique constraint)

### Performance
- [ ] Search 50,000 products in <10ms
- [ ] App startup <1 second
- [ ] Cart recalculation <5ms

### Offline
- [ ] All operations work with network disconnected
- [ ] Sync outbox populated correctly
- [ ] Data persists across app restarts

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
