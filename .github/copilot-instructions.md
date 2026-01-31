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

## 📚 Documentation & Comments (CRITICAL)

> **Learning-First Approach**: The developer is learning this tech stack (Rust, Tauri, SolidJS, sqlx).
> Write code as if teaching someone who understands programming but is new to these specific technologies.

### Comment Requirements

#### 1. Function-Level Documentation
Every function MUST have a doc comment explaining:
- **What** it does (brief summary)
- **Where** it's used in the user workflow / project architecture
- **Why** it exists (business reason)
- **How** it works (if non-obvious)

```rust
/// Searches products using SQLite FTS5 full-text search.
///
/// # User Workflow
/// ```text
/// ┌─────────────────────────────────────────────────────────────┐
/// │  POS Screen                                                 │
/// │  ┌─────────────────────────────────────────────────────┐   │
/// │  │ 🔍 Search: "coke"                                   │   │
/// │  └─────────────────────────────────────────────────────┘   │
/// │           │                                                 │
/// │           ▼ (debounced 150ms)                              │
/// │  ┌─────────────────────────────────────────────────────┐   │
/// │  │ invoke('search_products', { query: 'coke' })        │   │
/// │  └─────────────────────────────────────────────────────┘   │
/// │           │                                                 │
/// │           ▼                                                 │
/// │  THIS FUNCTION: Queries FTS5 index for matching products   │
/// │           │                                                 │
/// │           ▼                                                 │
/// │  Returns: Vec<ProductDto> displayed in product grid        │
/// └─────────────────────────────────────────────────────────────┘
/// ```
///
/// # Arguments
/// * `query` - Search term (searches SKU, name, barcode)
/// * `limit` - Maximum results to return (default: 20)
///
/// # Returns
/// Products matching the search, ordered by relevance
///
/// # Performance
/// - Target: <10ms for 50,000 products
/// - Uses FTS5 MATCH query, not LIKE (which would be slow)
pub async fn search_products(&self, query: &str, limit: u32) -> Result<Vec<Product>, DbError> {
    // ... implementation
}
```

#### 2. Module-Level Documentation
Every `mod.rs` or top-level module file MUST explain:
- The module's responsibility
- How it fits in the crate hierarchy
- Key types/functions exported

```rust
//! # Cart Module
//!
//! Manages shopping cart state and calculations for the POS system.
//!
//! ## Architecture Position
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    titan-core (this crate)                  │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
//! │  │   types/    │  │   cart/ ◄───┼──│ YOU ARE HERE        │ │
//! │  │  (Money,    │  │  (CartItem, │  │ Pure cart logic     │ │
//! │  │   Qty)      │  │   totals)   │  │ No I/O allowed      │ │
//! │  └─────────────┘  └─────────────┘  └─────────────────────┘ │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Types
//! - [`Cart`] - The shopping cart container
//! - [`CartItem`] - Individual line item with quantity
//! - [`CartTotals`] - Computed subtotal, tax, total
//!
//! ## Usage
//! ```rust
//! let mut cart = Cart::new();
//! cart.add_item(product, 2)?;
//! let totals = cart.calculate_totals(tax_rate);
//! ```
```

#### 3. Inline Comments for Complex Logic
Add comments explaining **WHY**, not just **WHAT**:

```rust
// ✅ GOOD: Explains the business reason
// Bankers Rounding: Round 0.5 to nearest even number
// This prevents systematic bias in large datasets
// Required for financial compliance in most jurisdictions
let tax_cents = (amount_cents as i128 * rate_bps as i128 + 5000) / 10000;

// ❌ BAD: Just restates the code
// Add 5000 and divide by 10000
let tax_cents = (amount_cents as i128 * rate_bps as i128 + 5000) / 10000;
```

#### 4. ASCII Diagrams for Data Flow
Use ASCII diagrams to visualize:
- Request/response flows
- State transitions
- Data transformations

```rust
/// Processes a payment and updates sale status.
///
/// # Payment Flow
/// ```text
///                    ┌──────────────────┐
///                    │   Tender Modal   │
///                    │   Amount: $25.00 │
///                    │   [Pay Cash]     │
///                    └────────┬─────────┘
///                             │
///                             ▼
///              ┌──────────────────────────────┐
///              │  process_payment() ◄─────────┼── YOU ARE HERE
///              └──────────────┬───────────────┘
///                             │
///         ┌───────────────────┼───────────────────┐
///         ▼                   ▼                   ▼
///   ┌──────────┐       ┌──────────┐       ┌──────────┐
///   │ Validate │       │ Record   │       │ Update   │
///   │ Amount   │       │ Payment  │       │ Sale     │
///   └──────────┘       └──────────┘       └──────────┘
///                             │
///                             ▼
///              ┌──────────────────────────────┐
///              │  If paid >= due:             │
///              │    → Finalize sale           │
///              │    → Generate receipt        │
///              │    → Queue for sync          │
///              └──────────────────────────────┘
/// ```
```

#### 5. TypeScript/SolidJS Comments
Same standards apply to frontend code:

```typescript
/**
 * ProductCard Component
 * 
 * Displays a single product in the search results grid.
 * 
 * ## User Interaction Flow
 * ```
 * ┌─────────────────────────────────────────────┐
 * │  Search Results Grid                        │
 * │  ┌─────────┐ ┌─────────┐ ┌─────────┐       │
 * │  │ Product │ │ Product │ │ Product │       │
 * │  │  Card   │ │  Card◄──┼─┼─ Click  │       │
 * │  └─────────┘ └────┬────┘ └─────────┘       │
 * │                   │                         │
 * │                   ▼                         │
 * │         onSelect(product) called            │
 * │                   │                         │
 * │                   ▼                         │
 * │         Product added to cart               │
 * └─────────────────────────────────────────────┘
 * ```
 * 
 * @param product - The product data to display
 * @param onSelect - Callback when user clicks the card
 */
export const ProductCard: Component<ProductCardProps> = (props) => {
  // ...
};
```

### Comment Checklist
Before committing code, ensure:
- [ ] Every public function has a doc comment with workflow context
- [ ] Every module has a header explaining its purpose
- [ ] Complex algorithms have inline comments explaining WHY
- [ ] Data flows are visualized with ASCII diagrams where helpful
- [ ] Error cases are documented with examples

---

## 🔍 Critical Implementation Pitfalls & Prevention

> **Learn from Experience**: First milestone debugging revealed these pitfalls. Use this section to avoid them.

### 1. Tauri State<T> Method Shadowing (CRITICAL)

**The Problem:**
```rust
// ❌ WRONG: This compiles but calls the wrong method!
#[tauri::command]
pub async fn my_command(db: State<'_, DbState>) -> Result<(), ApiError> {
    // db.inner() calls State::inner() which returns &DbState
    let db_inner: &DbState = db.inner();  // WRONG TYPE!
    
    // Trying to call DbState::inner() fails - it's shadowed
    let database: &Database = db_inner.inner(); // Won't work as expected
    Ok(())
}
```

**Why It Happens:**
- Tauri's `State<T>` type implements `Deref<Target=T>` AND has its own `inner(&self) -> &T` method
- When you call `db.inner()`, the `State::inner()` method is called, NOT `DbState::inner()`
- This is a subtle API gotcha that breaks type inference

**The Solution:**
```rust
// ✅ CORRECT: Dereference State first, then call DbState::inner()
#[tauri::command]
pub async fn my_command(db: State<'_, DbState>) -> Result<(), ApiError> {
    // (*db) dereferences to &DbState, then .inner() calls DbState::inner()
    let db_inner: &Database = (*db).inner();  // CORRECT!
    
    // Now you can use db_inner as &Database
    db_inner.products().search(query, limit).await?;
    Ok(())
}
```

**Prevention:**
- Always explicitly dereference State before calling wrapped type methods: `(*state).method()`
- Use explicit type annotations to catch mismatches: `let db: &Database = ...`
- Run `cargo check` immediately after writing State-dependent code
- Document this pattern in function comments

---

### 2. Repository Return Types Are Not Always What You'd Expect

**The Problem:**
```rust
// ❌ WRONG: Assumes finalize_sale() returns Sale
#[tauri::command]
pub async fn finalize_sale(db: State<'_, DbState>, sale_id: String) -> Result<ReceiptResponse, ApiError> {
    let db_inner: &Database = (*db).inner();
    
    // This returns (), not Sale!
    let sale = db_inner.sales().finalize_sale(&sale_id).await?;  // ERROR: sale is ()
    
    let receipt = ReceiptResponse::from(sale);
    Ok(receipt)
}
```

**Why It Happens:**
- Database operations like `finalize_sale()` often return `()` to indicate success
- They modify state and return nothing, expecting you to refetch if needed
- API documentation might not be immediately obvious
- This is similar to patterns in other databases (e.g., `execute()` vs `query()`)

**The Solution:**
```rust
// ✅ CORRECT: Call finalize_sale(), then refetch the updated sale
#[tauri::command]
pub async fn finalize_sale(db: State<'_, DbState>, sale_id: String) -> Result<ReceiptResponse, ApiError> {
    let db_inner: &Database = (*db).inner();
    
    // Finalize returns (), confirming the operation succeeded
    db_inner.sales().finalize_sale(&sale_id).await?;
    
    // Now refetch the updated sale
    let sale = db_inner.sales().get_by_id(&sale_id).await?
        .ok_or_else(|| ApiError::not_found("Sale", &sale_id))?;
    
    let receipt = ReceiptResponse::from(sale);
    Ok(receipt)
}
```

**Prevention:**
- Always check repository method signatures in `titan-db/src/repository/`
- When in doubt, assume mutating methods return `()` and refetch
- Use Context7 to fetch sqlx documentation for query patterns
- Add unit tests that verify return types immediately after implementation

---

### 3. JSON Payload Serialization Type Mismatches

**The Problem:**
```rust
// ❌ WRONG: Passing serde_json::Value instead of &str
#[tauri::command]
pub async fn queue_for_sync(db: State<'_, DbState>, entity: Entity) -> Result<(), ApiError> {
    let db_inner: &Database = (*db).inner();
    
    let payload = serde_json::to_value(&entity)?;  // Returns Value
    db_inner.sync_outbox().queue_for_sync("ENTITY", &entity.id, &payload).await?;
    // ERROR: queue_for_sync expects &str, not &Value
    
    Ok(())
}
```

**Why It Happens:**
- `serde_json::to_value()` creates a `Value` object, not a string
- `serde_json::to_string()` creates a `String` that can be referenced as `&str`
- Easy to mix up because both are serialization functions
- Type mismatch isn't caught until runtime or compilation

**The Solution:**
```rust
// ✅ CORRECT: Use to_string() for &str payload
#[tauri::command]
pub async fn queue_for_sync(db: State<'_, DbState>, entity: Entity) -> Result<(), ApiError> {
    let db_inner: &Database = (*db).inner();
    
    // to_string() returns String, which is then referenced as &str
    let payload = serde_json::to_string(&entity).unwrap_or_default();
    db_inner.sync_outbox().queue_for_sync("ENTITY", &entity.id, &payload).await?;
    
    Ok(())
}
```

**Prevention:**
- Check the exact parameter type in repository method signatures
- Remember: `to_value()` → `Value`, `to_string()` → `String`
- Add comments explaining why you chose `to_string()` vs `to_value()`
- Use explicit type annotations: `let payload: &str = ...`

---

### 4. UI Color Palette Incompleteness

**The Problem:**
```css
/* ❌ WRONG: Using color shades that don't exist in Tailwind config */
.btn-success {
  @apply btn bg-success-600 text-white
         hover:bg-success-700 active:bg-success-800
         focus-visible:ring-success-500;
  /* ERROR: success-700, success-800, success-500 don't exist */
}
```

**Why It Happens:**
- Initial Tailwind config often has incomplete color scales
- Only a few shades (50, 500, 600) are defined, not the full range
- CSS preprocessor can't find the missing classes at compile time
- Vite/PostCSS throws an error instead of applying the style

**The Solution:**
```javascript
// ✅ CORRECT: Define complete color scales (50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950)
export default {
  theme: {
    extend: {
      colors: {
        success: {
          50:  '#f0fdf4',
          100: '#dcfce7',
          200: '#bbf7d0',
          300: '#86efac',
          400: '#4ade80',
          500: '#22c55e',  // Base color
          600: '#16a34a',
          700: '#15803d',  // For hover states
          800: '#166534',
          900: '#14532d',
          950: '#0a2e1b',
        },
        // ... repeat for warning, danger, etc.
      },
    },
  },
};
```

**Prevention:**
- Always define complete Tailwind color scales (all 11 shades)
- Reference Material Design 3 color systems for inspiration
- Test CSS compilation early: `pnpm dev` or `cargo build`
- Include all states: base (500), hover (600-700), active (700-800), focus-ring (400-500)

---

### 5. Database Migration Timing Issues

**The Problem:**
```rust
// ❌ WRONG: Assume database is initialized before writing code
#[tokio::main]
async fn main() {
    let db = Database::connect().await?;
    
    // If migrations haven't run, tables don't exist yet
    let products = db.products().list().await?;  // Might fail!
}
```

**Why It Happens:**
- Migrations must run before any queries against migrated tables
- Automated migrations might not run if the database already exists
- Different developers might have different database states
- New migration files need to be explicitly applied

**The Solution:**
```rust
// ✅ CORRECT: Explicitly run migrations before any database operations
#[tokio::main]
async fn main() {
    let db = Database::connect().await?;
    
    // Run migrations first - this is idempotent and safe to call multiple times
    db.run_migrations().await?;
    
    // Now database is guaranteed to be up-to-date
    let products = db.products().list().await?;
}
```

**Prevention:**
- Always call `db.run_migrations()` in initialization code
- Run `DATABASE_URL=... cargo check` before making code changes
- Keep migration files in version control
- Document the migration workflow in `CONTRIBUTING.md`

---

### 6. Type Annotation in Complex Generic Chains

**The Problem:**
```rust
// ❌ WRONG: Missing type annotations in complex expressions
#[tauri::command]
pub async fn my_command(
    db: State<'_, DbState>,
    cart: State<'_, CartState>,
) -> Result<Vec<ProductDto>, ApiError> {
    let db_inner = (*db).inner();  // Rust has to infer this
    
    let products = db_inner.products()  // Type inference gets confused
        .search(query, limit).await?
        .into_iter()
        .map(ProductDto::from)
        .collect();
    
    Ok(products)
}
```

**Why It Happens:**
- Rust's type inference can get confused with long method chains
- State dereferencing + complex method chains = unclear types
- Compiler errors are hard to interpret
- Code looks correct but won't compile

**The Solution:**
```rust
// ✅ CORRECT: Add explicit type annotations at key points
#[tauri::command]
pub async fn my_command(
    db: State<'_, DbState>,
    cart: State<'_, CartState>,
) -> Result<Vec<ProductDto>, ApiError> {
    // Explicit type annotation immediately after dereferencing
    let db_inner: &Database = (*db).inner();
    
    // Now the chain is clear and type-checkable
    let products: Vec<ProductDto> = db_inner.products()
        .search(query, limit).await?
        .into_iter()
        .map(ProductDto::from)
        .collect();
    
    Ok(products)
}
```

**Prevention:**
- Always use explicit type annotations for State dereferencing
- Add type annotations after complex operations (`.await?`, `.into_iter()`, etc.)
- Use IDE hints (`hover to see type`) to verify types match expectations
- When a type error occurs, add intermediate type annotations to isolate the issue

---

## Validation Checklist Before Implementation

Before writing code for any command or complex function:

- [ ] **API Check**: Verified return types of repository methods in `titan-db/src/repository/`
- [ ] **State Dereferencing**: Used `(*state).method()` for any State-wrapped types
- [ ] **Type Annotations**: Added explicit types after State dereferencing and complex operations
- [ ] **Serialization**: Confirmed using `to_string()` (→ &str) vs `to_value()` (→ Value)
- [ ] **Colors**: All Tailwind color shades (50, 100, ... 950) defined if extending colors
- [ ] **Migrations**: Ran migrations and verified database state with `cargo check`
- [ ] **Error Handling**: All repository calls use `?` and return `Result`
- [ ] **Tests**: Quick local test of command with `pnpm tauri dev` before committing

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
9. **No functions without doc comments** - Always document
10. **No complex logic without inline comments explaining WHY**

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
