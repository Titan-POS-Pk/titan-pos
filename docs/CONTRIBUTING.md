# Contributing to Titan POS

> **Version**: 0.1.0  
> **Last Updated**: January 31, 2026

---

## Development Environment Setup

### Prerequisites

| Tool | Version | Installation |
|------|---------|--------------|
| Rust | 1.75+ | [rustup.rs](https://rustup.rs) |
| Node.js | 20+ | [nodejs.org](https://nodejs.org) |
| pnpm | 8+ | `npm install -g pnpm` |
| Tauri CLI | 2.0+ | `cargo install tauri-cli` |

### macOS Additional Requirements

```bash
# Xcode Command Line Tools
xcode-select --install
```

### Windows Additional Requirements

```powershell
# Visual Studio Build Tools
# Download from: https://visualstudio.microsoft.com/visual-cpp-build-tools/
# Select "Desktop development with C++"
```

### First-Time Setup

```bash
# Clone the repository
git clone https://github.com/Titan-POS-Pk/titan-pos.git
cd titan-pos

# Install Node dependencies and build the frontend.
# This has to happen before any cargo build: tauri_build checks that
# `frontendDist` exists, and the Rust side will not compile until it does.
cd apps/desktop
pnpm install
pnpm build

# Build the Rust workspace. No database setup is needed — sqlx verifies
# queries against the committed .sqlx/ cache.
cd ../..
cargo build --workspace

# Verify setup
cd apps/desktop && pnpm tauri dev
```

---

## Development Workflow

### Branch Naming

| Type | Format | Example |
|------|--------|---------|
| Feature | `feature/{ticket}-{description}` | `feature/TP-42-omni-search` |
| Bug Fix | `fix/{ticket}-{description}` | `fix/TP-99-cart-total-rounding` |
| Hotfix | `hotfix/{description}` | `hotfix/critical-tax-calc` |
| Docs | `docs/{description}` | `docs/api-reference` |

### Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

**Types**: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`

**Examples**:
```
feat(cart): add discount application logic
fix(db): correct FTS trigger for product updates
docs(api): document search_products command
test(core): add tax calculation edge cases
```

### Pull Request Process

1. Create feature branch from `main`
2. Make changes with tests
3. Run what CI runs: `cargo test --workspace`,
   `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all -- --check`, and `pnpm check && pnpm lint` in
   `apps/desktop`
4. Open PR with description template
5. Request review (see review requirements below)
6. Squash merge after approval

---

## Code Standards

### Rust

#### Formatting
```bash
# Format all Rust code
cargo fmt

# Check without modifying
cargo fmt --check
```

#### Linting
```bash
# Run Clippy with strict settings
cargo clippy -- -D warnings
```

#### Must-Follow Rules

1. **No `unwrap()` in application code**
   ```rust
   // ❌ Wrong
   let product = get_product(id).unwrap();
   
   // ✅ Correct
   let product = get_product(id)?;
   ```

2. **No `panic!()` in library code**
   ```rust
   // ❌ Wrong
   panic!("Invalid state");
   
   // ✅ Correct
   return Err(Error::InvalidState);
   ```

3. **Use `thiserror` for errors**
   ```rust
   #[derive(Debug, thiserror::Error)]
   pub enum CartError {
       #[error("Product not found: {0}")]
       ProductNotFound(String),
   }
   ```

4. **Integer math for money**
   ```rust
   // ❌ Wrong
   let total: f64 = 10.99 * 3.0;
   
   // ✅ Correct
   let total = Money::from_cents(1099) * 3;
   ```

### TypeScript

#### Formatting & Linting
```bash
cd apps/desktop

pnpm check     # tsc --noEmit
pnpm lint      # eslint, --max-warnings 0
pnpm format    # prettier --write
```

#### Must-Follow Rules

1. **No `any` type**
   ```typescript
   // ❌ Wrong
   function process(data: any) { }
   
   // ✅ Correct
   function process(data: ProductDto) { }
   ```

2. **Use SolidJS patterns, not React**
   ```typescript
   // ❌ Wrong (React)
   const [state, setState] = useState(initial);
   
   // ✅ Correct (SolidJS)
   const [state, setState] = createSignal(initial);
   ```

3. **Type all Tauri invocations**
   ```typescript
   // ❌ Wrong
   const result = await invoke('search_products', { query });
   
   // ✅ Correct
   const result = await invoke<Product[]>('search_products', { query });
   ```

---

## Testing

### Running Tests

```bash
# All Rust tests
cargo test

# Specific crate
cargo test -p titan-core

# With output
cargo test -- --nocapture

# Integration tests that need a live cloud-api or a provisioned database
cargo test --workspace -- --ignored
```

There is no frontend test runner yet. `apps/desktop` has `check` (tsc) and
`lint` (eslint) only, and those are what CI runs; a `pnpm test` script does
not exist. Frontend behaviour is currently covered only through the Rust
command layer.

### Test Coverage Targets

Targets, not gates — nothing in CI enforces a coverage number today.

| Component | Unit | Integration |
|-----------|------|-------------|
| titan-core | 90%+ | N/A |
| titan-db | 50%+ | 80%+ |
| titan-sync | 50%+ | 80%+ |
| Frontend | none yet | none yet |

### Writing Tests

#### Rust Unit Tests
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
    
    #[test]
    fn test_tax_calculation_bankers_rounding() {
        // Test edge case: 5.5% of $10.00
        let amount = Money::from_cents(1000);
        let tax = calculate_tax(amount, 550);
        assert_eq!(tax.cents(), 55);
    }
}
```

#### Rust Integration Tests
```rust
// tests/integration/db_test.rs
#[tokio::test]
async fn test_product_search_returns_results() {
    let db = setup_test_database().await;
    
    // Insert test product
    db.insert_product(Product {
        id: Uuid::new_v4().to_string(),
        sku: "TEST-001".into(),
        name: "Test Product".into(),
        price_cents: 1000,
        ..Default::default()
    }).await.unwrap();
    
    // Search
    let results = db.search_products("test", 10).await.unwrap();
    
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].sku, "TEST-001");
}
```

---

## Code Review

### Review Requirements

| Change Type | Approvals Needed |
|-------------|------------------|
| Database schema | 2 |
| Core business logic | 2 |
| API changes | 2 |
| Bug fixes | 1 |
| Documentation | 1 |
| UI/styling | 1 |

### Review Checklist

- [ ] Code follows project style guidelines
- [ ] Tests added/updated for changes
- [ ] No `unwrap()` or `panic!()` in application code
- [ ] Money calculations use integer math
- [ ] Database queries use `sqlx` compile-time checks
- [ ] Error handling is appropriate
- [ ] Documentation updated if needed

---

## Architecture Guidelines

### Crate Boundaries

See [CRATE_GUIDE.md](architecture/CRATE_GUIDE.md) for detailed responsibilities.

**Key Rules**:
- `titan-core` must have ZERO I/O dependencies
- Database logic stays in `titan-db`
- Tauri commands are thin wrappers only

### Adding New Features

1. Start with types in `titan-core`
2. Add repository methods in `titan-db`
3. Create Tauri commands in `apps/desktop/src-tauri`
4. Build UI components in `apps/desktop/src`

---

## Common Tasks

### Adding a New Database Table

1. Create migration in `migrations/sqlite/`:
   ```sql
   -- migrations/sqlite/003_add_customers.sql
   CREATE TABLE customers (
       id TEXT PRIMARY KEY NOT NULL,
       ...
   );
   ```

2. Add model in `titan-db/src/models.rs`

3. Add repository in `titan-db/src/sqlite/repository/`

4. Export from `titan-db/src/lib.rs`

### Adding a New Tauri Command

1. Implement in `apps/desktop/src-tauri/src/commands/`:
   ```rust
   #[tauri::command]
   pub async fn my_command(
       state: State<'_, AppState>,
       arg: String,
   ) -> Result<ResponseDto, ApiError> {
       // Implementation
   }
   ```

2. Register in `main.rs`:
   ```rust
   .invoke_handler(tauri::generate_handler![
       my_command,
       // ...
   ])
   ```

3. Create TypeScript types in `apps/desktop/src/types/`

4. Call from frontend:
   ```typescript
   const result = await invoke<ResponseDto>('my_command', { arg: 'value' });
   ```

---

## Getting Help

- **Architecture questions**: Review [docs/architecture/](architecture/)
- **Tauri command signatures**: `apps/desktop/src-tauri/src/commands/`
- **Bug reports**: Open GitHub issue with reproduction steps
- **Feature requests**: Open GitHub issue with use case
