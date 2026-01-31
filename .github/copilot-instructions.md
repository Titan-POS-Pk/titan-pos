# Titan POS - GitHub Copilot Instructions

> **Project**: Titan POS - Offline-First Point of Sale System  
> **Architecture**: Local-First with Cloud Sync  
> **Stack**: Rust + Tauri v2 + SolidJS + SQLite

---

## 🔧 Using Context7 for Latest Documentation

When working with this project, **always use Context7 MCP** to fetch up-to-date documentation for libraries. This ensures you're using current APIs and best practices.

### How to Use Context7

1. **First, resolve the library ID:**
   ```
   Use mcp_context7_resolve-library-id with libraryName: "tauri"
   ```

2. **Then fetch documentation:**
   ```
   Use mcp_context7_get-library-docs with:
   - context7CompatibleLibraryID: "/tauri-apps/tauri" (from step 1)
   - topic: "commands" (optional, to focus on specific topic)
   ```

### Key Libraries to Query

| Library | Context7 ID | Common Topics |
|---------|-------------|---------------|
| Tauri v2 | `/tauri-apps/tauri` | commands, state, events, window |
| SolidJS | `/solidjs/solid` | signals, stores, effects, components |
| XState | `/statelyai/xstate` | machines, actors, actions |
| sqlx | `/launchbadge/sqlx` | queries, migrations, pool |
| Serde | `/serde-rs/serde` | derive, attributes, custom |

### When to Use Context7

- Before implementing any Tauri command
- When unsure about SolidJS reactive patterns
- For XState machine syntax
- For sqlx query macros
- When encountering deprecation warnings

---

## Project Overview

Titan POS is a mission-critical Point of Sale system designed for **offline-first operation**. The local SQLite database is the single source of truth. Cloud sync is a background side-effect, not a prerequisite.

### Core Principles (NEVER Violate)

1. **Integer Math Only**: All monetary values MUST be stored as integers (cents). NEVER use floating point for money.
2. **Dual-Key Identity**: Every entity has an immutable `id` (UUID v4) and a mutable business identifier (e.g., `sku`).
3. **Local-First**: All operations MUST complete successfully with zero network connectivity.
4. **CRDT for Sync**: Inventory changes are sent as deltas (e.g., `-3`), not absolute values (e.g., `stock = 7`).

---

## Tech Stack Constraints

### Rust (Backend)
- **Edition**: 2021
- **Async Runtime**: Tokio
- **Database**: `sqlx` with `runtime-tokio` and `sqlite` features
- **Error Handling**: `thiserror` for library errors, `anyhow` only in application code
- **Serialization**: `serde` with `serde_json`
- **IDs**: `uuid` crate with `v4` feature

### Frontend (SolidJS)
- **Language**: TypeScript (strict mode)
- **Styling**: TailwindCSS
- **State Machine**: XState v5
- **Build**: Vite

---

## Code Generation Rules

### Rust Guidelines

#### Money Type (CRITICAL)
```rust
// ✅ CORRECT: Use integer cents
pub struct Money(i64);

impl Money {
    pub fn from_cents(cents: i64) -> Self { Money(cents) }
    pub fn cents(&self) -> i64 { self.0 }
}

// Calculate tax (Bankers Rounding)
pub fn calculate_tax(amount: Money, rate_bps: u32) -> Money {
    let tax = (amount.cents() as i128 * rate_bps as i128 + 5000) / 10000;
    Money::from_cents(tax as i64)
}

// ❌ WRONG: Never use floats for money
let price: f64 = 10.99; // FORBIDDEN
```

#### UUID Generation
```rust
// ✅ CORRECT: Always use UUID v4 for primary keys
use uuid::Uuid;

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

// ❌ WRONG: Never use auto-increment for distributed systems
// id INTEGER PRIMARY KEY AUTOINCREMENT -- FORBIDDEN for entities
```

#### Error Handling
```rust
// ✅ CORRECT: Domain-specific errors
#[derive(Debug, thiserror::Error)]
pub enum InventoryError {
    #[error("Insufficient stock for {sku}: available {available}, requested {requested}")]
    InsufficientStock { sku: String, available: i32, requested: i32 },
    
    #[error("Product not found: {0}")]
    ProductNotFound(String),
}

// ❌ WRONG: Generic string errors
fn do_something() -> Result<(), String> { ... } // FORBIDDEN
```

#### Database Queries (sqlx)
```rust
// ✅ CORRECT: Use compile-time checked queries
let product = sqlx::query_as!(
    Product,
    r#"SELECT id, sku, name, price_cents FROM products WHERE id = ?"#,
    product_id
)
.fetch_optional(&pool)
.await?;

// ❌ WRONG: String concatenation in queries (SQL injection risk)
let query = format!("SELECT * FROM products WHERE sku = '{}'", sku); // FORBIDDEN
```

#### Tauri Commands
```rust
// ✅ CORRECT: Structured response with explicit error handling
#[tauri::command]
pub async fn search_products(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<ProductDto>, ApiError> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(vec![]);
    }
    
    let products = state.db.search_products(query, 20).await
        .map_err(|e| ApiError::from(e))?;
    
    Ok(products.into_iter().map(ProductDto::from).collect())
}

// ❌ WRONG: Panicking in commands
#[tauri::command]
pub fn bad_command() -> String {
    panic!("This will crash the app!"); // FORBIDDEN
}
```

### TypeScript/SolidJS Guidelines

#### Component Structure
```tsx
// ✅ CORRECT: Functional component with explicit types
import { Component, createSignal } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';

interface Product {
  id: string;
  sku: string;
  name: string;
  priceCents: number;
}

export const ProductCard: Component<{ product: Product; onSelect: (p: Product) => void }> = (props) => {
  const formattedPrice = () => formatMoney(props.product.priceCents);
  
  return (
    <div class="p-4 border rounded" onClick={() => props.onSelect(props.product)}>
      <h3 class="font-bold">{props.product.name}</h3>
      <p class="text-gray-600">{props.product.sku}</p>
      <p class="text-lg">{formattedPrice()}</p>
    </div>
  );
};

// ❌ WRONG: Using React patterns in SolidJS
const BadComponent = ({ product }) => {
  const [state, setState] = useState(product); // WRONG: This is React, not SolidJS
  return <div>{state.name}</div>;
};
```

#### Money Formatting (Frontend)
```typescript
// ✅ CORRECT: Format cents to display string
export function formatMoney(cents: number, currency = 'USD'): string {
  return new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency,
  }).format(cents / 100);
}

// ❌ WRONG: Performing calculations in JS
const total = item.price * item.qty; // DANGEROUS: Do this in Rust
```

#### Tauri Invoke Calls
```typescript
// ✅ CORRECT: Type-safe invoke with error handling
import { invoke } from '@tauri-apps/api/core';

interface SearchResult {
  id: string;
  sku: string;
  name: string;
  priceCents: number;
}

async function searchProducts(query: string): Promise<SearchResult[]> {
  try {
    return await invoke<SearchResult[]>('search_products', { query });
  } catch (error) {
    console.error('Search failed:', error);
    return [];
  }
}

// ❌ WRONG: Untyped invoke
const result = await invoke('search_products', { query }); // Missing type annotation
```

---

## File Naming Conventions

### Rust
- Snake_case for files: `cart_manager.rs`, `price_calculator.rs`
- Modules: `mod.rs` or `module_name.rs`
- Tests: `#[cfg(test)] mod tests { ... }` in same file, or `tests/` directory

### TypeScript/SolidJS
- PascalCase for components: `ProductCard.tsx`, `TenderModal.tsx`
- camelCase for utilities: `formatMoney.ts`, `apiClient.ts`
- kebab-case for styles: `tender-modal.css`

### SQL Migrations
- Sequential numbering: `001_initial_schema.sql`, `002_add_fts.sql`
- Descriptive names: `003_add_payment_methods.sql`

---

## Database Schema Patterns

### Required Columns for All Tables
```sql
-- Every entity table MUST have:
CREATE TABLE example (
    id TEXT PRIMARY KEY NOT NULL,      -- UUID v4
    created_at TEXT NOT NULL,          -- ISO8601 timestamp
    updated_at TEXT NOT NULL,          -- ISO8601 timestamp
    sync_version INTEGER DEFAULT 0     -- For CRDT/sync logic
);
```

### Foreign Key Pattern
```sql
-- Always reference the UUID, never the business ID
CREATE TABLE sale_items (
    product_id TEXT NOT NULL,          -- References products.id (UUID)
    sku_snapshot TEXT NOT NULL,        -- Frozen copy of SKU at sale time
    FOREIGN KEY(product_id) REFERENCES products(id)
);
```

### Full-Text Search Pattern
```sql
-- FTS5 virtual table with triggers
CREATE VIRTUAL TABLE products_fts USING fts5(sku, name, content='products', content_rowid='rowid');

CREATE TRIGGER products_ai AFTER INSERT ON products BEGIN
  INSERT INTO products_fts(rowid, sku, name) VALUES (new.rowid, new.sku, new.name);
END;
```

---

## Testing Requirements

### Rust Unit Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tax_calculation_rounds_correctly() {
        // Test Bankers Rounding: 5.5% tax on $10.00
        let amount = Money::from_cents(1000);
        let tax = calculate_tax(amount, 550); // 5.50%
        assert_eq!(tax.cents(), 55); // $0.55
    }

    #[test]
    fn test_cart_total_with_multiple_items() {
        let mut cart = Cart::new();
        cart.add_item(/* ... */);
        // Assert totals match expected values
    }
}
```

### Integration Tests
```rust
// tests/integration/sync_test.rs
#[tokio::test]
async fn test_offline_sync_preserves_data() {
    // 1. Create sale offline
    // 2. Simulate network reconnection
    // 3. Verify data synced correctly
}
```

---

## Common Patterns

### The Outbox Pattern (Sync)
```rust
// When finalizing a sale:
pub async fn finalize_sale(db: &Database, sale: Sale) -> Result<Receipt> {
    let mut tx = db.begin().await?;
    
    // 1. Insert sale record
    insert_sale(&mut tx, &sale).await?;
    
    // 2. Insert line items
    for item in &sale.items {
        insert_sale_item(&mut tx, item).await?;
    }
    
    // 3. Queue for sync (CRITICAL: same transaction)
    insert_sync_outbox(&mut tx, "SALE", &sale.id, &sale).await?;
    
    // 4. Commit atomically
    tx.commit().await?;
    
    Ok(Receipt::from(sale))
}
```

### State Machine (XState)
```typescript
// machines/posMachine.ts
import { createMachine } from 'xstate';

export const posMachine = createMachine({
  id: 'pos',
  initial: 'idle',
  states: {
    idle: {
      on: { ADD_ITEM: 'inCart' }
    },
    inCart: {
      on: {
        ADD_ITEM: 'inCart',
        CHECKOUT: 'tender',
        CLEAR: 'idle'
      }
    },
    tender: {
      on: {
        PAYMENT_COMPLETE: 'receipt',
        BACK: 'inCart'
      }
    },
    receipt: {
      on: { NEW_SALE: 'idle' }
    }
  }
});
```

---

## What NOT to Generate

1. **No `console.log` in production code** - Use proper logging
2. **No `unwrap()` in Rust application code** - Use `?` or explicit error handling
3. **No inline styles in SolidJS** - Use TailwindCSS classes
4. **No `any` type in TypeScript** - Always use explicit types
5. **No floating point for money** - Integer cents only
6. **No auto-increment IDs for entities** - UUID v4 only
7. **No synchronous database calls** - Always async
8. **No hardcoded strings for errors** - Use error types

---

## Quick Reference

| Concept | Pattern |
|---------|---------|
| Money | `i64` cents, wrapped in `Money` type |
| IDs | UUID v4 as `TEXT` primary key |
| Timestamps | ISO8601 strings (`2026-01-31T12:00:00Z`) |
| Tax rates | Basis points (`500` = 5.00%) |
| FTS | SQLite FTS5 with sync triggers |
| State | XState for UI, Rust for business |
| Errors | `thiserror` enums, never strings |
| Sync | Outbox pattern, CRDT deltas |

---

*This document is the authoritative guide for AI code generation in Titan POS.*
