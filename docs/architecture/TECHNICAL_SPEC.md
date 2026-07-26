# Titan POS v0.1 - Technical Specifications

> The parameters v0.1 was built to. Where the shipped code and this document
> disagree, the code is authoritative — see `crates/titan-core` for the money
> and validation rules.

---

## Confirmed Technical Decisions

### Core Configuration

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Tenant Model | Multi-tenant schema, single runtime | Future-proof |
| User Auth (v0.1) | Device-level (no login) | Simplicity for MVP |
| Tax Mode | Configurable (default: exclusive) | Global flexibility |
| Discount Order | Discount before tax | Accounting standard |
| Inventory Tracking | Configurable per product | Flexibility |
| Receipt Format | `YYYYMMDD-Device-Seq` | Human-readable |
| Sync Notification | Silent (audit log only) | UX simplicity |
| Offline Duration | Unlimited with warnings | Never block sales |
| Data Retention | Rolling 90 days local | Balance storage/history |
| Target Platforms | macOS ARM + Windows 10/11 | Primary markets |
| Currency | Single per tenant | Simplicity |
| Logging | JSON + optional telemetry | Cloud-ready |

### Database Migration Tool

**Choice**: `sqlx` with embedded migrations

```toml
# Cargo.toml
[dependencies]
sqlx = { version = "0.7", features = ["runtime-tokio", "sqlite", "postgres", "migrate"] }
```

**Why sqlx?**
- Compile-time SQL verification
- Works with both SQLite and PostgreSQL
- Migrations embedded in binary (no runtime files)
- Async-native with Tokio

---

## Payment Processing (v0.1)

### Mock Payment Flow

For v0.1, all payments are **simulated locally**. No real payment gateway integration.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentMethod {
    Cash,           // Physical cash
    ExternalCard,   // Customer paid on external terminal
    // Future: IntegratedCard, MobileWallet, QrCode
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payment {
    pub id: String,           // UUID v4
    pub sale_id: String,      // FK to sales
    pub method: PaymentMethod,
    pub amount_cents: i64,
    pub reference: Option<String>,  // External reference (future)
    pub created_at: String,   // ISO8601
}
```

### Exposed Commands (Mockable)

```rust
#[tauri::command]
pub async fn process_payment(
    state: State<'_, AppState>,
    sale_id: String,
    method: PaymentMethod,
    amount_cents: i64,
) -> Result<PaymentResult, ApiError> {
    // v0.1: Just record the payment locally
    // v1.0+: Call payment gateway based on method
    
    let payment = Payment {
        id: Uuid::new_v4().to_string(),
        sale_id,
        method,
        amount_cents,
        reference: None,
        created_at: Utc::now().to_rfc3339(),
    };
    
    state.db.insert_payment(&payment).await?;
    
    Ok(PaymentResult {
        success: true,
        payment_id: payment.id,
        change_due_cents: calculate_change(state, &sale_id).await?,
    })
}
```

### Future Payment Integration (v1.0+)

| Region | Provider | Integration Type |
|--------|----------|------------------|
| USA | Stripe Terminal | Semi-integrated SDK |
| Europe | Stripe Terminal / Adyen | Semi-integrated SDK |
| Pakistan | JazzCash / EasyPaisa | REST API + QR |
| India | Razorpay | REST API |
| SE Asia | GrabPay | REST API + QR |

---

## Omni-Search Explained

### What is Omni-Search?

A **unified search input** that searches across all product identifiers simultaneously:
- SKU (e.g., "COKE-330")
- Product name (e.g., "Coca-Cola 330ml")
- Barcode (e.g., "5449000000996")

### Why "Omni"?

Because one search bar finds everything. No switching between "search by SKU" and "search by name".

### Technical Implementation

```sql
-- FTS5 virtual table for instant search
CREATE VIRTUAL TABLE products_fts USING fts5(
    sku, 
    name, 
    barcode,
    content='products', 
    content_rowid='rowid'
);

-- Search query (< 10ms for 50,000 products)
SELECT p.* 
FROM products p
WHERE p.rowid IN (
    SELECT rowid FROM products_fts 
    WHERE products_fts MATCH ?
)
LIMIT 20;
```

### User Experience

```
┌─────────────────────────────────────────────────────────┐
│  🔍 Search products...                               × │
├─────────────────────────────────────────────────────────┤
│  User types: "coke"                                     │
│                                                         │
│  Results appear instantly:                              │
│  ┌─────────────────────────────────────────────────┐   │
│  │ COKE-330    Coca-Cola 330ml         $2.50      │   │
│  │ COKE-500    Coca-Cola 500ml         $3.50      │   │
│  │ COKE-ZERO   Coca-Cola Zero 330ml    $2.50      │   │
│  │ COKE-DIET   Diet Coke 330ml         $2.50      │   │
│  └─────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

---

## Containerization Strategy

### Development Environment

```bash
# Start PostgreSQL + Redis
docker compose up -d

# Access database admin
open http://localhost:8080  # Adminer
```

### What's Containerized

| Component | Container | Port |
|-----------|-----------|------|
| PostgreSQL 16 | `titan-postgres` | 5432 |
| Redis 7 | `titan-redis` | 6379 |
| Adminer (dev) | `titan-adminer` | 8080 |

### What's NOT Containerized

| Component | Reason |
|-----------|--------|
| Tauri Desktop App | Runs natively on POS hardware |
| SQLite (local) | Embedded in Tauri app |

### Future Cloud Services (v1.0+)

```
┌─────────────────────────────────────────────────────────┐
│                   KUBERNETES CLUSTER                    │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐    │
│  │  API (x3)   │  │ Sync (x2)   │  │ Jobs (x1)   │    │
│  │  Rust/Axum  │  │ WebSocket   │  │ Background  │    │
│  └─────────────┘  └─────────────┘  └─────────────┘    │
│         │                │                │            │
│         └────────────────┼────────────────┘            │
│                          │                             │
│  ┌───────────────────────┴───────────────────────┐    │
│  │              PostgreSQL (RDS/Cloud SQL)        │    │
│  └───────────────────────────────────────────────┘    │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

---

## Project Structure (Final)

```
titan-pos/
├── .github/
│   └── workflows/
│       └── ci.yml
├── .sqlx/                    # sqlx offline query cache (tracked)
├── crates/
│   ├── titan-core/           # Pure business logic
│   ├── titan-db/             # Database layer
│   └── titan-sync/           # Sync engine
├── apps/
│   ├── desktop/              # Tauri application
│   │   ├── src-tauri/        # Rust backend + its own package.json
│   │   └── src/              # SolidJS frontend
│   └── cloud-api/            # Cloud gRPC server
├── migrations/
│   ├── sqlite/               # Local DB migrations
│   └── postgres/             # Cloud DB migrations
├── proto/                    # Protobuf definitions
├── scripts/                  # Multi-instance dev helpers
├── docs/
│   ├── architecture/
│   └── CONTRIBUTING.md
├── docker-compose.yml
├── Cargo.toml                # Workspace root
├── Cargo.lock                # Tracked: every member is a binary
└── README.md
```

There is no root `package.json`. The only npm workspace is
`apps/desktop`, which is where `pnpm install` and `pnpm build` run.
