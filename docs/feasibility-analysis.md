# Sybil V2: Feasibility & Complexity Analysis

## Executive Summary

**Bottom line**: The architecture is feasible but significantly more complex than typical order matching systems. The core risk is not "will it work theoretically" but "can we keep complexity bounded while maintaining the key features that differentiate the system."

**Key insight**: The system is really solving a **combinatorial auction** problem, not a simple matching problem. This is a fundamentally harder class of problems. The question isn't whether it's solvable (it is), but whether it's solvable *efficiently enough* in *bounded time* with *acceptable approximation*.

**Recommendation**: Staged implementation with explicit complexity gates. Start with a system equivalent to V1 (simple, proven), then add complexity incrementally with clear metrics for success/failure at each stage.

---

## Part 1: Sources of Complexity (Ranked)

### 🔴 Critical Complexity: The Bilinear Budget Constraint

**What it is**: The flash quoting feature requires checking budgets against *execution prices*, not limit prices:
```
Σ fill[order] × clearing_price[order] ≤ user_balance
```

Both `fill` and `clearing_price` are unknowns. This makes the optimization problem **bilinear** (product of two variables), not linear.

**Why it matters**:
- Linear programs (LP) are solved in polynomial time with Simplex/Interior Point
- Bilinear programs require iteration or heuristics
- No guaranteed convergence
- May have multiple equilibria
- Solver complexity goes from "trivial" to "research problem"

**The cascade effect**: This single feature touches:
- Solver algorithm (must iterate, can't use standard LP)
- Validation logic (must handle edge cases)
- Dust handling (rounding errors in iteration)
- Convergence guarantees (or lack thereof)

**Can we remove it?**

| Option | Description | Impact |
|--------|-------------|--------|
| **Keep bilinear** | Budget at execution price | Full flash quoting, complex solving |
| **Budget at limit** | Budget at worst-case (limit) price | LP becomes linear, users need more capital |
| **Hybrid** | Limit price + small buffer | Mostly linear, slight over-collateralization |

**Recommendation**: Start with **budget at limit price**. This makes the LP linear. Add bilinear budget later as an advanced feature for sophisticated users who opt in.

---

### 🔴 Critical Complexity: Cross-Market Coupling

**What it is**: Several features create dependencies between markets:

1. **Flash quoting**: User's $100 spans markets A, B, C
2. **Jumbo orders**: "Buy A AND B" — must fill both or neither
3. **Conditional orders**: "Buy B only if A fills"
4. **Spread trades**: "Long A, Short B"

**Why it matters**:
- Independent markets can be solved in parallel
- Coupled markets must be solved together
- Worst case: ALL markets coupled → one giant LP
- Combinatorial explosion in constraint space

**The dependency graph problem**:
```
User1: orders on A, B
User2: orders on B, C
User3: orders on C, D
→ Markets A, B, C, D are transitively coupled via budgets
```

One user's cross-market order can force the entire system to be solved jointly.

**Can we bound it?**

| Option | Description | Impact |
|--------|-------------|--------|
| **Full coupling** | Any order can reference any market | Maximum expressiveness, unbounded complexity |
| **Cluster limits** | Max N markets in one order | Bounded complexity, some orders rejected |
| **Isolated markets** | No cross-market orders | Parallel solving, no flash quoting |
| **Explicit clusters** | Markets grouped into clusters, cross only within | Parallelism between clusters |

**Recommendation**: **Explicit market clusters** with size limits. Users who want cross-market must operate within a cluster. Clusters are solved independently and in parallel.

---

### 🟡 Significant Complexity: Multi-Solver Block Building

**What it is**: Multiple solvers submit `ExecutionPlan`s, block builder combines them.

**The problem**: If Solver A fills Order #101 and Solver B also fills Order #101, which wins? The MWIS (Maximum Weight Independent Set) formulation is NP-hard.

**Why it might be over-engineered**:
- Ethereum block builders work because MEV is highly competitive
- Our solvers all see the same orderbook → likely similar solutions
- The "combination" problem may be mostly theoretical

**Reality check from CoW Swap**:
- They have ~20 active solvers
- Solutions often overlap significantly
- Winner-takes-all (best solution wins entirely) works fine
- Complex combination rarely needed

**Can we simplify?**

| Option | Description | Impact |
|--------|-------------|--------|
| **MWIS combination** | Find optimal non-conflicting subset | Complex, NP-hard, may not add value |
| **Winner takes all** | Best scoring solution wins entirely | Simple, may leave money on table |
| **Single solver** | Only one solver allowed | Simplest, but centralized |
| **Solver rotation** | Solvers take turns as primary | Simple, fair, but not optimal |

**Recommendation**: Start with **winner takes all**. The marginal gain from complex combination is likely small compared to the complexity cost. Add MWIS later if empirically justified.

---

### 🟡 Significant Complexity: WASM Sandbox

**What it is**: Running untrusted solver code in TEE with memory/CPU limits.

**Why it's complex**:
- Security: sandbox escapes, side channels
- Performance: WASM overhead, context switching
- Interface: host functions, memory management
- Debugging: hard to diagnose issues in sandbox

**The trust question**: Do we actually need untrusted solvers?

- **CoW Swap**: Solvers are semi-trusted (must stake, can be slashed)
- **MEV**: Searchers are untrusted but only submit bundles, not code
- **Us**: Do we need to run arbitrary user code, or just accept solutions?

**Alternative architecture**:
```
Current: Solver WASM runs in TEE, produces solution
Alternative: Solver runs ANYWHERE, submits solution, TEE validates
```

The alternative is much simpler:
- No sandboxing needed
- Solvers can use any language/hardware
- TEE only validates (which it must do anyway)
- Trade-off: solvers see solution before TEE, slight information leakage

**Recommendation**: **Solvers run outside TEE, submit solutions for validation.** This eliminates WASM sandbox complexity entirely. The "TEE runs solver" model can be an optional optimization later.

---

### 🟢 Manageable Complexity: Linear Constraint Orders

**What it is**: Orders as linear equations rather than simple limit orders.

**Why it's actually not that bad**:
- Parsing/validation is straightforward
- LP construction is mechanical
- Well-understood mathematics
- Libraries exist (HiGHS, GLPK, etc.)

**The expressiveness is the feature**:
- Simple limit orders are a special case
- Spread trades, distributions, conditionals fall out naturally
- Worth the complexity if cross-market coupling is bounded

**Recommendation**: **Keep this feature.** It's the core value proposition and complexity is manageable if other sources are bounded.

---

### 🟢 Manageable Complexity: Solver Incentives

**What it is**: Fee sharing, welfare bounties, JIT fees.

**Why it's actually separable**:
- Incentive calculation is post-hoc
- Doesn't affect matching algorithm
- Can be changed without touching core engine
- A "boundary" as planned

**Recommendation**: **Start with simple fixed fees.** Optimize incentive structure later based on observed solver behavior.

---

## Part 2: Unknown Unknowns

### What We Don't Know We Don't Know

| Area | Unknown | Potential Impact | Mitigation |
|------|---------|------------------|------------|
| **LP Convergence** | Does fixed-point iteration converge for arbitrary orderbooks? | Batches fail to execute | Start linear, add bilinear gradually |
| **Solver Economics** | Is it profitable to run a solver at low volume? | No one builds solvers | Protocol-run fallback, subsidies |
| **Adversarial Orders** | Can malicious orders break/slow the LP? | DoS, griefing | Fuzzing, order complexity limits |
| **Price Equilibria** | Do multiple equilibria exist? Which one do we pick? | Non-determinism, arbitrage | Price continuity constraints |
| **TEE Performance** | How does TEE perform under load? | Latency, throughput limits | Benchmark early, plan for limits |
| **Real Orderflow** | What do actual orders look like? | Mistuned system | Simulation, testnet, gradual rollout |

### The "Looks Fine Until It Isn't" Risk

The system may work perfectly in testing and fail under adversarial conditions. The combinatorial nature means:
- Edge cases are hard to enumerate
- Fuzzing helps but can't find everything
- Real attackers are creative

**Mitigation strategy**:
1. Formal verification of core invariants
2. Economic audits of incentive structure
3. Staged rollout with circuit breakers
4. Bug bounty program before mainnet

---

## Part 3: Tight Coupling Analysis

### The Coupling Matrix

| Component | Depends On | Depended By | Coupling Risk |
|-----------|------------|-------------|---------------|
| **Order format** | Nothing | Everything | 🔴 Critical — changes cascade everywhere |
| **Prices** | Solver | Engine, validation | 🟡 Medium — clear interface |
| **User balance** | Deposits | All orders, all markets | 🔴 Critical — creates cross-market coupling |
| **Batch state** | Sequencer | Engine, solvers | 🟡 Medium — clear state machine |
| **Market state** | Engine | Solvers | 🟢 Low — well-defined boundary |

### Breaking the Coupling

**Goal**: Each component should be modifiable without affecting others.

**Strategy 1: Immutable Order Format**
- Define Order v1 format now
- Never modify, only extend with new versions
- Old orders always valid

**Strategy 2: Per-Market Balance Shards**
- User balance is sharded by market cluster
- Cross-cluster orders require explicit transfer
- Each cluster is independent

**Strategy 3: Solver Interface as Contract**
- `SolverInput` and `ExecutionPlan` are the ONLY interface
- Engine and solver can be developed independently
- Changes to either side don't affect the other

**Strategy 4: Validation as Separate Layer**
- All validation logic in one place
- Engine trusts validation output
- Validation can be upgraded independently

### Recommended Module Boundaries

```
┌─────────────────────────────────────────────────────────────┐
│                    COMPLEXITY FIREWALL                       │
├─────────────────┬─────────────────┬─────────────────────────┤
│   Zone A:       │   Zone B:       │   Zone C:               │
│   Untrusted     │   Trusted Core  │   Execution             │
├─────────────────┼─────────────────┼─────────────────────────┤
│ • User orders   │ • Validation    │ • State transitions     │
│ • Solver code   │ • LP construct  │ • Balance updates       │
│ • External APIs │ • Price checks  │ • Position updates      │
├─────────────────┼─────────────────┼─────────────────────────┤
│ Can fail/attack │ Catches errors  │ Assumes valid input     │
│ Sandboxed       │ Thorough checks │ Minimal logic           │
└─────────────────┴─────────────────┴─────────────────────────┘
```

---

## Part 4: Would Solvers Actually Work?

### The Solver Economics Question

**Revenue model**:
```
Solver Revenue = (Fee Share × Volume Matched) + (Welfare Bounty) + (JIT Profits)
```

**Cost model**:
```
Solver Cost = Compute + Capital (stake) + Development + Operations
```

**Break-even analysis** (rough estimates):

| Daily Volume | Fee Share (5bps) | Compute Cost | Profitable? |
|--------------|------------------|--------------|-------------|
| $10K | $5/day | $50/day | ❌ No |
| $100K | $50/day | $50/day | ⚠️ Marginal |
| $1M | $500/day | $100/day | ✅ Yes |
| $10M | $5K/day | $200/day | ✅ Very |

**The cold start problem**:
- No volume → no solver incentive
- No solvers → bad execution → no volume
- Classic chicken-and-egg

**Solution**: Protocol-run solver that operates at a loss initially. This is standard (Flashbots relay, CoW Swap in-house solver, 0x market makers).

### Can Solvers Solve Efficiently?

**Problem dimensions** (typical batch):
- 100 orders
- 10 markets
- 2 outcomes per market
- ~120 variables, ~200 constraints

**For linear LP**: Trivial. HiGHS solves in <10ms.

**For bilinear (with iteration)**:
- 10-50 iterations typical
- Each iteration is one LP solve
- Total: 100-500ms
- Within 1s window: ✅ Yes

**For pathological cases** (1000 cross-market orders):
- LP becomes huge
- Iteration may not converge
- May need approximation

**The 90/10 heuristic**:
- 90% of batches are "easy" (few cross-market orders)
- 10% are "hard" (many dependencies)
- Optimize for easy case, handle hard case gracefully

**Recommendation**:
- Set complexity limits (max orders per batch, max cross-market per user)
- Timeout at 800ms, use best solution found so far
- Track metrics to tune limits

### Would Block Building Work?

**Comparison to Ethereum**:

| Aspect | Ethereum Block Building | Sybil Block Building |
|--------|------------------------|---------------------|
| Input | Bundles (tx sequences) | ExecutionPlans (fills + prices) |
| Conflict | Tx ordering, state conflicts | Order fill conflicts, price conflicts |
| Objective | Maximize MEV extraction | Maximize welfare + volume |
| Solvers | ~10-20 serious builders | Unknown, likely fewer |
| Stakes | Billions in ETH | Much smaller |

**Key differences**:
1. Ethereum has clear "winning" objective (profit)
2. Our objective (welfare + volume) is more complex
3. Ethereum bundles are simpler (just tx ordering)
4. Our solutions have more dimensions (fills AND prices)

**Will greedy MWIS be good enough?**

Greedy gives 2-approximation for MWIS in general. But:
- Our conflict graph may be sparse (most orders don't conflict)
- When sparse, greedy is near-optimal
- Can improve algorithm if metrics show it's a bottleneck

**Recommendation**: Start with greedy or winner-takes-all. Instrument to measure optimality gap. Improve only if gap is significant.

---

## Part 5: Security Deep Dive

### Threat Model

| Attacker | Goal | Capability |
|----------|------|------------|
| Malicious User | Profit unfairly, grief others | Submit arbitrary orders |
| Malicious Solver | Extract value, grief competitors | Submit solutions, (maybe) run in TEE |
| External Observer | Front-run, copy strategies | See public data (not TEE internals) |
| TEE Attacker | Break confidentiality | Side channels, exploits |

### Attack Vectors & Mitigations

**1. LP Poisoning**
- Attack: Submit orders that make LP infeasible or slow
- Impact: Batch fails, orders stuck
- Mitigation: Order complexity scoring, reject pathological orders
- Residual risk: Medium (hard to enumerate all bad patterns)

**2. Solver Griefing**
- Attack: Submit invalid solutions repeatedly
- Impact: Wastes engine time, delays execution
- Mitigation: Solver stake slashing, rate limits
- Residual risk: Low (economic penalty)

**3. Budget Exploitation**
- Attack: Exploit bilinear constraint for "free" leverage
- Impact: User ends up with negative balance
- Mitigation: Conservative validation, dust epsilon
- Residual risk: Medium (edge cases in iteration)

**4. Price Manipulation**
- Attack: Submit orders to move clearing price, profit elsewhere
- Impact: Other users get worse execution
- Mitigation: Hard to prevent entirely, monitor patterns
- Residual risk: High (fundamental to any market)

**5. Information Leakage**
- Attack: Solver uses orderbook info to front-run
- Impact: Users get worse execution
- Mitigation: JIT fee moat, asymmetric fees
- Residual risk: Medium (solvers see orderbook by design)

**6. TEE Escape**
- Attack: Exploit TEE vulnerability to extract secrets
- Impact: Order confidentiality broken
- Mitigation: Standard TEE hardening, minimize secrets
- Residual risk: Low (TEE exploits are rare and valuable)

### Security Boundaries (Hardened)

```
                    UNTRUSTED
                        │
                        ▼
┌─────────────────────────────────────────┐
│           INPUT VALIDATION              │ ← All inputs validated here
│  • Order format checks                  │
│  • Signature verification               │
│  • Balance sufficiency                  │
│  • Complexity scoring                   │
└─────────────────────────────────────────┘
                        │
                        ▼ (only valid inputs pass)
┌─────────────────────────────────────────┐
│           SOLVER SANDBOX                │ ← Solvers isolated here
│  • Resource limits                      │
│  • No cross-solver communication        │
│  • Output validation                    │
└─────────────────────────────────────────┘
                        │
                        ▼ (only valid solutions pass)
┌─────────────────────────────────────────┐
│           STATE MACHINE                 │ ← Minimal trusted core
│  • Applies validated state transitions  │
│  • Invariant checks after each step     │
│  • Halt on any violation                │
└─────────────────────────────────────────┘
```

---

## Part 6: Scaling Limits

### Theoretical Limits

| Resource | Limit | Determined By |
|----------|-------|---------------|
| Orders per batch | ~10K | LP solver capacity in 1s |
| Markets per batch | ~1K | State management, memory |
| Solvers per batch | ~100 | Block builder combination time |
| Cross-market orders | ~100 | LP coupling complexity |
| Users | ~1M | State size, memory |

### Practical Bottlenecks

**1. LP Solve Time**
- Scales roughly O(n³) in worst case for dense LP
- Sparse LP (typical) is much better
- Bottleneck at ~10K constraints

**2. Validation Time**
- Scales O(n) per order
- Parallelizable
- Not a bottleneck until millions of orders

**3. State Updates**
- Scales O(n) per batch
- In-memory is fast
- Bottleneck is state size, not speed

**4. TEE Memory**
- SGX: ~90MB enclave heap (older), ~1GB (newer)
- SEV: Full VM, larger
- May limit concurrent batches

### Scaling Strategy

**Vertical (immediate)**:
- Optimize hot paths (LP construction, validation)
- Use better LP solver (HiGHS > GLPK)
- Profile and fix bottlenecks

**Horizontal (future)**:
- Shard by market cluster
- Each shard runs independently
- Cross-shard orders require explicit bridging

**Temporal**:
- If batch too large, split into sub-batches
- Adjust batch duration based on load

---

## Part 7: What Can We Sacrifice?

### The Simplification Menu

| Feature | Complexity Cost | Value Add | Sacrifice? |
|---------|----------------|-----------|------------|
| Linear constraint orders | Low | High | ❌ Keep |
| Flash quoting (bilinear) | High | Medium | ⚠️ Defer |
| Cross-market orders | High | High | ⚠️ Limit |
| Multi-solver combination | Medium | Low | ✅ Simplify |
| WASM sandbox | High | Medium | ✅ Defer |
| JIT liquidity | Medium | Medium | ⚠️ Limit |
| Private solver context | Medium | Low | ✅ Defer |

### Recommended Cuts for V0

1. **Remove bilinear budget**: Use budget at limit price
2. **Remove cross-market**: Each market independent
3. **Single solver**: No block builder
4. **No WASM sandbox**: Trusted solver only
5. **No JIT orders**: Solvers can't inject liquidity
6. **No private context**: All solvers see same data

This gives us essentially V1 from old system with linear constraints. Proven to work.

### Phased Reintroduction

**V0.1**: Add cross-market within clusters (bounded coupling)
**V0.2**: Add JIT orders with strict limits
**V0.3**: Add bilinear budget as opt-in
**V1.0**: Add multi-solver with winner-takes-all
**V1.5**: Add WASM sandbox for untrusted solvers
**V2.0**: Add MWIS combination if justified

---

## Part 8: Tricks & Optimizations

### Solver Optimizations

**1. Warm Starting**
- Use previous batch's prices as initial guess
- Convergence is 2-5x faster
- Prices don't change much batch-to-batch

**2. Market Decomposition**
- Identify independent market clusters
- Solve each in parallel
- Combine at the end

**3. Early Termination**
- Don't need optimal, just good enough
- Stop when improvement < threshold
- Use remaining time for validation

**4. Incremental Updates**
- When one order changes, don't rebuild entire LP
- Update affected constraints only
- 10-100x faster for small changes

### Block Builder Optimizations

**1. Solution Fingerprinting**
- Hash solutions by which orders they touch
- Detect conflicts without full comparison
- O(1) conflict check vs O(n²)

**2. Priority Queues**
- Keep solutions sorted by score
- Early termination when remaining solutions can't improve
- Prune search space

**3. Caching**
- Similar batches often have similar optimal combinations
- Cache and adapt

### Engine Optimizations

**1. Lazy Validation**
- Don't validate everything upfront
- Validate on-demand when needed
- Fail fast on obvious errors

**2. Batch Pipelining**
- While batch N is solving, validate orders for batch N+1
- Reduces latency

**3. State Snapshots**
- Don't copy entire state for solver input
- Use copy-on-write or immutable data structures

---

## Part 9: Success Metrics & Failure Modes

### Metrics to Track

| Metric | Target | Alert Threshold |
|--------|--------|-----------------|
| Batch success rate | 99.9% | < 99% |
| Solve time p95 | < 500ms | > 800ms |
| Solver optimality | > 90% | < 70% |
| Validation failures | < 1% | > 5% |
| User fill rate | > 80% | < 50% |

### Failure Modes & Responses

| Failure | Detection | Response |
|---------|-----------|----------|
| LP infeasible | Solver returns error | Greedy fallback |
| Solver timeout | Time limit exceeded | Use partial solution |
| Validation failure | Invariant check fails | Reject solution, log for analysis |
| State corruption | Invariant check fails | HALT, manual investigation |
| TEE failure | Attestation fails | Failover to backup TEE |

### Circuit Breakers

1. **Batch failure rate > 5%**: Pause new orders, investigate
2. **Validation failures spike**: Reject all solver solutions, use fallback
3. **State invariant violation**: HALT immediately, do not proceed

---

## Part 10: Recommendations Summary

### Architecture Simplifications

| Current Design | Recommended | Rationale |
|---------------|-------------|-----------|
| Bilinear budget | Linear (at limit) | Major complexity reduction |
| Arbitrary cross-market | Bounded clusters | Enables parallelism |
| MWIS combination | Winner-takes-all | Simpler, likely sufficient |
| WASM sandbox | External solver | Eliminates sandbox complexity |
| Private solver context | Defer | Low value, medium complexity |

### Implementation Order

1. **Week 1-2**: Types crate with simplified order format
2. **Week 3-4**: Engine with single-market, linear LP
3. **Week 5-6**: Simple greedy solver (trusted)
4. **Week 7-8**: Integration testing, property tests
5. **Week 9-10**: Add market clusters, cross-market (bounded)
6. **Week 11-12**: Add second solver, winner-takes-all builder

### Critical Success Factors

1. **Lock down Order format early** — it's the most coupling
2. **Get validation 100% correct** — it's the security boundary
3. **Instrument everything** — you can't optimize what you can't measure
4. **Fail gracefully** — greedy fallback for any edge case
5. **Start simple** — complexity is easy to add, hard to remove

---

## Conclusion

The architecture is **feasible but risky in its current form**. The main risks are:

1. **Bilinear budget constraint** — makes solver problem fundamentally harder
2. **Unbounded cross-market coupling** — can force global solving
3. **Over-engineered block building** — complexity may not be justified

With the recommended simplifications, the system becomes:
- Mathematically tractable (linear LP)
- Parallelizable (market clusters)
- Incrementally improvable (add features when justified)

The core value proposition (linear constraint orders, FBA matching) is preserved. The advanced features (flash quoting, multi-solver combination) can be added later when the foundation is solid.

**Final recommendation**: Build V0 with simplifications. Run it. Measure. Add complexity only when empirically justified.
