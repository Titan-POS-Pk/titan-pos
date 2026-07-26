# Titan POS

> Offline-first point of sale. Rust + Tauri v2 + SolidJS + SQLite.

A till has to keep selling when the network is gone, and the money has to
still be right when it comes back. Local SQLite is the source of truth; sync
is a background side-effect, never a precondition for a sale.

---

## Three problems this repo actually solved

**Tax rounding was biased against refunds.** `calculate_tax` documented
banker's rounding — round half to even — and implemented
`(cents * bps + 5000) / 10000`, which is round half *up*. Rust's integer
division truncates toward zero rather than flooring, so the bias ran in
opposite directions by sign. A $10.00 sale at 8.25% is exactly 82.5¢: the
sale charged 83¢, the matching refund returned 82¢. Every tie leaked a cent
into the tax account. Fixed with Euclidean division so the tie test is
sign-symmetric, and sign symmetry is now a property test over 2,000 amounts
rather than an example.
→ [`crates/titan-core/src/money.rs`](crates/titan-core/src/money.rs)

**The split-brain guard could never fire.** The election waits a randomized
150–300ms and then checks whether it is still a Candidate before promoting
itself — a competing claim arriving in that window is supposed to demote it.
The wait was a bare `sleep().await` inside the service's own `select!` arm,
which starved the command channel for its full duration. Heartbeats queued
behind the sleep and were drained only after the decision, so the check was
unconditionally true and two devices that campaigned together both became
PRIMARY for the same store. Both that window and the post-election grace
period now drain commands while they wait.
→ [`crates/titan-sync/src/election.rs`](crates/titan-sync/src/election.rs)

**Searching by SKU threw.** The FTS5 query was built as
`format!("{}*", input)`, but the MATCH right-hand side is a query language,
not a literal. Every SKU here looks like `BEV-COC-001`, and a bare `-` is a
column-filter operator, so the commonest search a cashier performs returned
`Error: no such column: COC` straight out of SQLite.
→ [`crates/titan-db/src/repository/product.rs`](crates/titan-db/src/repository/product.rs)

### Search latency, measured

50,000 products, WAL mode, SQLite 3.x on an M-series Mac. 500 iterations per
query in a single connection, process startup subtracted:

| Query | Rows matched | Median |
|-------|--------------|--------|
| SKU prefix `BEV` | 200 | 0.34 ms |
| Full product name | 16 | 0.57 ms |
| Barcode prefix | 1,000 | 1.02 ms |
| Term matching 12% of catalogue | 6,135 | 27.7 ms |

Cost tracks the number of matches, not the catalogue size, because
`ORDER BY rank` scores every hit before the `LIMIT` applies. Sub-10ms is
comfortable for selective queries — SKU, barcode, product name — which is
what a lane actually types. A deliberately unselective term is not, and that
row is in the table for the same reason.

Note the seed generator caps at ~1,000 products regardless of `--count`; the
50k figures above come from synthesized rows, not from `cargo run --bin seed`.

### Design commitments

- **Integer money.** Every amount is `i64` cents. There is no
  `Money::from_float`. Operators panic on overflow rather than wrapping,
  because a wrapped total is a wrong total that looks plausible.
- **Dual-key identity.** UUID for relations, SKU for humans. The SKU can
  change; the UUID cannot.
- **Offline-first.** Every operation completes against local SQLite. Sync
  reads an outbox afterwards.
- **CRDT-shaped sync.** Inventory moves as deltas (`change: -3`), never as
  absolutes (`stock = 7`), so two tills selling the same item concurrently
  converge instead of overwriting each other.

### Tech Stack

| Layer | Technology | Rationale |
|-------|------------|-----------|
| Runtime | Tauri v2 | Native performance, small footprint |
| Backend | Rust | Memory safety, zero GC pauses |
| Frontend | SolidJS | Compiled DOM updates, fast rendering |
| Local DB | SQLite | Embedded, transactional, zero-config |
| Cloud DB | PostgreSQL | ACID, RLS, partitioning |
| State | XState | Finite state machines for POS flow |
| Sync | WebSocket + Protobuf | Binary, efficient, typed |

---

## Project Structure

```
titan-pos/
├── crates/                 # Rust workspace
│   ├── titan-core/         # Pure business logic (no I/O)
│   ├── titan-db/           # Database abstraction
│   └── titan-sync/         # Sync engine & CRDT
├── apps/
│   └── desktop/            # Tauri application
│       ├── src-tauri/      # Rust backend
│       └── src/            # SolidJS frontend
├── docs/                   # Architecture docs
├── migrations/             # SQL migrations
└── proto/                  # Protobuf definitions
```

---

## Quick Start

### Prerequisites

- Rust 1.75+ (with `cargo`)
- Node.js 20+ (with `pnpm`)
- Tauri CLI (`cargo install tauri-cli`)

### Development

```bash
# Clone the repository
git clone https://github.com/your-org/titan-pos.git
cd titan-pos

# Install dependencies
pnpm install

# Run in development mode
pnpm dev
```

### Build

```bash
# Build for production
pnpm build

# Build installers (macOS, Windows)
pnpm tauri build
```

---

## Documentation

| Document | Description |
|----------|-------------|
| [Architecture Decisions](docs/architecture/ARCHITECTURE_DECISIONS.md) | ADRs and design rationale |
| [Architecture Diagrams](docs/architecture/ARCHITECTURE_DIAGRAMS.md) | System diagrams |
| [Crate Guide](docs/architecture/CRATE_GUIDE.md) | Crate responsibilities |
| [Sync Architecture](docs/architecture/SYNC_ARCHITECTURE.md) | Store hub, election, CRDT sync |
| [Running the sync modes](docs/RUNNING_SYNC_MODES.md) | Bring up a multi-device store |

---

## Roadmap

| Version | Focus | Status |
|---------|-------|--------|
| v0.1 | Logical core: money, cart, FTS search, local persistence | Shipped |
| v0.2 | Store sync: hub election, CRDT inventory, store aggregation | Shipped |
| v0.5 | Hardware I/O | Q2 2026 |
| v1.0 | Integrated Payments | Q3 2026 |
| v1.5 | Multi-Store | Q4 2026 |
| v2.0 | Enterprise Analytics | 2027 |

---

## Contributing

See [CONTRIBUTING.md](docs/CONTRIBUTING.md) for development guidelines.

---

## License

Proprietary. All rights reserved.
