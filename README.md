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
sign-symmetric, and `tax(-x) == -tax(x)` is now asserted by an exhaustive
sweep over every amount from 1¢ to $20.00 at 8.25% — a deterministic loop,
not a randomised property test.
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

```bash
cargo run --release -p titan-db --bin bench_search
```

50,000 products, WAL mode, SQLite 3.51 on an Apple M1 Pro. 500 iterations per
query against one already-open pool, after a warmup pass, so process startup
and connection setup are outside the measurement:

| Query | Rows matched | Median | p95 |
|-------|--------------|--------|-----|
| Full SKU, typed (`BEV-COC-000`) | 1 | 0.11 ms | 0.26 ms |
| Full barcode, scanned | 1 | 0.08 ms | 0.18 ms |
| Full product name | 500 | 1.10 ms | 2.78 ms |
| Bare category prefix (`BEV`) | 10,000 | 6.99 ms | 11.15 ms |

Cost tracks the number of matches, not the catalogue size, because
`ORDER BY rank` scores every hit before the `LIMIT` applies. The two things a
lane actually does — type a SKU, scan a barcode — are the two fastest rows
and stay under a millisecond. The last row is a query nobody types
deliberately, and it is in the table because it is where the sub-10ms claim
stops holding: at p95 it does not.

The harness seeds its own throwaway database, so these numbers are
reproducible from a clean checkout rather than asserted. Medians were stable
to within 0.05 ms across three runs; the table is one machine's figures, not
a guarantee.

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
│   ├── desktop/            # Tauri application
│   │   ├── src-tauri/      # Rust backend
│   │   └── src/            # SolidJS frontend
│   └── cloud-api/          # Cloud gRPC server
├── .sqlx/                  # sqlx offline query cache (tracked)
├── docs/                   # Architecture docs
├── migrations/             # SQL migrations
├── proto/                  # Protobuf definitions
└── scripts/                # Multi-instance dev helpers
```

---

## Quick Start

### Prerequisites

- Rust 1.75+ (with `cargo`)
- Node.js 20+ and `pnpm` 9+
- Tauri CLI (`cargo install tauri-cli`)

No database setup is required. `sqlx` verifies SQL at compile time against
the committed `.sqlx/` cache, so a fresh clone builds offline.

### Build and run

The npm workspace is `apps/desktop`, not the repository root — there is no
root `package.json`. The frontend has to be built at least once before the
Rust side will compile, because `tauri_build` checks that `frontendDist`
exists.

```bash
git clone https://github.com/Titan-POS-Pk/titan-pos.git
cd titan-pos/apps/desktop

pnpm install
pnpm build            # required before any cargo build
pnpm tauri dev        # run the app
```

### Rust-only checks

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

### Installers

```bash
cd apps/desktop
pnpm tauri build      # macOS, Windows, Linux
```

### Seed a catalogue

```bash
cargo run -p titan-db --bin seed -- --count 50000 --db ./data/titan.db
```

---

## Documentation

| Document | Description |
|----------|-------------|
| [Sync Architecture](docs/architecture/SYNC_ARCHITECTURE.md) | Store hub, election, CRDT sync |
| [Architecture Decisions](docs/architecture/ARCHITECTURE_DECISIONS.md) | ADRs and design rationale |
| [Crate Guide](docs/architecture/CRATE_GUIDE.md) | Crate responsibilities |
| [Architecture Diagrams](docs/architecture/ARCHITECTURE_DIAGRAMS.md) | System diagrams |
| [Running the sync modes](docs/RUNNING_SYNC_MODES.md) | Bring up a multi-device store |
| [Contributing](docs/CONTRIBUTING.md) | Development guidelines |

---

## Roadmap

| Version | Focus | Status |
|---------|-------|--------|
| v0.1 | Logical core: money, cart, FTS search, local persistence | Shipped |
| v0.2 | Store sync: hub election, CRDT inventory, store aggregation | Shipped |
| v0.5 | Hardware I/O: receipt printer, cash drawer, scanner | Next |
| v1.0 | Integrated payments | Planned |
| v1.5 | Multi-store | Planned |
| v2.0 | Enterprise analytics | Planned |

Unshipped milestones are deliberately undated. There is no delivery team
behind this, and dates on it would be decoration.

### Known gaps

- No authentication. The device is trusted and `user_id` comes from config,
  so anyone at the till can ring a sale. The schema is shaped for PIN-based
  cashier switching, but it is not implemented.
- Receipt numbers use timestamp milliseconds rather than a per-device
  counter, so they are collision-resistant but not ordered.
- Cloud notification topic filtering is unimplemented: every subscriber to a
  store receives every notification for it.
- The sync agent sends its `Hello` handshake once, from a task that breaks on
  first connect, so a SECONDARY that drops and reconnects is never
  re-registered by the hub.
- 24 integration tests are `#[ignore]`d because they need a live cloud-api or
  a provisioned database.

---

## License

MIT. See [LICENSE](LICENSE).
