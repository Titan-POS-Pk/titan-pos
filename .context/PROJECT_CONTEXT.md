# Titan POS - Project Context

> **Purpose**: Quick reference for AI assistants and new developers  
> **Last Updated**: January 31, 2026

---

## What is Titan POS?

A **mission-critical Point of Sale system** that operates **offline-first**. The local SQLite database is the source of truth. Cloud sync is optional.

---

## Critical Rules (NEVER VIOLATE)

### 1. Integer Math for Money
```rust
// ✅ ALWAYS use cents
Money::from_cents(1099) // $10.99

// ❌ NEVER use floats
let price: f64 = 10.99; // FORBIDDEN
```

### 2. UUID Primary Keys
```sql
-- ✅ ALWAYS UUID v4 for entity IDs
id TEXT PRIMARY KEY NOT NULL -- UUID v4

-- ❌ NEVER auto-increment for entities
id INTEGER PRIMARY KEY AUTOINCREMENT -- FORBIDDEN
```

### 3. Dual-Key Pattern
```sql
-- Every entity has:
id TEXT PRIMARY KEY,  -- Immutable system ID (UUID)
sku TEXT UNIQUE,      -- Mutable business ID (human-readable)
```

### 4. No Panics in Production
```rust
// ✅ Use Result and ?
let product = repo.get(id).await?;

// ❌ NEVER unwrap/panic
let product = repo.get(id).await.unwrap(); // FORBIDDEN
```

---

## Tech Stack Quick Reference

| Component | Technology |
|-----------|------------|
| Desktop Runtime | Tauri v2 |
| Backend Language | Rust (Edition 2021) |
| Frontend Framework | SolidJS (NOT React) |
| Local Database | SQLite (FTS5 for search) |
| Cloud Database | PostgreSQL |
| State Management | XState (FSM) |
| Styling | TailwindCSS |
| Sync Protocol | WebSocket + Protobuf |

---

## Crate Responsibilities

| Crate | Purpose | I/O Allowed |
|-------|---------|-------------|
| `titan-core` | Business logic, types, math | ❌ NO |
| `titan-db` | Database operations | ✅ YES |
| `titan-sync` | Cloud synchronization | ✅ YES |
| `titan-tauri` | Tauri commands (thin wrapper) | ✅ YES |

---

## Common Patterns

### Money Calculation
```rust
use titan_core::types::Money;

let subtotal = Money::from_cents(1000); // $10.00
let tax_rate_bps = 825; // 8.25%
let tax = subtotal.calculate_tax(tax_rate_bps);
let total = subtotal + tax;
```

### Database Query
```rust
let products = sqlx::query_as!(
    Product,
    r#"SELECT id, sku, name, price_cents 
       FROM products 
       WHERE rowid IN (SELECT rowid FROM products_fts WHERE products_fts MATCH ?)"#,
    query
)
.fetch_all(&pool)
.await?;
```

### Tauri Command
```rust
#[tauri::command]
pub async fn search_products(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<ProductDto>, ApiError> {
    state.product_repo.search(&query, 20).await
        .map(|p| p.into_iter().map(ProductDto::from).collect())
        .map_err(ApiError::from)
}
```

### Frontend Invoke
```typescript
import { invoke } from '@tauri-apps/api/core';

const products = await invoke<Product[]>('search_products', { query: 'test' });
```

---

## Database Schema (Core Tables)

```sql
-- Products with FTS
CREATE TABLE products (
    id TEXT PRIMARY KEY NOT NULL,      -- UUID v4
    sku TEXT UNIQUE NOT NULL,          -- Business ID
    name TEXT NOT NULL,
    price_cents INTEGER NOT NULL,      -- $10.00 = 1000
    tax_rate_bps INTEGER DEFAULT 0,    -- 5% = 500
    is_active BOOLEAN DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    sync_version INTEGER DEFAULT 0
);

-- Sales (aggregate)
CREATE TABLE sales (
    id TEXT PRIMARY KEY NOT NULL,
    tenant_id TEXT NOT NULL,
    total_cents INTEGER NOT NULL,
    tax_total_cents INTEGER NOT NULL,
    status TEXT NOT NULL,              -- 'DRAFT', 'COMPLETED', 'VOIDED'
    created_at TEXT NOT NULL,
    finalized_at TEXT
);

-- Sale line items
CREATE TABLE sale_items (
    id TEXT PRIMARY KEY NOT NULL,
    sale_id TEXT NOT NULL,
    product_id TEXT NOT NULL,
    sku_snapshot TEXT NOT NULL,        -- Frozen at sale time
    name_snapshot TEXT NOT NULL,
    qty INTEGER NOT NULL,
    unit_price_cents INTEGER NOT NULL,
    tax_rate_snapshot_bps INTEGER NOT NULL,
    FOREIGN KEY(sale_id) REFERENCES sales(id)
);

-- Sync outbox (for offline sync)
CREATE TABLE sync_outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    payload BLOB NOT NULL,
    status TEXT DEFAULT 'PENDING',
    created_at TEXT NOT NULL
);
```

---

## File Naming Conventions

| Type | Convention | Example |
|------|------------|---------|
| Rust modules | snake_case | `cart_manager.rs` |
| SolidJS components | PascalCase | `ProductCard.tsx` |
| TypeScript utilities | camelCase | `formatMoney.ts` |
| SQL migrations | numbered | `001_initial_schema.sql` |

---

## Error Handling

### Rust Error Types
```rust
// Domain errors (titan-core)
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("Invalid quantity: {0}")]
    InvalidQuantity(i32),
}

// DB errors (titan-db)
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Not found: {entity} with id {id}")]
    NotFound { entity: String, id: String },
}

// API errors (titan-tauri)
#[derive(Debug, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}
```

---

## Key Architectural Decisions

1. **Offline-First**: Local SQLite is truth, cloud is backup
2. **CRDT Sync**: Inventory uses delta-state CRDTs (send changes, not absolutes)
3. **Multi-Tenant Ready**: `tenant_id` in all tables from day one
4. **Outbox Pattern**: Sync queue in same transaction as business data
5. **Feature Flags**: Hardware/fiscal modules are compile-time optional

---

## Quick Commands

```bash
# Development
pnpm dev           # Start Tauri dev server

# Testing
cargo test         # Rust tests
pnpm test          # TypeScript tests

# Building
pnpm build         # Build for production
cargo build -r     # Release build

# Linting
cargo fmt          # Format Rust
cargo clippy       # Lint Rust
pnpm lint          # Lint TypeScript
```

---

## Links to Documentation

- [Architecture Decisions](docs/architecture/ARCHITECTURE_DECISIONS.md)
- [Architecture Diagrams](docs/architecture/ARCHITECTURE_DIAGRAMS.md)
- [Crate Guide](docs/architecture/CRATE_GUIDE.md)
- [Contributing](docs/CONTRIBUTING.md)
- [Copilot Instructions](.github/copilot-instructions.md)
