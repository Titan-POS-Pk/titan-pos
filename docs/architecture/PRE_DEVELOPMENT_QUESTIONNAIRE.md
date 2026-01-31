# Titan POS: Pre-Development Questionnaire

> **Status**: AWAITING YOUR INPUT  
> **Date**: January 31, 2026  
> **Purpose**: Finalize architectural decisions before coding begins

---

## Instructions

Please review each question and provide your decision. I've included my recommendations based on the architectural analysis. Once you confirm or override these decisions, I'll proceed with implementation.

---

## Category A: Core Business Logic

### Q1: Tenant Model for v0.1

**Context**: Multi-tenancy affects every table schema and query.

| Option | Description | Migration Effort Later |
|--------|-------------|------------------------|
| **A** | Single-tenant (no tenant_id) | High - schema changes |
| **B** ⭐ | Multi-tenant schema, single-tenant runtime | None |

**My Recommendation**: **Option B**
- We add `tenant_id` to all tables from day one
- For v0.1, hardcode a single tenant UUID in config
- Zero refactoring when we add multi-tenancy later

**Your Decision**: _____________

---

### Q2: User Authentication for v0.1

**Context**: How do cashiers identify themselves?

| Option | Description | User Experience |
|--------|-------------|-----------------|
| **A** ⭐ | Device-level auth (no login) | Fast, but no accountability |
| **B** | PIN-based switching (4-6 digits) | Good balance |
| **C** | Full login (username/password) | Secure, but slow |

**My Recommendation**: **Option A** for v0.1
- `user_id` comes from a config file (e.g., "Terminal 1")
- Schema includes `user_id` for future expansion
- v0.5 adds PIN-based switching

**Your Decision**: _____________

---

### Q3: Tax Calculation Mode

**Context**: Different regions display prices differently.

| Option | Description | Regions |
|--------|-------------|---------|
| **A** ⭐ | Tax-exclusive (price + tax) | USA, Canada |
| **B** | Tax-inclusive (tax in price) | EU, UK, AU |
| **C** | Both (configurable per tenant) | Global |

**My Recommendation**: **Option C** (configurable)
- Store `tax_mode: 'exclusive' | 'inclusive'` in tenant config
- Default to 'exclusive' for v0.1
- Tax calculation logic handles both modes

**Your Decision**: _____________

---

### Q4: Discount Application Order

**Context**: Tax calculation depends on when discounts apply.

```
Scenario: $100 item, 10% discount, 10% tax

Option A (Discount → Tax):         Option B (Tax → Discount):
  Subtotal:    $100                  Subtotal:    $100
  Discount:    -$10                  Tax (10%):   +$10
  Taxable:     $90                   Total:       $110
  Tax (10%):   +$9                   Discount:    -$10
  Total:       $99                   Final:       $100
```

| Option | Description | Standard |
|--------|-------------|----------|
| **A** ⭐ | Discount before tax | Most common |
| **B** | Discount after tax | Rare |

**My Recommendation**: **Option A**
- This is the accounting standard in most jurisdictions
- Discounts reduce the taxable amount

**Your Decision**: _____________

---

### Q5: Inventory Tracking Behavior

**Context**: Should we block sales when stock hits zero?

| Option | Behavior | Use Case |
|--------|----------|----------|
| **A** | Block sale at 0 stock | Strict inventory |
| **B** | Warning, allow sale | "We'll order more" |
| **C** ⭐ | Configurable per product | Flexible |

**My Recommendation**: **Option C**
- Each product has `track_inventory: bool` and `allow_negative: bool`
- Services (e.g., "Gift Wrapping") don't need tracking
- Physical goods can be strict or lenient per business need

**Your Decision**: _____________

---

## Category B: Data & Sync

### Q6: Receipt Number Format

**Context**: Receipts need human-readable numbers that don't collide offline.

| Option | Format | Example | Collision Risk |
|--------|--------|---------|----------------|
| **A** | UUID only | `a1b2c3d4-...` | None, but ugly |
| **B** | Device + Sequential | `POS01-000001` | Low |
| **C** ⭐ | Date + Device + Seq | `20260131-01-0001` | Very low |

**My Recommendation**: **Option C**
- UUID remains the database primary key
- `receipt_number` is the human-facing field
- Format: `{YYYYMMDD}-{device_code}-{daily_seq}`
- Resets sequence daily for shorter numbers

**Your Decision**: _____________

---

### Q7: Sync Conflict Notification

**Context**: When CRDT merges inventory after sync, should users know?

| Option | Behavior | Transparency |
|--------|----------|--------------|
| **A** ⭐ | Silent merge (CRDT handles it) | Low - "It just works" |
| **B** | Toast notification | Medium - "Inventory adjusted" |
| **C** | Manager approval queue | High - Review conflicts |

**My Recommendation**: **Option A** for v0.1
- CRDT math guarantees correctness
- Users don't need to worry about sync internals
- Audit log captures all changes for investigation
- v1.0 can add manager reports for unusual variances

**Your Decision**: _____________

---

### Q8: Maximum Offline Duration

**Context**: How long can a terminal operate without syncing?

| Option | Limit | Enforcement |
|--------|-------|-------------|
| **A** ⭐ | Unlimited | Can operate forever |
| **B** | Soft (7 days) | Warning banner |
| **C** | Hard (30 days) | Lock until sync |

**My Recommendation**: **Option A** with soft warnings
- Core principle: "Never block a sale"
- Show warning after 24 hours offline
- Show urgent warning after 7 days
- Admin can optionally enable hard lock

**Your Decision**: _____________

---

### Q9: Local Data Retention

**Context**: How long to keep transaction history on the device?

| Option | Retention | Storage Impact |
|--------|-----------|----------------|
| **A** | Forever | Grows indefinitely |
| **B** ⭐ | Rolling 90 days | ~500MB typical |
| **C** | Delete after sync | Minimal, but no offline history |

**My Recommendation**: **Option B**
- Keep 90 days of full transaction data locally
- Older records archived after confirmed cloud sync
- Allows historical lookups for returns/disputes
- Configurable per tenant (some may want longer)

**Your Decision**: _____________

---

## Category C: Platform & Deployment

### Q10: Target Platforms for v0.1

**Context**: Which operating systems must we support?

| Platform | Priority | Notes |
|----------|----------|-------|
| macOS (Apple Silicon) | ⭐ Must | Dev machines, modern Macs |
| macOS (Intel) | Should | Legacy Macs |
| Windows 10/11 | ⭐ Must | 90% of POS hardware |
| Linux (Ubuntu 22.04) | Could | Kiosk deployments |

**My Recommendation**:
- **v0.1**: macOS (ARM) + Windows 10/11
- **v0.5**: Add macOS Intel + Ubuntu

**Your Decision**: _____________

---

### Q11: Currency Support

**Context**: Single currency or multi-currency?

| Option | Description | Complexity |
|--------|-------------|------------|
| **A** ⭐ | Single currency per tenant | Low |
| **B** | Multi-currency per tenant | High |

**My Recommendation**: **Option A**
- `currency: 'USD' | 'EUR' | 'GBP' | ...` in tenant config
- All amounts stored in that currency's minor unit (cents)
- Multi-currency can come in v2.0 if needed

**Your Decision**: _____________

---

### Q12: Logging & Telemetry

**Context**: How should we log events?

| Option | Local | Cloud |
|--------|-------|-------|
| **A** | JSON files only | None |
| **B** ⭐ | JSON files | Optional telemetry sink |
| **C** | JSON files | Mandatory telemetry |

**My Recommendation**: **Option B**
- Local: JSON logs via `tracing` crate
- Cloud: Optional (can be disabled for privacy)
- Crash reports: Only with user consent

**Your Decision**: _____________

---

## Category D: Development Process

### Q13: Minimum Test Coverage

**Context**: What level of test coverage do we require?

| Component | Unit Tests | Integration | E2E |
|-----------|------------|-------------|-----|
| titan-core | 90%+ | N/A | N/A |
| titan-db | 50%+ | 80%+ | N/A |
| titan-sync | 50%+ | 80%+ | N/A |
| titan-tauri | 30%+ | 50%+ | Key flows |

**Is this acceptable?**: _____________

---

### Q14: Code Review Requirements

**Context**: What level of review for changes?

| Change Type | Review Required |
|-------------|-----------------|
| Schema changes | 2 approvals |
| Core logic (titan-core) | 2 approvals |
| Bug fixes | 1 approval |
| UI/styling | 1 approval |

**Is this acceptable?**: _____________

---

## Summary of My Recommendations

| Question | My Recommendation |
|----------|-------------------|
| Q1: Tenant Model | Multi-tenant schema, single runtime |
| Q2: User Auth | Device-level (no login) |
| Q3: Tax Mode | Configurable (default exclusive) |
| Q4: Discount Order | Discount before tax |
| Q5: Inventory | Configurable per product |
| Q6: Receipt Number | Date + Device + Sequence |
| Q7: Sync Notification | Silent (audit log only) |
| Q8: Offline Duration | Unlimited with warnings |
| Q9: Data Retention | Rolling 90 days |
| Q10: Platforms | macOS ARM + Windows |
| Q11: Currency | Single per tenant |
| Q12: Logging | JSON + Optional telemetry |
| Q13: Test Coverage | As specified |
| Q14: Code Review | As specified |

---

## Next Steps

Once you provide your decisions:

1. I'll update the Architecture Decision Records (ADR)
2. Create the Rust workspace structure
3. Implement the database migrations
4. Scaffold the Tauri application
5. Build the core business logic

---

**Please reply with your decisions in this format:**

```
Q1: B (confirmed) or A (override) or "need clarification"
Q2: ...
Q3: ...
...
```

Or simply: **"Confirm all recommendations"** if you agree with my suggestions.
