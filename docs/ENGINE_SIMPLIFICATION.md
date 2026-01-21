# Engine Simplification: Resolving the Market Model Confusion

## Summary of FIXMEs

The matching-engine crate has several FIXME comments all pointing to the same core confusion:

| File | Line | Issue |
|------|------|-------|
| `market.rs:5` | Why do we have both `Market` and `LiquidityPool`? |
| `book.rs:42` | "I thought all markets are binary? Multi-outcome ones are just multiple binaries?" |
| `book.rs:240` | What is our model? (1) All binary or (2) All multi-outcome? |
| `book.rs:245` | Style: use imports instead of `std::collections::` prefix |
| `constraints.rs:11-16` | Constraint system seems "sus" - who sets implications? Too much overhead? |

---

## The Core Confusion: Two Incompatible Mental Models

The codebase is caught between two different models:

### Model A: All Markets Are Binary (Simpler)

```
Engine level:  [Binary Market A] [Binary Market B] [Binary Market C]
                    ↓                  ↓                  ↓
               Yes/No shares      Yes/No shares      Yes/No shares

Solver level:  "Markets A, B, C are related - their YES prices should sum to 100%"
               (This is the PriceNormalization solver's job)
```

In this model:
- Engine only deals with binary markets
- "Multi-outcome" is a **UI/solver concept** - just a label grouping related binaries
- The solver enforces `Σ prices = 100%` for related markets
- Constraints like "Trump wins → Republican wins" are expressed via **orders** (MMs using flash liquidity)
- `LiquidityPool` keys by `(MarketId, 0)` only - outcome_idx always 0 for YES

### Model B: Native Multi-Outcome Markets (Current Partial Implementation)

```
Engine level:  [Multi-Outcome Market: Trump/Harris/Other]
                         ↓
               Separate liquidity books for each outcome
               outcome_idx = 0, 1, 2

Orders:        Payoff vectors over product of all outcome combinations
```

In this model:
- Engine understands 3+ outcome markets natively
- `LiquidityPool` keys by `(MarketId, outcome_idx)` where outcome_idx ∈ {0, 1, 2, ...}
- `Market` struct needs `outcomes: Vec<String>`
- More complex state space calculations

---

## What The Code Currently Does (The Mess)

```
Market          → Has outcomes: Vec<String> (supports multi-outcome)
MarketSet       → Can create binary OR multi-outcome markets
LiquidityPool   → Keys by (MarketId, outcome_idx) (supports multi-outcome)
LiquidityBook   → Has outcome_idx field (but often unused)
Order           → Uses payoff vectors over states (works for both models)
Constraints     → Full constraint system (Implication, Hierarchy, etc.)
```

The code **partially implements Model B** but the **mental model is Model A**.

Result: Confused code, unused fields, redundant abstractions.

---

## Recommendation: Simplify to Model A (All Binary)

### Why Model A?

1. **Simpler engine** - Less state, fewer abstractions
2. **Multi-outcome is a UI concern** - Users see "Who wins: Trump/Harris/Other" but engine sees 3 binary markets
3. **Solver handles normalization** - PriceNormalization already exists for this
4. **Constraints become orders** - MMs express "Trump → Republican" via their order payoffs, not via a separate constraint system
5. **Partial resolution support** - Binary markets naturally support partial resolution (market resolves to 80% YES)

### What Changes

| Component | Current | Simplified |
|-----------|---------|------------|
| `Market` | `outcomes: Vec<String>` | Just `name: String`, always binary |
| `MarketSet` | `add()` with outcomes | Only `add_binary()` |
| `LiquidityPool` | `(MarketId, outcome_idx)` | Just `MarketId` (outcome always 0=YES) |
| `LiquidityBook` | Has `outcome_idx` | Remove `outcome_idx` |
| `constraints.rs` | Full constraint system | **Delete entirely** |
| Solver | PriceNormalization | Same, but now the ONLY place multi-outcome logic lives |

### Multi-Outcome in UI/Solver Layer

```rust
// New structure at solver level (NOT in matching-engine)
struct OutcomeGroup {
    name: String,           // "2024 Presidential Election"
    markets: Vec<MarketId>, // [trump_market, harris_market, other_market]
}

// Solver enforces: Σ price(market.YES) = 100% for markets in group
```

---

## The Constraint System Question

The FIXME in `constraints.rs` asks: "Who sets those implications?"

**Current design**: Market creator defines constraints like "Trump wins → Republican wins"

**Problem**:
- Overhead for market creators
- Duplicate logic (orders already express relationships via payoffs)
- Not used in practice?

**Recommendation**: Delete `constraints.rs`

Relationships between markets should be expressed via:
1. **Orders** - A bundle order "Buy Trump YES, Buy Republican YES" naturally expresses correlation
2. **MM flash liquidity** - MMs quote based on their view of relationships
3. **Arbitrage detection** - Solver finds and exploits mispricings

The only constraint we might keep: `SumToOne` for outcome groups (but this moves to solver layer).

---

## Partial Resolution Support

The FIXME mentions: "we want support markets where resolution could be partial"

Example: Market resolves to 80% probability → YES shares worth $0.80, NO shares worth $0.20

**Good news**: Binary model supports this naturally!
- Resolution is just: `YES_value = resolve_probability`, `NO_value = 1 - resolve_probability`
- No special multi-outcome logic needed

---

## Action Plan

### Phase 1: Cleanup (No Behavior Change)

1. Fix style issue: Replace `std::collections::HashMap` with imported `HashMap`
2. Add `OutcomeGroup` to solver layer for UI grouping
3. Audit: Is `outcome_idx` ever used with value > 0?

### Phase 2: Simplify (Breaking Change)

1. Delete `constraints.rs` entirely
2. Simplify `Market` to always binary (remove `outcomes` field)
3. Simplify `LiquidityPool` to key by `MarketId` only
4. Remove `outcome_idx` from `LiquidityBook`
5. Update `MarketSet` to only have `add_binary()`

### Phase 3: Document

1. Document that multi-outcome is a UI/solver concept
2. Document how MMs express market relationships via orders
3. Add `OutcomeGroup` documentation

---

## Files Affected

| File | Action |
|------|--------|
| `constraints.rs` | **DELETE** |
| `market.rs` | Simplify `Market` struct |
| `book.rs` | Remove `outcome_idx`, simplify `LiquidityPool` |
| `problem.rs` | Remove constraint references |
| `lib.rs` | Remove constraint exports |
| `state.rs` | May need updates |

---

## Audit Results

### Q1: Is `outcome_idx > 0` ever used?

**YES** - Multi-outcome is actively used:

```rust
// local_solver.rs tests (3-outcome market)
problem.liquidity.add_ask(market, 0, 400_000_000, 1000); // A at $0.40
problem.liquidity.add_ask(market, 1, 350_000_000, 1000); // B at $0.35
problem.liquidity.add_ask(market, 2, 300_000_000, 1000); // C at $0.30

// mega.rs scenario generation
.add_ask(market_id, outcome_idx as u8, ask_price, liquidity_qty);
```

Binary markets use: `outcome_idx=0` (YES), `outcome_idx=1` (NO)

### Q2: Are constraints used anywhere?

**YES** - In scenario generation:

```rust
// random.rs:105
let mut constraint_builder = ConstraintBuilder::new();

// stress.rs:148, 689
let mut constraint_builder = ConstraintBuilder::new();
```

But constraints are NOT used in the core solving logic - they're only for test scenarios.

### Q3: What does this mean?

The code **fully implements Model B** (native multi-outcome). The FIXMEs express confusion because the author was thinking in terms of Model A but the code does Model B.

**Options:**

1. **Keep Model B** (current) - Multi-outcome is already working, just remove the confused comments
2. **Migrate to Model A** - Significant refactor, but simpler mental model

---

## Revised Recommendation

Given that Model B is already implemented and working, the pragmatic choice is:

### Option 1: Keep Model B, Clean Up Confusion

1. **Remove FIXME comments** - The code is correct, the comments are confused
2. **Document the model clearly** - "Markets can have N outcomes, binary is just N=2"
3. **Keep constraints** - They're useful for scenario generation
4. **Fix style** - Import HashMap instead of using prefix

### Option 2: Simplify to Model A (More Work)

Only do this if there's a strong reason to prefer binary-only at engine level.

**My recommendation: Option 1** - The code works, just needs documentation.

---

## Summary

**The code is not confused - the comments are.**

The engine correctly implements Model B (native multi-outcome markets). The FIXMEs reflect uncertainty during development, not actual bugs.

**Minimal fix:**
1. Remove the confused FIXME comments
2. Fix the `std::collections::` style issue
3. Add clear documentation: "Markets have N outcomes. Binary markets have N=2."

**Larger refactor (optional):**
If you want Model A (all-binary), it's a significant change but would simplify the mental model. Only worth doing if there's a compelling reason.

**The constraints system** is used for test scenario generation, not core solving. Keep it unless you want to simplify test scenarios.
