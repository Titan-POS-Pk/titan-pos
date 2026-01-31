# Quick Reference: AI Code Generation Pitfalls

> **Use this card to quickly reference the 6 critical issues and their solutions**

---

## ⚡ Quick Lookup Table

| # | Issue | Wrong ❌ | Right ✅ | Prevention |
|---|-------|---------|---------|------------|
| 1 | **State<T> shadowing** | `db.inner()` | `(*db).inner()` | Type annotate: `let db_inner: &Database = ...` |
| 2 | **Return types** | Assume `Sale` returned | Refetch after mutation | Check `titan-db/src/repository/` signatures |
| 3 | **JSON serialization** | `to_value(&entity)` | `to_string(&entity)` | Parameter expects `&str` not `&Value` |
| 4 | **Colors incomplete** | `success: { 50, 500, 600 }` | `success: { 50-950 (11 shades) }` | Define all shades: 50, 100, 200, ... 950 |
| 5 | **DB not ready** | Skip migrations | Call `db.run_migrations()` first | Always migrate before queries |
| 6 | **Type inference** | `let x = ...` (no type) | `let x: Vec<T> = ...` (annotated) | Annotate after State deref & collections |

---

## 🔴 Issue #1: State<T> Method Shadowing

```rust
// ❌ WRONG - calls State::inner(), returns &DbState
let db_inner = db.inner();

// ✅ CORRECT - dereference first, then DbState::inner(), returns &Database
let db_inner: &Database = (*db).inner();
```

**Key Point**: `State<T>` has its own `inner()` method!  
**Prevention**: Always use `(*state).method()` pattern

---

## 🔴 Issue #2: Repository Return Types

```rust
// ❌ WRONG - assumes finalize_sale() returns Sale
let sale = db_inner.sales().finalize_sale(&id).await?;

// ✅ CORRECT - finalize returns (), then refetch
db_inner.sales().finalize_sale(&id).await?;
let sale = db_inner.sales().get_by_id(&id).await?
    .ok_or_else(|| ApiError::not_found("Sale", &id))?;
```

**Key Point**: Mutations return `()`, not the entity  
**Prevention**: Check method signatures in `titan-db/src/repository/`

---

## 🔴 Issue #3: JSON Serialization

```rust
// ❌ WRONG - to_value returns Value type, not String
let payload = serde_json::to_value(&sale)?;  // Value
db_inner.sync_outbox().queue_for_sync("SALE", &id, &payload)?;

// ✅ CORRECT - to_string returns String (reference as &str)
let payload = serde_json::to_string(&sale).unwrap_or_default();  // String
db_inner.sync_outbox().queue_for_sync("SALE", &id, &payload)?;
```

**Key Point**: `to_value()` → `Value`, `to_string()` → `String`  
**Prevention**: Check parameter type; use `to_string()` for storage/sync

---

## 🔴 Issue #4: Tailwind Color Palette

```javascript
// ❌ WRONG - Incomplete (missing 700, 800, 500 for states)
success: {
  50: '#f0fdf4',
  500: '#22c55e',
  600: '#16a34a',
}

// ✅ CORRECT - Complete (all 11 shades for states)
success: {
  50: '#f0fdf4', 100: '#dcfce7', 200: '#bbf7d0', 300: '#86efac',
  400: '#4ade80', 500: '#22c55e', 600: '#16a34a', 700: '#15803d',
  800: '#166534', 900: '#14532d', 950: '#0a2e1b',
}
```

**Key Point**: Need 11 shades for: base (500), hover (600), active (700), ring (400)  
**Prevention**: Define all shades 50, 100, 200, ... 900, 950

---

## 🔴 Issue #5: Database Migrations

```rust
// ❌ WRONG - No migrations, tables don't exist
let db = Database::connect().await?;
let products = db.products().list().await?;  // FAIL!

// ✅ CORRECT - Migrations first
let db = Database::connect().await?;
db.run_migrations().await?;
let products = db.products().list().await?;  // OK!
```

**Key Point**: Migrations create schema; must run before queries  
**Prevention**: Always call `db.run_migrations()` at startup

---

## 🔴 Issue #6: Type Inference with Async

```rust
// ❌ WRONG - Type inference fails after .await?
let products = db_inner.products()
    .search(query, limit).await?
    .into_iter()
    .map(ProductDto::from)
    .collect();  // ERROR: Can't infer Vec vs HashMap

// ✅ CORRECT - Annotate at boundaries
let db_inner: &Database = (*db).inner();  // Annotation #1
let products: Vec<ProductDto> = db_inner.products()  // Annotation #2
    .search(query, limit).await?
    .into_iter()
    .map(ProductDto::from)
    .collect();
```

**Key Point**: `.await?` breaks type inference chains  
**Prevention**: Annotate after State deref and at final collection

---

## ✅ Validation Checklist

Before committing code, verify:

- [ ] **State Dereferencing**: Using `(*state).method()`?
- [ ] **Type Annotations**: Explicit types after State deref?
- [ ] **Return Types**: Verified repository method signatures?
- [ ] **Serialization**: Using `to_string()` for payloads?
- [ ] **Colors**: All 11 shades (50-950) defined?
- [ ] **Migrations**: `db.run_migrations()` called?
- [ ] **Type Inference**: Annotated collection types?
- [ ] **Compile Check**: `cargo check` passing?
- [ ] **Manual Test**: Tested with `pnpm tauri dev`?

---

## 📚 Documentation References

### Full Details
- **Copilot Instructions**: See `.github/copilot-instructions.md`
  - Section: "🔍 Critical Implementation Pitfalls & Prevention"

### Detailed Analysis
- **Lessons Document**: See `docs/AI_CODE_GENERATION_LESSONS.md`
  - Category 1-6 detailed analysis
  - Root cause explanations

### Visual Diagrams
- **Flow Diagrams**: See `docs/DEBUGGING_FLOW_DIAGRAMS.md`
  - ASCII flow charts for each issue

### Quick Summary
- **Debugging Summary**: See `docs/MILESTONE_1_DEBUGGING_SUMMARY.md`
  - Before/after code comparison
  - Impact assessment

---

## 🚀 How to Use This Card

### For Code Generation
1. Generate code
2. Check this card for all 6 issues
3. Verify against ✅ CORRECT examples
4. Run validation checklist
5. Test with `cargo check` + `pnpm tauri dev`

### For Code Review
1. See issue in PR
2. Reference row in quick lookup table
3. Show correct example from this card
4. Link to full documentation if needed
5. Request revision with specific guidance

### For Debugging
1. Encounter error
2. Match pattern to one of 6 issues
3. Reference solution code
4. Check prevention strategy for future

---

## 💡 Pro Tips

✅ **Bookmark this file** for quick reference  
✅ **Print the table** for physical reference  
✅ **Share with team** during code reviews  
✅ **Reference in commit messages** when fixing these issues  
✅ **Update as new patterns emerge** from future milestones

---

*Last Updated: February 1, 2026*  
*Status: Quick Reference Card*  
*Next Update: After Milestone 2 Completion*
