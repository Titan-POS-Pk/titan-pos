# Titan POS

> **Offline-First Point of Sale System**  
> Built with Rust + Tauri v2 + SolidJS + SQLite

[![Build Status](https://img.shields.io/badge/build-pending-yellow)]()
[![License](https://img.shields.io/badge/license-proprietary-red)]()

---

## Overview

Titan POS is a mission-critical Point of Sale system designed for **offline-first operation**. The local SQLite database is the single source of truth. Cloud sync is a background side-effect, not a prerequisite.

### Key Features (v0.1 - Logical Core)

- ✅ **Offline-First**: Operates indefinitely without internet
- ✅ **Integer Math**: All monetary calculations in cents (no floating point)
- ✅ **Dual-Key Identity**: UUID (system) + SKU (business)
- ✅ **Sub-10ms Search**: Full-text search across 50,000+ products
- ✅ **CRDT Sync**: Conflict-free inventory synchronization

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
| [Copilot Instructions](.github/copilot-instructions.md) | AI coding guidelines |

---

## Core Principles

### 1. Integer Math Only
```rust
// ✅ Correct: Use cents
let price = Money::from_cents(1099); // $10.99

// ❌ Wrong: Never use floats
let price = 10.99; // FORBIDDEN
```

### 2. Dual-Key Identity
```sql
-- Every entity has both:
id TEXT PRIMARY KEY,  -- UUID v4 (immutable, for FK)
sku TEXT UNIQUE,      -- Business ID (mutable, for humans)
```

### 3. Local-First
```rust
// All operations complete locally first
let sale = create_sale(&local_db, cart).await?;

// Sync is a background side-effect
sync_outbox.queue(sale).await;
```

### 4. CRDT for Sync
```rust
// Send deltas, not absolutes
sync_message = InventoryDelta { 
    product_id: "...", 
    change: -3  // Not "stock = 7"
};
```

---

## Roadmap

| Version | Focus | Target |
|---------|-------|--------|
| v0.1 | Logical Core | Q1 2026 |
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
