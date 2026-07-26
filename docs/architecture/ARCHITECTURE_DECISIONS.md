# Titan POS: Architectural Decision Records

> The decisions this codebase is built on, and what each one cost. Where a
> decision has been revised by the implementation, the revision is recorded
> under it rather than the original being edited away.

---

## ADR-001: Monorepo with a Rust Workspace

### Context

The system has to cover an offline-first desktop app, a store-local sync hub,
a cloud service, and eventually hardware and regional-compliance modules.
Those share domain types — if `Money` means one thing in the till and another
in the cloud, the two disagree about a total and nobody can tell which is
right.

### Decision

One repository, one Cargo workspace, with the domain in a crate that no other
layer can bypass.

```
titan-pos/
├── crates/
│   ├── titan-core/       # Pure business logic, no I/O
│   ├── titan-db/         # SQLite persistence
│   └── titan-sync/       # Hub, election, CRDT sync
├── apps/
│   ├── desktop/          # Tauri v2 app (src-tauri + SolidJS src)
│   └── cloud-api/        # Cloud gRPC server
├── migrations/{sqlite,postgres}/
├── proto/                # Protobuf definitions
└── scripts/              # Multi-instance dev helpers
```

### Rationale

`titan-core` has no I/O dependency, so every rule in it is testable without a
database, a socket or a clock. That is what makes the money tests cheap enough
to write exhaustively.

### Revised since

The original plan had `packages/` for shared TypeScript, `turbo.json`, a root
`package.json`, and `titan-hal` / `titan-fiscal` crates. None were built. The
only npm workspace is `apps/desktop`; there is no root `package.json`. Types
cross the Rust/TypeScript boundary through Tauri command signatures and
Protobuf, which turned out to be enough.

---

## ADR-002: Crate Boundaries

| Crate | Owns | May depend on |
|-------|------|---------------|
| `titan-core` | `Money`, `TaxRate`, cart maths, validation | `serde`, `thiserror` — no I/O |
| `titan-db` | SQLite schema, repositories, FTS5 search | `sqlx`, `titan-core` |
| `titan-sync` | Transport, hub election, CRDT deltas, outbox | `tokio`, `tungstenite`, `prost`, `titan-core` |

The rule that matters is the first one: `titan-core` cannot open a file or a
socket. Anything that needs to is in the wrong crate.

---

## ADR-003: Migrations via sqlx, verified at compile time

### Decision

`sqlx` with migrations embedded in the binary, and `sqlx::query_as!` verifying
SQL against a real schema when the crate compiles.

### Consequence, which is not free

Compile-time verification means the build needs either a live database or the
committed `.sqlx/` offline cache. The cache is tracked for exactly this
reason; regenerate it with
`cargo sqlx prepare --workspace -- --all-targets` when a query changes. A
stale cache fails the build rather than passing quietly.

### Schema versioning for offline clients

```
┌─────────────────────────────────────────────────────────┐
│ Problem: client is offline across a schema migration    │
├─────────────────────────────────────────────────────────┤
│ 1. App stores `schema_version` in local metadata        │
│ 2. On startup it migrates locally                       │
│ 3. Migrations run before any sync attempt               │
│ 4. The sync handshake carries schema_version            │
│ 5. The hub rejects a sync below its minimum             │
└─────────────────────────────────────────────────────────┘
```

---

## ADR-004: Layered Error Types

Each layer names the failures it can actually produce, and the boundary
converts. The frontend never sees a `sqlx::Error`.

```rust
// titan-core: domain errors
pub enum DomainError {
    InvalidQuantity(i32),
    InsufficientStock { sku: String, available: i32, requested: i32 },
    InvalidDiscount(String),
    TransactionAlreadyFinalized,
}

// titan-db: persistence errors
pub enum DbError {
    NotFound { entity: String, id: String },
    UniqueViolation { field: String, value: String },
    ConnectionFailed(String),
    MigrationFailed(String),
}

// Tauri boundary: what the frontend sees
#[derive(Serialize)]
pub struct ApiError {
    pub code: String,           // "INSUFFICIENT_STOCK"
    pub message: String,        // "Not enough stock"
    pub details: Option<Value>,
}
```

---

## ADR-005: State Ownership

| State | Owner | Mechanism |
|-------|-------|-----------|
| UI state (modals, focus) | Frontend | SolidJS signals |
| POS flow | Frontend | XState |
| Cart | Rust | `Mutex<CartState>` |
| Transaction | Rust | State machine |
| Sync | Rust | Actor model over channels |

The cart lives in Rust and the frontend never mutates it directly:

```
┌──────────────────────────────────────────────────────────┐
│ Frontend (SolidJS)                                       │
│  ├── Sends intent commands to Rust                       │
│  └── Receives a state projection back                    │
├──────────────────────────────────────────────────────────┤
│ Rust core                                                │
│  ├── Owns the cart (single source of truth)              │
│  ├── Validates every mutation                            │
│  └── Emits events for UI updates                         │
└──────────────────────────────────────────────────────────┘
```

The point is that a cart total can only be produced by code that went through
validation. A frontend that computed its own subtotal would be a second,
divergent implementation of the money rules.

---

## ADR-006: Money is `i64` Cents

### Decision

`Money(i64)` in the smallest currency unit. There is no `Money::from_float`,
and adding one would be the single fastest way to break this system.

### Tax: round half to even

The first implementation of `calculate_tax` was:

```rust
let tax_cents = (amount.cents() as i128 * rate_bps as i128 + 5000) / 10000;
```

That is documented as banker's rounding and is not. `+5000` before an
integer division is round half *up*, and because Rust's `/` truncates toward
zero rather than flooring, the bias ran in opposite directions by sign. $10.00
at 8.25% is exactly 82.5¢: the sale charged 83¢ and the matching refund
returned 82¢, so the errors compounded instead of cancelling.

It now goes through `div_round_half_even`, which uses `div_euclid`/`rem_euclid`
so the remainder is always non-negative and the tie test is the same
expression on both signs. `crates/titan-core/src/money.rs`.

### Bounds are derived, not chosen

`MAX_PRICE_CENTS` is $1,000,000,000.00. It exists because
`validate_price_cents` previously had no upper bound, so `i64::MAX` passed
validation and overflowed the line total three layers away in `cart.rs`. The
value is the one that keeps the worst legal cart inside `i64`:

`MAX_PRICE_CENTS × MAX_ITEM_QUANTITY (999) × MAX_CART_ITEMS (100)`, plus the
100% tax that `validate_tax_rate_bps` allows, is about 1.0e16 — roughly 920×
below `i64::MAX`. `test_worst_case_cart_fits_in_i64` asserts that chain, so
raising any one limit fails a test instead of silently reopening the overflow.

Arithmetic operators are `checked_*` and panic in every profile, release
included. A wrapped `i64` total goes negative, and every consumer downstream
reads a negative total as a refund; a panic is loud and leaves the local
transaction unfinalised. `checked_add` / `checked_sub` /
`checked_multiply_quantity` are public for callers that want to handle it.

---

## ADR-007: Multi-tenant Schema, Single-tenant Runtime

Every table carries `tenant_id` from the first migration, defaulted to a
single hardcoded UUID
(`migrations/sqlite/001_initial_schema.sql`). Adding a column to a shipped
offline database on thousands of tills is a migration problem; carrying an
unused column is not.

---

## ADR-008: No Authentication in v0.1

The device is trusted and `user_id` comes from config. The schema is shaped
for PIN-based cashier switching, which is what a lane actually needs — a
username-and-password login at every cashier change is not workable at a
till — but v0.1 does not implement it. This is a real limitation, not a
deferred nicety: anyone at the device can ring a sale.

---

## ADR-009: Tax-exclusive by Default, Discount Before Tax

Tax mode is a tenant setting; the default is tax-exclusive (USA/Canada
convention), with tax-inclusive (EU/UK/AU) the other option.

Discounts apply before tax, which is what almost every jurisdiction requires:

```
Subtotal:  $100
Discount:  -$10
Taxable:    $90
Tax (10%):   $9
Total:      $99
```

The alternative — taxing before discounting — charges tax on money the
customer never paid.

`apply_percentage_discount` is clamped at 10,000 bps. It previously had no
upper bound, so a 150% discount returned a negative price, i.e. the till
paying the customer.

---

## ADR-010: Negative Inventory is a Per-Product Setting

`track_inventory` and `allow_negative_stock` are columns on `products`
(`001_initial_schema.sql`), defaulting to tracked and non-negative. A single
global policy would be wrong for any catalogue that mixes stocked goods with
services.

---

## ADR-011: Receipt Numbers are Date + Device + Sequence

`generate_receipt_number` produces `YYYYMMDD-DD-NNNN`, e.g.
`20260131-01-0001` (`crates/titan-db/src/repository/sale.rs`). The UUID stays
the primary key; the receipt number exists so a human can read it back over a
phone. Prefixing by device is what keeps two offline tills from minting the
same number.

### Known gap

The sequence is currently derived from timestamp milliseconds rather than a
per-device counter. It is collision-resistant in practice but it is not a
sequence, and a customer reading two receipts cannot tell which came first.

---

## ADR-012: Sync Conflicts Merge Silently

Inventory moves as CRDT deltas (`change: -3`), never absolutes
(`stock = 7`), so two tills selling the same item concurrently converge
instead of overwriting each other. There is no user-facing conflict prompt: a
cashier with a customer waiting cannot adjudicate a merge. The audit log is
where a manager investigates afterwards.

---

## ADR-013: Offline Duration is Soft-Limited

Warn after 7 days, configurable per tenant; never lock. A till that refuses
to sell because it has not phoned home is worse than a till with stale
product data — the entire premise of the system is that the network is
optional.

---

## Decisions Inherited from the Product Requirements

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Local DB | SQLite | Embedded, zero-config, transactional |
| Cloud DB | PostgreSQL | ACID, RLS, partitioning |
| Frontend | SolidJS | Compiled DOM updates, no VDOM diff |
| Desktop | Tauri v2 | Native webview, small footprint |
| Language | Rust | Memory safety, no GC pauses at the till |
| Sync protocol | WebSocket + Protobuf | Binary, typed, efficient |
| ID strategy | UUID v4 (system) + SKU (business) | Dual-key immutability |
| Money format | Integer cents | No floating point |
| Conflict resolution | CRDT (delta-state) | Converges without a coordinator |
