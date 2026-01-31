# Titan POS: Architectural Decision Records (ADR)

> **Status**: DRAFT - Awaiting Product Owner Input  
> **Version**: 0.1.0  
> **Last Updated**: January 31, 2026

---

## Executive Summary

This document captures the key architectural decisions for Titan POS. Each decision is documented with context, options considered, and the chosen approach. **Items marked with ⚠️ require explicit confirmation before implementation.**

---

## ADR-001: Project Structure

### Context
We need to organize the codebase to support:
- Offline-first desktop app (Tauri)
- Future cloud sync service
- Hardware abstraction layer
- Regional compliance modules

### Decision: **Monorepo with Rust Workspace**

```
titan-pos/
├── .github/                    # CI/CD workflows
├── docs/                       # Architecture & API docs
│   ├── architecture/
│   ├── api/
│   └── decisions/
├── crates/                     # Rust workspace members
│   ├── titan-core/             # Pure business logic (no I/O)
│   ├── titan-db/               # Database abstractions
│   ├── titan-sync/             # Sync engine & CRDT
│   ├── titan-hal/              # Hardware abstraction (future)
│   └── titan-fiscal/           # Regional compliance (future)
├── apps/
│   ├── desktop/                # Tauri v2 application
│   │   ├── src-tauri/          # Rust backend
│   │   └── src/                # SolidJS frontend
│   └── cloud/                  # Cloud sync service (future)
├── packages/                   # Shared TypeScript packages
│   ├── ui/                     # Shared UI components
│   └── types/                  # Shared TypeScript types
├── proto/                      # Protobuf definitions
├── migrations/                 # Database migrations
│   ├── sqlite/
│   └── postgres/
├── scripts/                    # Build & dev scripts
├── Cargo.toml                  # Workspace root
├── package.json                # pnpm workspace root
└── turbo.json                  # Turborepo config (optional)
```

### Rationale
- **Crate Isolation**: `titan-core` has zero dependencies on I/O, making it fully testable
- **Feature Flags**: Hardware and fiscal modules can be compiled out for lighter builds
- **Shared Types**: Protobuf ensures type safety between Rust and TypeScript
- **Future Growth**: Cloud service shares crates with desktop app

---

## ADR-002: Rust Crate Boundaries

### titan-core (The Heart)
```rust
// NO external I/O allowed - pure functions only
pub mod cart;           // Cart calculations
pub mod pricing;        // Discount & tax logic
pub mod transaction;    // Transaction state machine
pub mod validation;     // Business rule validation
pub mod types;          // Domain types (Money, Qty, etc.)
```

**Dependencies**: Only `serde`, `thiserror`, `rust_decimal` (for intermediate calc)

### titan-db (The Persistence Layer)
```rust
pub mod sqlite;         // SQLite implementation
pub mod postgres;       // Postgres implementation (future)
pub mod migrations;     // Schema management
pub mod repository;     // Repository traits
```

**Dependencies**: `sqlx` or `rusqlite`, `titan-core`

### titan-sync (The Bridge)
```rust
pub mod crdt;           // CRDT implementations
pub mod protocol;       // Sync protocol (Protobuf)
pub mod outbox;         // Outbox pattern
pub mod transport;      // WebSocket client
```

**Dependencies**: `tokio`, `tungstenite`, `prost`, `titan-core`

---

## ADR-003: Database Migration Strategy

### Decision: **sqlx with Embedded Migrations**

### Rationale
- Compile-time SQL verification
- Migrations embedded in binary (no separate files at runtime)
- Works for both SQLite and Postgres

### Schema Versioning for Offline Clients
```
┌─────────────────────────────────────────────────────────┐
│ Problem: Client offline during schema migration         │
├─────────────────────────────────────────────────────────┤
│ Solution:                                               │
│ 1. App stores `schema_version` in local metadata        │
│ 2. On startup, app checks if migrations are needed      │
│ 3. Migrations run locally before any sync attempt       │
│ 4. Sync protocol includes schema_version in handshake   │
│ 5. Server rejects sync if schema_version < minimum      │
└─────────────────────────────────────────────────────────┘
```

---

## ADR-004: Error Handling Strategy

### Decision: **Layered Error Types**

```rust
// titan-core: Domain errors
pub enum DomainError {
    InvalidQuantity(i32),
    InsufficientStock { sku: String, available: i32, requested: i32 },
    InvalidDiscount(String),
    TransactionAlreadyFinalized,
}

// titan-db: Persistence errors
pub enum DbError {
    NotFound { entity: String, id: String },
    UniqueViolation { field: String, value: String },
    ConnectionFailed(String),
    MigrationFailed(String),
}

// titan-tauri: API errors (what frontend sees)
#[derive(Serialize)]
pub struct ApiError {
    pub code: String,           // Machine-readable: "INSUFFICIENT_STOCK"
    pub message: String,        // Human-readable: "Not enough stock"
    pub details: Option<Value>, // Additional context
}
```

---

## ADR-005: State Management Architecture

### Decision: **Hybrid State Management**

| State Type | Owner | Technology |
|------------|-------|------------|
| UI State (modals, focus) | Frontend | SolidJS signals |
| POS Flow State | Frontend | XState |
| Cart State | Rust Core | `Mutex<CartState>` |
| Transaction State | Rust Core | State machine in Rust |
| Sync State | Rust Core | Actor model |

### The Cart Ownership Model
```
┌──────────────────────────────────────────────────────────┐
│ Frontend (SolidJS)                                       │
│  ├── Sends "Intent" commands to Rust                     │
│  └── Receives "State Projection" from Rust               │
├──────────────────────────────────────────────────────────┤
│ Rust Core                                                │
│  ├── Owns the Cart (single source of truth)              │
│  ├── Validates all mutations                             │
│  └── Emits events for UI updates                         │
└──────────────────────────────────────────────────────────┘
```

---

## ADR-006: Money Representation

### Decision: **Integer Cents with Explicit Type**

```rust
/// Represents monetary value in the smallest currency unit (cents)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Money(i64);

impl Money {
    pub fn from_cents(cents: i64) -> Self {
        Money(cents)
    }
    
    pub fn from_major(major: i64, minor: i64) -> Self {
        Money(major * 100 + minor)
    }
    
    pub fn cents(&self) -> i64 {
        self.0
    }
    
    // Display formatted (e.g., "$10.00")
    pub fn display(&self, currency: &Currency) -> String {
        // Implementation
    }
}
```

### Tax Calculation (Bankers Rounding)
```rust
pub fn calculate_tax(amount: Money, rate_bps: u32) -> Money {
    // rate_bps: 500 = 5.00%
    let tax_cents = (amount.cents() as i128 * rate_bps as i128 + 5000) / 10000;
    Money::from_cents(tax_cents as i64)
}
```

---

## ⚠️ DECISIONS REQUIRING YOUR INPUT

### DQ-001: Tenant Model for v0.1

**Question**: Should v0.1 be single-tenant (one store, one device) or include `tenant_id` from the start?

| Option | Pros | Cons |
|--------|------|------|
| A: Single-tenant | Simpler, faster to build | Schema change later |
| B: Multi-tenant schema, single-tenant runtime | Future-proof | Slightly more complex |

**My Recommendation**: Option B - Include `tenant_id` in schema but hardcode it for v0.1

---

### DQ-002: User Authentication Model

**Question**: How should user authentication work in v0.1?

| Option | Description |
|--------|-------------|
| A: No auth | Device is implicitly trusted, `user_id` from config |
| B: PIN-based | Fast cashier switching with 4-6 digit PIN |
| C: Full login | Username + password (stored locally hashed) |

**My Recommendation**: Option A for v0.1, with schema ready for Option B

---

### DQ-003: Tax Calculation Mode

**Question**: How is pricing displayed to customers?

| Option | Description | Common In |
|--------|-------------|-----------|
| A: Tax-exclusive | Price + Tax shown separately | USA, Canada |
| B: Tax-inclusive | Price includes tax | EU, UK, Australia |

**My Recommendation**: Support both via `tenant.tax_mode` config, default to tax-exclusive

---

### DQ-004: Discount Application

**Question**: When discounts are applied, what's the order of operations?

```
Option A: Discount → Tax (Most common)
  Subtotal: $100
  Discount: -$10
  Taxable:  $90
  Tax (10%): $9
  Total:    $99

Option B: Tax → Discount (Rare)
  Subtotal: $100
  Tax (10%): $10
  Discount: -$10
  Total:    $100
```

**My Recommendation**: Option A (Discount before tax)

---

### DQ-005: Negative Inventory

**Question**: Should the system allow selling items when stock is 0 or negative?

| Option | Use Case |
|--------|----------|
| A: Block sale | Strict inventory control |
| B: Allow with warning | "We'll ship it later" retail |
| C: Allow silently | Services or non-tracked items |

**My Recommendation**: Configurable per-product with `track_inventory: bool` and `allow_negative: bool`

---

### DQ-006: Receipt/Invoice Numbering

**Question**: How should receipt numbers be generated to avoid offline collisions?

| Option | Format | Example |
|--------|--------|---------|
| A: UUID only | UUID v4 | `a1b2c3d4-...` |
| B: Device-prefixed sequential | `{device_id}-{seq}` | `POS001-000001` |
| C: Date-based sequential | `{YYYYMMDD}-{device}-{seq}` | `20260131-01-0001` |

**My Recommendation**: Option C for human readability, with UUID as the actual PK

---

### DQ-007: Sync Conflict Display

**Question**: When inventory conflicts occur after sync, should users be notified?

| Option | Description |
|--------|-------------|
| A: Silent merge | CRDT handles it, user never knows |
| B: Notification | "Inventory adjusted after sync" alert |
| C: Manual review | Flagged for manager approval |

**My Recommendation**: Option A for v0.1, with audit log for investigation

---

### DQ-008: Offline Duration Limit

**Question**: Is there a maximum time the system can operate offline before requiring sync?

| Option | Description |
|--------|-------------|
| A: Unlimited | Can operate forever offline |
| B: Soft limit | Warning after X days |
| C: Hard limit | Lock after X days |

**My Recommendation**: Soft limit (warning after 7 days), configurable per tenant

---

### DQ-009: Data Retention

**Question**: How long should transaction history be kept locally?

| Option | Description |
|--------|-------------|
| A: Forever | All history on device |
| B: Rolling window | Last N days, older archived to cloud |
| C: Synced only | Remove local data after cloud confirmation |

**My Recommendation**: Option B (90 days local, archived after sync)

---

### DQ-010: Target Platforms for v0.1

**Question**: Which desktop platforms should v0.1 support?

| Platform | Priority | Notes |
|----------|----------|-------|
| macOS (ARM) | ? | Development machine |
| macOS (Intel) | ? | Legacy Macs |
| Windows 10/11 | ? | Most POS hardware |
| Linux (Ubuntu) | ? | Kiosk deployments |

**My Recommendation**: macOS (ARM) + Windows 10/11 for v0.1

---

## Confirmed Decisions (From PRD)

These are already decided in the PRD:

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Local DB | SQLite | Embedded, zero-config, transactional |
| Cloud DB | PostgreSQL | ACID, RLS, partitioning |
| Frontend | SolidJS | Compiled DOM updates, fast |
| Desktop | Tauri v2 | Native webview, small footprint |
| Language | Rust | Memory safety, no GC pauses |
| Sync Protocol | WebSocket + Protobuf | Binary, typed, efficient |
| ID Strategy | UUID v4 (system) + SKU (business) | Dual-key immutability |
| Money Format | Integer (cents) | No floating point errors |
| Conflict Resolution | CRDT (Delta-state) | Mathematical convergence |

---

## Next Steps

1. **Review this document** and provide answers to DQ-001 through DQ-010
2. Once confirmed, I will create:
   - `.github/copilot-instructions.md` - AI coding guidelines
   - `docs/CONTRIBUTING.md` - Development guidelines
   - `docs/architecture/CRATE_GUIDE.md` - Crate responsibilities
   - Initial Cargo.toml workspace structure
   - Database schema migrations

---

*Document maintained by: AI Architect*  
*Review cycle: Before each major version*
