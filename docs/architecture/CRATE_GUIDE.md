# Titan POS: Crate Responsibility Guide

> **Version**: 0.1.0  
> **Last Updated**: January 31, 2026

This document defines the responsibilities and boundaries of each Rust crate in the workspace.

---

## Crate Dependency Hierarchy

```
Level 0 (No deps):     titan-core
Level 1 (Core only):   titan-db, titan-sync
Level 2 (Multiple):    titan-tauri (app)
Future:                titan-hal, titan-fiscal
```

**Rule**: A crate may only depend on crates at a lower level.

---

## titan-core

### Purpose
The **pure business logic** crate. Contains domain types, calculations, and validation rules. **Zero I/O operations.**

### Responsibilities
- Define domain types (`Money`, `Quantity`, `TaxRate`, `Discount`)
- Cart calculations (subtotals, taxes, totals)
- Discount application logic
- Transaction state transitions
- Business rule validation

### Dependencies (Minimal)
```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
thiserror = "1.0"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1.0", features = ["v4", "serde"] }
```

### What Belongs Here
```rust
// ✅ YES: Pure types
pub struct Money(i64);
pub struct CartItem { product_id: String, qty: i32, unit_price: Money }

// ✅ YES: Pure calculations
pub fn calculate_line_total(price: Money, qty: i32) -> Money;
pub fn apply_discount(subtotal: Money, discount: &Discount) -> Money;

// ✅ YES: Validation
pub fn validate_quantity(qty: i32) -> Result<(), ValidationError>;
```

### What Does NOT Belong Here
```rust
// ❌ NO: Database operations
pub async fn get_product(id: &str) -> Product; // WRONG

// ❌ NO: Network calls
pub async fn sync_to_cloud() -> Result<()>; // WRONG

// ❌ NO: File I/O
pub fn load_config(path: &str) -> Config; // WRONG
```

---

## titan-db

### Purpose
Database abstraction layer. Handles SQLite (local) and PostgreSQL (cloud) operations.

### Responsibilities
- Connection pool management
- Query execution
- Schema migrations
- Repository implementations
- Transaction management

### Dependencies
```toml
[dependencies]
titan-core = { path = "../titan-core" }
sqlx = { version = "0.7", features = ["runtime-tokio", "sqlite"] }
tokio = { version = "1.0", features = ["sync"] }
tracing = "0.1"
```

### Structure
```
titan-db/src/
├── lib.rs
├── sqlite/
│   ├── mod.rs
│   ├── connection.rs      # Pool creation, config
│   ├── migrations.rs      # Embedded migrations
│   └── repository/
│       ├── mod.rs
│       ├── product.rs     # ProductRepository
│       ├── sale.rs        # SaleRepository
│       └── sync.rs        # SyncOutboxRepository
└── traits.rs              # Repository traits
```

### Repository Pattern
```rust
// traits.rs - Define the interface
#[async_trait]
pub trait ProductRepository {
    async fn get_by_id(&self, id: &str) -> Result<Option<Product>, DbError>;
    async fn search(&self, query: &str, limit: u32) -> Result<Vec<Product>, DbError>;
    async fn insert(&self, product: &NewProduct) -> Result<Product, DbError>;
    async fn update(&self, product: &Product) -> Result<(), DbError>;
}

// sqlite/repository/product.rs - SQLite implementation
pub struct SqliteProductRepository { pool: SqlitePool }

#[async_trait]
impl ProductRepository for SqliteProductRepository {
    async fn get_by_id(&self, id: &str) -> Result<Option<Product>, DbError> {
        sqlx::query_as!(Product, "SELECT * FROM products WHERE id = ?", id)
            .fetch_optional(&self.pool)
            .await
            .map_err(DbError::from)
    }
}
```

---

## titan-sync

### Purpose
Handles data synchronization between local SQLite and cloud PostgreSQL using CRDTs and the Outbox pattern.

### Responsibilities
- CRDT implementations (G-Counter, Delta-State)
- Outbox queue management
- WebSocket client for cloud communication
- Conflict resolution logic
- Sync state machine

### Dependencies
```toml
[dependencies]
titan-core = { path = "../titan-core" }
tokio = { version = "1.0", features = ["sync", "time", "net"] }
tokio-tungstenite = "0.21"
prost = "0.12"           # Protobuf
serde = "1.0"
tracing = "0.1"
```

### Structure
```
titan-sync/src/
├── lib.rs
├── crdt/
│   ├── mod.rs
│   ├── counter.rs        # G-Counter for inventory
│   └── lww_register.rs   # Last-Write-Wins for customer data
├── outbox/
│   ├── mod.rs
│   ├── manager.rs        # Queue operations
│   └── worker.rs         # Background sync worker
└── transport/
    ├── mod.rs
    └── websocket.rs      # WebSocket client
```

### CRDT Implementation
```rust
// crdt/counter.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCounter {
    node_id: String,
    counts: HashMap<String, i64>,
}

impl GCounter {
    pub fn increment(&mut self, amount: i64) {
        let count = self.counts.entry(self.node_id.clone()).or_insert(0);
        *count += amount;
    }
    
    pub fn merge(&mut self, other: &GCounter) {
        for (node, count) in &other.counts {
            let current = self.counts.entry(node.clone()).or_insert(0);
            *current = (*current).max(*count);
        }
    }
    
    pub fn value(&self) -> i64 {
        self.counts.values().sum()
    }
}
```

---

## titan-tauri (apps/desktop/src-tauri)

### Purpose
The Tauri application layer. Thin wrapper that connects the UI to the core logic.

### Responsibilities
- Tauri command handlers
- Application state management
- Window/menu management
- Event emission to frontend
- Error translation to API responses

### Dependencies
```toml
[dependencies]
titan-core = { path = "../../crates/titan-core" }
titan-db = { path = "../../crates/titan-db" }
titan-sync = { path = "../../crates/titan-sync" }
tauri = { version = "2.0", features = ["protocol-asset"] }
tokio = { version = "1.0", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

### Structure
```
src-tauri/src/
├── main.rs               # Entry point
├── lib.rs                # Library root
├── state.rs              # AppState struct
├── error.rs              # ApiError type
└── commands/
    ├── mod.rs
    ├── inventory.rs      # search_products, get_product
    ├── transaction.rs    # create_sale, add_item, finalize
    └── system.rs         # health_check, get_config
```

### Command Handler Pattern
```rust
// commands/inventory.rs
use crate::{state::AppState, error::ApiError};
use titan_core::types::Product;
use tauri::State;

#[tauri::command]
pub async fn search_products(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<ProductDto>, ApiError> {
    // 1. Validate input
    let query = query.trim();
    if query.len() < 2 {
        return Ok(vec![]);
    }
    
    // 2. Call repository (through state)
    let products = state.product_repo
        .search(query, 20)
        .await
        .map_err(ApiError::from)?;
    
    // 3. Map to DTO
    Ok(products.into_iter().map(ProductDto::from).collect())
}
```

---

## Future Crates

### titan-hal (Hardware Abstraction Layer)

**Purpose**: Isolate hardware-specific code from business logic.

**Feature Flags**:
```toml
[features]
default = []
printer_escpos = ["serialport"]
scanner_hid = ["hidapi"]
drawer_pulse = []
```

**Structure**:
```
titan-hal/src/
├── lib.rs
├── printer/
│   ├── mod.rs
│   ├── traits.rs         # PrinterDriver trait
│   └── escpos.rs         # ESC/POS implementation
├── scanner/
│   ├── mod.rs
│   └── hid.rs            # HID scanner
└── drawer/
    └── mod.rs            # Cash drawer control
```

### titan-fiscal (Regional Compliance)

**Purpose**: Tax/fiscal compliance modules per country.

**Feature Flags**:
```toml
[features]
default = []
fiscal_de = []  # Germany TSE
fiscal_it = []  # Italy RT
fiscal_fr = []  # France NF525
```

---

## Testing Strategy

### titan-core (Unit Tests)
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_money_addition() {
        let a = Money::from_cents(100);
        let b = Money::from_cents(50);
        assert_eq!((a + b).cents(), 150);
    }
}
```

### titan-db (Integration Tests)
```rust
// tests/sqlite_integration.rs
#[tokio::test]
async fn test_product_search_fts() {
    let db = setup_test_db().await;
    
    // Insert test data
    db.insert_product(test_product()).await.unwrap();
    
    // Test FTS
    let results = db.search_products("test", 10).await.unwrap();
    assert_eq!(results.len(), 1);
}
```

### titan-tauri (E2E Tests)
```rust
// Use Tauri's test utilities
#[cfg(test)]
mod tests {
    use tauri::test::{mock_builder, MockRuntime};
    
    #[tokio::test]
    async fn test_search_command() {
        let app = mock_builder().build().unwrap();
        let result: Vec<ProductDto> = app
            .invoke("search_products", json!({ "query": "test" }))
            .await
            .unwrap();
        // Assert
    }
}
```

---

## Code Review Checklist

When reviewing changes to any crate:

- [ ] Does `titan-core` have any I/O? (Should be NO)
- [ ] Are all money operations using `Money` type?
- [ ] Are UUIDs generated with `uuid::Uuid::new_v4()`?
- [ ] Are errors using `thiserror`, not strings?
- [ ] Are database queries using `sqlx` macros for compile-time checks?
- [ ] Are Tauri commands returning `Result<T, ApiError>`?
- [ ] Are there tests for the new functionality?

---

*This guide is the authoritative reference for crate responsibilities.*
