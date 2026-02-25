# LMSR Duality & Frank-Wolfe for MM Budgets: Research Notes

## The LMSR–LP Connection

### Core result

The LP batch auction's minting cost function `V(D) = max_k D_k` and LMSR's cost function `C_b(D) = b·ln(Σ exp(D_k/b))` are the same object at different temperatures. As b→0, LMSR sharpens into the LP.

### Fenchel conjugates (the real insight)

- `V* = δ_Δ` — the conjugate of max is the simplex indicator. Minting at $1 per complete set *forces* prices to be probabilities. Three-line proof: convex combination ≤ max (Case 1), scaling/negativity blow up (Cases 2-3).
- `C_b* = -b·H(p)` — the conjugate of log-sum-exp is negative entropy. LMSR prices are softmax because entropy is the soft version of the simplex constraint.
- The duality diagram commutes: primal smoothing (V → C_b) and dual smoothing (δ_Δ → -bH) are Fenchel duals of each other.

### What the b parameter means

- b = liquidity depth (like LP tokens in a DeFi pool)
- b·ln(K) = maximum market maker loss = the LMSR subsidy
- b = 0 means no subsidy, self-financing minting. The order book provides all liquidity.
- Economically: batching makes the liquidity subsidy unnecessary.

### Self-financing theorem (Theorem 4)

By complementary slackness: if D_k < μ (constraint not tight), then p_k = 0. All probability mass sits on outcomes where D_k = μ. Therefore Σ p_k D_k = μ · 1 = μ. P&L = μ - μ = 0. The minting mechanism breaks even exactly.

### Novelty assessment (honest)

The individual pieces are known: LSE as smooth max (textbook), Fenchel conjugate of LSE = negative entropy (textbook), LMSR-entropy connection (Abernethy et al. 2013). What's new is the specific framing through batch auction minting cost, the self-financing theorem, and the O(K) vs O(2^K) scaling for groups. This is a "bridge paper" — connecting known results in a clean, economically meaningful way. Good section in a larger paper, not standalone.

## MM Budget Constraints

### The problem

The constraint `Σ capital(price, quantity) ≤ Budget` is bilinear (price × quantity). Price is a dual variable, quantity is primal. This is the *sole* source of non-convexity.

### Why SLP can fail

SLP = solve LP without budgets, linearize budget at resulting prices, re-solve. Two failure modes:

1. **Groups:** Trimming fills on market A frees probability mass (via Σp=1 constraint) that flows to market B, inflating capital there. Concrete example: MM has BuyYes q=100 on A and q=10 on B. Trim B → p_A rises → capital on A exceeds budget.

2. **Two-sided MM on same market:** Trimming buys drops price, which *increases* sell capital ((1-p) rises). The own-price feedback helps the trimmed side but hurts the opposite side.

SLP is conservative (over-trims) for one-sided orders on independent markets, but can under-trim for groups and two-sided positions.

### "Thick markets" is not a useful assumption

The interesting use case — flash liquidity on long-tail markets — is inherently thin. The whole point of flash quoting is lopsided fills (concentrate capital where welfare is highest). Protocol constraints like "per-market cap ≤ B/n" kill the feature.

### Frank-Wolfe approach

Dualize budget constraints with multipliers μ_k. The budget shadow price modifies each MM order's effective welfare:

```
w'_i = w_i - Σ_k μ_k · capital_per_unit_i(price)
```

The Frank-Wolfe oracle is the same LP as the base solver, with adjusted objectives. Each iteration:
1. Compute softmax prices at current q
2. Evaluate budget violation
3. Compute modified welfare coefficients
4. Solve LP with modified welfare → get vertex s
5. Step: q ← (1-γ)q + γs, with γ = 2/(t+2)
6. Update μ_k based on violation

**Key difference from SLP:** step size γ < 1 prevents overshooting. Dual variable μ accumulates budget information across iterations. SLP is Frank-Wolfe with γ=1, μ=0.

**Convergence:** O(1/√t) to a KKT point. ~50 LP solves total with annealing.

### Annealing

Start with large b (fast convergence, smooth prices), decrease b toward 0 (sharp LP prices). Warm-start each temperature from previous solution. ~10 temperatures × 5 iterations = 50 LP solves total. Same b as LMSR — this is the unifying idea.

## The Uniqueness Question (Theorems 8–9)

### Budget slippage convexity (the core math)

The Lagrangian L(q, μ) = f_b(q) - μ·cap(q) + μ·B is concave in q when cap(q) is convex. We computed the Hessian of the capital function cap = p(q)·q:

```
d²(p·Q)/dQ² = p(1-p)/b · [2 + Q(1-2p)/b]
```

This is positive (cap convex) iff `Q·(2p-1) ≤ 2b`. Economic meaning: **price slippage is a brake**. Buying more shares raises the price, making each additional share more expensive. This negative feedback prevents multiple equilibria.

Group version (Proposition 4): `v^T H v = (1/b) Σ_k p_k (v_k - v̄)² (2 + (Q_k - Q̄)/b)`. Covariance cross-terms cancel exactly. PSD iff `Q̄ - Q_k ≤ 2b` for all k, simplifying to cap_group ≤ 2b.

### The Diluted Influence Condition (Theorem 8)

Sufficient condition: for every MM k and every market/group, the fill influence is bounded relative to temperature. When DIC holds: cap convex → Lagrangian concave → unique KKT → Frank-Wolfe finds global optimum.

**Practical limitation:** At realistic annealing temperatures (b₀ = $0.10), DIC typically fails. Cap_group ~$140 requires b ≥ $70. The DIC is a theoretical certificate, not a runtime guarantee.

### Why unconditional uniqueness is impossible

The symmetric counterexample: two identical markets, symmetric MM → two KKT points with DIFFERENT prices. The entropy curvature (from C_b) and the budget non-convexity (from cap_k) both scale as O(1/b) — neither dominates. This is the fundamental reason.

### Generic uniqueness (Theorem 9) — the main new result

**Theorem 9:** For any b > 0, the set of parameters with multiple KKT points has Lebesgue measure zero.

**Proof structure:**
1. **Sard/transversality:** KKT system is C^∞ for b > 0. Parametric Transversality Theorem → for generic parameters, all KKT points are non-degenerate (isolated, finitely many).
2. **Homotopy from large b:** At b₀ > ||A||∞/2, contraction bound gives unique KKT point. As b decreases, KKT points trace smooth paths (implicit function theorem). New points appear only via saddle-node bifurcation (codimension-1).
3. **Global max persistence:** Bifurcation-born local maxima have welfare strictly below the existing global max (generically). The global max is the smooth continuation of the unique optimum at b₀.

**Corollary:** Annealed Frank-Wolfe with b₀ ≥ ||A||∞/2 converges to global optimum for generic parameters.

**Economic meaning:** Multiple optima require exact parameter symmetry (measure zero). Real order books are generically unique. DIC is a checkable certificate for specific instances; Theorem 9 says you almost never need it.

### Demand diameter bound (Proposition 5)

When multiple KKT points exist (measure-zero case), their demands can't be far apart:

```
||D¹ - D²||₂ ≤ b · Σ_k μ_k Q̄_k / p_min
```

**Key corollary:** μ = 0 (no binding budgets) → demands are identical. The strict concavity of C_b fully controls the problem when budgets are slack.

**Proof:** Midpoint argument. The entropy gain at (q¹+q²)/2 is quadratic: δ ≥ (p_min/2b)||D¹-D²||². The cap perturbation is linear: ε ≤ Σ_k μ_k Q̄_k/(4b) · ||D¹-D²||. Since the midpoint can't beat the optimum, δ ≤ ε, giving the bound.

### The expenditure perspective (Eisenberg-Gale)

Change variables to expenditure e_i = c_i(p)·q_i → budget becomes LINEAR: Σ e_i ≤ B_k. But welfare becomes w_i·e_i/c_i(p) — rational in prices.

In Fisher markets (Eisenberg & Gale 1959), the analogous program is convex because agents have diminishing returns (log utility). Our MMs have constant marginal returns (linear welfare). **This is the structural reason** prediction market clearing is harder than Fisher market equilibrium.

### Risk-averse clearing (Theorem 10) — the Eisenberg-Gale convex program

**The key insight**: you don't even need the expenditure substitution. Budget constraints vanish INTO the objective:

```
P_b^RA: max_{q ∈ C}  Σ_k B_k ln(Σ_{i∈MM_k} w_i q_i) + Σ_{j∉MM} w_j q_j - C_b(D(q))
```

**Why it's convex**: B_k ln(linear) is concave (log of linear). Linear terms are concave. -C_b is concave (C_b is convex). Sum of concave = concave. Strictly concave from the ln term.

**Budget absorption (the Eisenberg-Gale magic)**: No budget constraints appear anywhere! KKT conditions for interior MM fill i of agent k: B_k w_i / U_k = p_{m(i)}. Multiplying by q_i, summing: B_k/U_k · Σ w_i q_i = B_k/U_k · U_k = B_k = Σ c_i(p) q_i. Each MM automatically spends exactly its budget.

**The C_b interaction is trivial**: In the risk-neutral model, entropy smoothing was the ONLY source of concavity → isospectral deadlock (both O(1/b)). In risk-averse model, B_k ln U_k provides b-independent curvature O(B_k/U_k²). The -C_b term adds MORE concavity. Even at b=0, the program is strictly concave. **Annealing is unnecessary.**

**Fisher market isomorphism**: P_b^RA is structurally isomorphic to the Eisenberg-Gale Fisher market program. Prediction markets extend Fisher markets in two ways: (1) endogenous supply (minting), (2) mixed population (retail + MM). Both preserve concavity.

**What changes economically**: MMs with larger B_k have proportionally more price influence, but with diminishing marginal impact. The ln naturally diversifies MM capital across markets — enforces flash-liquidity without protocol constraints.

**Open question**: How much welfare does P_b^RA sacrifice vs risk-neutral LP? When B_k >> U_k, ln approaches linear → gap should vanish. Formalizing this convergence would establish risk-averse clearing as a practical drop-in.

### Landscape of uniqueness results

| Result | Condition | What's unique | Strength |
|--------|-----------|---------------|----------|
| Proposition 6 | No budgets | Prices (Fenchel dual) | Unconditional |
| Theorem 8 | DIC holds | Fills + prices | Checkable certificate |
| Contraction | b > \|\|A\|\|∞/2 | Everything (exponential) | Checkable, global |
| Proposition 5 | μ = 0 | Demands + prices | Unconditional (slack budgets) |
| Theorem 9 | Generic θ | Everything | Almost everywhere |
| Theorem 10 | Kelly utility (B_k ln U_k) | Everything | Unconditional (modified model) |
| Proposition 7 | — | — | **Impossible** (risk-neutral) |

### The Fenchel dual (Propositions 6-7) — structural impossibility

**Unconstrained (no budgets):** The Fenchel dual is `min_p [W*(p) + C_b*(p)]` where W* is consumer surplus (convex PL) and C_b* is negative entropy (strictly convex). Sum is strictly convex → **unique prices, unconditionally** (Proposition 6).

**Budget-constrained:** Replace W with W_B (includes cap constraint). The cap depends on D through softmax at rate O(1/b), making W_B non-concave. Its conjugate W_B* is non-convex with curvature O(1/b) — the **same scale as the entropy Hessian** O(1/(pb)).

**Proposition 7 (Cross-Price Obstruction):** The standard monotonicity argument for price uniqueness fails because of *cross-price budget violation*. Given two KKT points, strict convexity of C_b* gives ⟨D¹-D², p¹-p²⟩ > 0. But to force a contradiction, we need the fills of KKT point 1 to be budget-feasible at KKT point 2's prices — and there's no guarantee of this. If p² is higher where the MM is long, E_k(q¹, p²) > B_k. This is a **Generalized Nash Equilibrium Problem (GNEP)**: the feasible set depends on the dual variable. Both the entropy curvature and the cross-price violation scale as O(1/b), preventing either from dominating.

**The risk-averse escape:** Replace linear welfare with Kelly/log utility: U_k = Σ w_i ln(1 + q_i/s_i). The log Hessian O(w/q²) is independent of b, breaking the deadlock. This gives unconditional uniqueness for the modified model. Economically well-motivated (real MMs use Kelly-like sizing).

### Key references for uniqueness

- McKelvey & Palfrey (1995): QRE uniqueness at high temperature — our analog is Theorem 8 (DIC)
- Hofbauer & Sandholm (2007): unique logit equilibrium for NSD games — covers unconstrained case
- Devanur & Dudík (2015): price uniqueness for budget-constrained sequential LMSR — budget additivity hints at hidden convexity
- Rockafellar (2023): tilt stability at non-degenerate KKT points — mechanism underlying Theorem 9's homotopy
- Abraham & Robbin (1967); Debreu (1970): Parametric Transversality Theorem — standard in economic theory
- Eisenberg & Gale (1959): convex program for Fisher market equilibrium — identifies the structural gap (diminishing returns)

## Approaches tried and their evolution

### v1: Homotopy / differential topology (§8.8 v1)

Original approach: prove uniqueness via homotopy continuation from b=∞ to b=0 using Sard's theorem and parametric transversality. Abandoned because non-constructive, wrong audience, and the Lagrangian concavity approach (DIC) was cleaner.

### v2: Lagrangian concavity / DIC (§8.8 v2, Theorem 8)

The constructive approach: prove cap(q) is convex under DIC → Lagrangian is concave → unique KKT. Solid math (Propositions 3-4, Theorem 8). But DIC fails at practical temperatures — it's a theoretical certificate, not a runtime guarantee.

### v3: Generic uniqueness + Fenchel dual + impossibility + convex fix (§8.8 v3, current)

**Combined approach:**
- **Theorem 9** (generic uniqueness via Sard — ironically the same tool from v1, now used properly)
- **Proposition 5** (demand diameter bound — quantifies how bad non-uniqueness can be)
- **Proposition 6** (unconstrained price uniqueness via Fenchel dual — clean positive result)
- **Proposition 7** (cross-price obstruction — proves unconditional uniqueness is impossible for risk-neutral MMs)
- **Theorem 10** (risk-averse Eisenberg-Gale program — unconditional uniqueness for modified model)

The key insight: **unconditional uniqueness is provably impossible** for the risk-neutral model (Prop 7), but the modified model (Theorem 10) is convex and admits a clean solution. The risk-neutral obstruction is structural (GNEP from cross-price budget violation), not a technique gap. Theorem 10 resolves it by changing the economics (diminishing returns), not by finding cleverer math for the linear case.

## Paper Strategy

### Positioning

**"Prediction Markets Are Fisher Markets."** Applied mechanism design paper with a clean theoretical core. The Fisher market isomorphism (Theorem 5) is the crown jewel. Everything else is foundation or appendix.

### Final structure (6 sections + appendix, ~11 pages)

1. **Introduction** — state main result upfront: prediction markets with risk-averse MMs = Fisher markets
2. **Batch Auction Clearing and LMSR** — compressed foundation: minting cost, Fenchel duality diagram, LMSR = smoothed LP, self-financing, group scaling O(K). (~3 pages)
3. **The Budget Obstacle** — bilinear constraint, cross-price obstruction (Prop 3), GNEP structure. Why risk-neutral is provably non-convex. (~1.5 pages)
4. **Why Market Makers Have Diminishing Returns** — economic case: Kelly/Breiman survival, order ladders, budget = risk aversion, inventory risk, repeated batches. (~1.5 pages)
5. **Risk-Averse Batch Clearing** — Theorem 5 (main result): convex program, budget absorption proof, Fisher market isomorphism table. (~2.5 pages)
6. **Discussion** — landscape table, open problems, prior work. (~1.5 pages)
7. **Appendix A** — risk-neutral limit: Frank-Wolfe, DIC, generic uniqueness, convergence. (~3 pages)

### What TO claim

- **Prediction markets are Fisher markets** — Theorem 5 is the main result: unconditional uniqueness + polynomial solvability + no annealing for risk-averse MMs
- **Impossibility for risk-neutral** — Proposition 3 settles the negative direction (GNEP)
- **The economic case** — log utility is the correct model, linear is the approximation (§4)
- LMSR–LP unification via Fenchel duality (foundation, not contribution)
- Self-financing minting, O(K) group scaling (practical engineering wins)

### What NOT to claim

- Don't claim the Fenchel duality is new math (it's well-packaged known results)
- Don't claim the DIC/Frank-Wolfe/generic uniqueness are the main results (they're appendix material for the risk-neutral fallback)
- Don't present Theorem 5 as "a fix for a technical obstacle" — it IS the paper

## Key References

**Core LMSR / prediction markets:**
- Abernethy, Chen, Vaughan (2013): cost-function AMMs ↔ convex optimization ↔ conjugate duality
- Fortnow et al. (2005): LP for combinatorial call markets
- Agrawal, Wang, Ye (2011): convex pari-mutuel call auction
- Hanson (2003): LMSR
- Chen, Pennock (2007): bounded-loss market makers
- Budish, Cramton, Shim (2015): FBAs for equity markets

**Uniqueness / equilibrium:**
- McKelvey, Palfrey (1995): Quantal Response Equilibria (logit uniqueness)
- Hofbauer, Sandholm (2007): unique logit equilibrium for NSD games
- Devanur, Dudík (2015): price uniqueness for budget-constrained LMSR via Bregman divergence

**Optimization theory:**
- Rockafellar (2023): variational convexity, tilt stability
- Eisenberg, Gale (1959): convex program for Fisher markets
- Abraham, Robbin (1967): Parametric Transversality Theorem
- Debreu (1970): generic finiteness of Walrasian equilibria
- Lacoste-Julien (2016): Frank-Wolfe convergence for non-convex objectives

## Practical notes

- With Theorem 5 (risk-averse clearing), annealing is unnecessary. One convex program, one solution. Interior-point method or projected gradient.
- For the risk-neutral fallback (Appendix A): LP solve time ~1s at extreme scale (100k markets). Annealing with 50 iterations = 50s.
- The key implementation question: how much welfare does P_b^RA sacrifice vs risk-neutral LP? When B_k >> U_k, gap should vanish. Need empirical validation.
- α = 1 (log/Kelly) is a qualitative phase transition, not a point on a spectrum. α < 1 (CRRA) does NOT absorb budgets — the Eisenberg-Gale trick requires exactly log. Don't parameterize α.

## Files

- `design/paper.typ` — main paper draft (7 pages, outdated — to be merged with lmsr-proof.typ)
- `design/lmsr-proof.typ` — restructured proof sketch: "Prediction Markets Are Fisher Markets" (~11 pages)
- `design/problem-statement.md` — LP formulation
- `design/solution-approaches.md` — survey of 6 approaches to MM budgets
- `design/lmsr-frank-wolfe-notes.md` — this file
