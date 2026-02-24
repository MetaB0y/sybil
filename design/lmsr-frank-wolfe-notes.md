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

## The Uniqueness Question (Theorem 8)

### The breakthrough: budget slippage convexity

The Lagrangian L(q, μ) = f_b(q) - μ·cap(q) + μ·B is concave in q when cap(q) is convex. We computed the Hessian of the capital function cap = p(q)·q:

```
d²(p·Q)/dQ² = p(1-p)/b · [2 + Q(1-2p)/b]
```

This is positive (cap convex) iff `Q·(2p-1) ≤ 2b`. Economic meaning: **price slippage is a brake**. Buying more shares raises the price, making each additional share more expensive. This negative feedback prevents multiple equilibria.

### The Diluted Influence Condition

An instance satisfies Diluted Influence at temperature b if for every MM k and every market m:

```
Q_m^k · (2p_m - 1)⁺ ≤ 2b
```

When this holds: cap is convex → Lagrangian is concave → unique KKT point → Frank-Wolfe finds global optimum (Theorem 8).

### When Diluted Influence holds naturally

- **Large b** (high temperature): automatic for first phase of annealing
- **Retail dilution**: MM fills small relative to total volume
- **Buying underdogs** (p < 0.5): condition is unconditional
- **Flash liquidity** (spreading across many markets): Q per market stays small

### When it fails

- MM dominates a market (Q >> 2b): price saturates (sigmoid flattens), slippage vanishes
- Very small b (near-LP): sigmoid is steep, moderate fills exceed 2b
- Symmetric counterexample: two identical markets, symmetric MM → two KKT points

### The annealing rescue

Even when Diluted Influence fails at low b: Algorithm 2 starts at high b (condition holds, unique global optimum). As b decreases, the optimum traces a smooth path. The annealing trajectory tracks it, staying in the correct basin. So the condition only needs to hold at the START of annealing.

### What's proven vs. open

**Proven:**
- Budget slippage convexity condition (Proposition 3)
- Global optimality under Diluted Influence for independent markets (Theorem 8)
- Frank-Wolfe convergence to KKT at O(1/√t) (Theorem 7)

**Proven (group extension, Proposition 4):**
- Full group Hessian quadratic form: `v^T H v = (1/b) Σ_k p_k (v_k - v̄)² (2 + (Q_k - Q̄)/b)`
- Group DIC: cap_group ≤ 2b (MM's capital on a group must be at most 2b)
- Theorem 8 now covers groups — the conjecture about "restorative cross-terms" was correct and proven via the quadratic form (covariance terms cancel exactly)

**Partially proven:**
- Annealing continuation: implicit function theorem gives smooth path tracking, but no formal error bound across temperature steps

**Open:**
- Unconditional uniqueness (without Diluted Influence) for generic instances — Devanur-Dudík price uniqueness via KL divergence is promising but needs formalization for batch auctions
- Explicit convergence rates for the full annealing schedule

### Connection to QRE (Quantal Response Equilibria)

Diluted Influence is the prediction-market analog of the contraction condition for Logit QRE (McKelvey & Palfrey 1995). Agents choose via softmax, uniqueness holds when temperature is high enough relative to payoff sensitivity. Same math, different domain.

### Quantitative contraction threshold (from deep research)

Softmax is exactly 1/(2b)-Lipschitz (tight bound, improves commonly assumed 1/b). This gives a concrete threshold: the best-response Jacobian has spectral radius < 1 when `b > ||A||∞ / 2` where A is the demand matrix. For the annealing schedule: set b₀ ≥ ||A||∞ / 2 to guarantee the first phase finds the global optimum.

### Three fallback layers when Diluted Influence fails

1. **Price uniqueness via Bregman divergence** (Devanur & Dudík 2015): Even when primal fills are non-unique, clearing PRICES are unique. KL(p || p') > 0 for distinct price vectors. Their result is for sequential LMSR, but the mechanism (strict convexity of KL) transfers to our batch auction via strict concavity of f_b in demand space.

2. **Local uniqueness via variational convexity** (Rockafellar 2023): The augmented Lagrangian L_r = L + (r/2)Σ(cap_k - B_k)² is locally strongly convex-concave near any regular KKT point. This gives tilt stability (Lipschitz continuity of the optimum w.r.t. parameters) without global concavity.

3. **Annealing continuation**: Start at high b (globally unique), track the smooth path as b decreases. Layer 2 prevents bifurcation at regular points.

### Negative semidefinite games (Hofbauer & Sandholm 2007)

Our market is a "negative semidefinite game": buying more of outcome k raises p_k, reducing the payoff to further buyers (self-defeating externality). Hofbauer & Sandholm proved unique logit equilibrium for this class. Their result covers the unconstrained case; our Theorem 8 extends to budget constraints.

### Deep research assessment

The deep research confirmed:
- `cap(q) = p(q)·q` is NOT globally convex (matches our Proposition 3)
- High-temperature uniqueness is real and well-established (QRE, contraction mapping)
- The Diluted Influence condition is the right structure
- Devanur-Dudík 2015 and Hofbauer-Sandholm 2007 are the key references to cite

Overclaimed by deep research:
- "The hypothesis is fundamentally correct and entirely resolvable" — too strong. The four "parallel guarantees" are really four angles on the same question
- Gorissen hidden convexity "structural prerequisites align identically" — handwaved, not proven for our softmax case
- Budget additivity transferring to batch auctions — open question, not settled

## Approaches tried and abandoned

### Homotopy / differential topology (§8.8 v1)

Original approach: prove uniqueness via homotopy continuation from b=∞ to b=0 using Sard's theorem and parametric transversality. Problems:
- **Non-constructive**: Sard gives "for generic parameters" but doesn't tell you which parameters fail
- **Boundary nightmare**: polytope has exponentially many faces, face-by-face argument is tedious
- **Uniform regularity gap**: need Jacobian non-singularity along the entire path, not just at each point
- **Wrong audience**: differential topology is not in the toolbox of market design / optimization people

Replaced by the Lagrangian concavity argument, which is constructive, gives a checkable condition, and uses standard optimization tools.

## Paper Strategy

### Positioning

Applied systems / market design paper, not pure theory. "We bridge the 15-year gap between LMSR theory and MEV-resistant practice."

### Proposed structure (streamlined, 6 sections)

**Part I: Core Mechanism (Convex & Unconstrained)**
1. **Introduction & Setup**: LP batch auction + group minting
2. **LMSR as Entropy-Smoothed LP**: Fenchel duality, LSE sandwich, main theorem (merge old §§2-4)
3. **Endogenous Liquidity & Scaling**: Self-financing + O(K) scaling (merge old §§6-7)

**Part II: Real-World Friction (Non-Convex Budgets)**
4. **Budget Constraints via Frank-Wolfe**: Lagrangian relaxation, algorithm, O(1/√t) convergence
5. **Global Optimality & Negative Feedback**: Budget slippage convexity, Diluted Influence Condition, uniqueness
6. **Experimental Results & Conclusion**: Empirical validation, runtime comparisons

### What NOT to claim

- Don't claim new math (the Fenchel duality is applied, not invented)
- Don't claim the pipeline replacement is a contribution (it was obviously bad)
- Don't over-claim unconditional uniqueness (Theorem 8 has the Diluted Influence condition)

### What TO claim

- Self-financing minting (Theorem 4) — clean, original
- O(K) scaling via group minting vs O(2^K) combinatorial LMSR — practical
- Frank-Wolfe budget handling with LP oracle — novel algorithm for this domain
- **Budget slippage convexity** — the genuinely novel insight (Proposition 3 + Theorem 8)
- Diluted Influence Condition — checkable, economically meaningful sufficient condition for global optimality

## Key References

- Abernethy, Chen, Vaughan (2013): cost-function AMMs ↔ convex optimization ↔ conjugate duality
- Fortnow et al. (2005): LP for combinatorial call markets
- Agrawal, Wang, Ye (2011): convex pari-mutuel call auction
- Hanson (2003): LMSR
- Budish, Cramton, Shim (2015): FBAs for equity markets
- Chen, Pennock (2007): bounded-loss market makers
- McKelvey, Palfrey (1995): Quantal Response Equilibria (logit uniqueness)
- Gorissen, den Hertog, Reusken (2022): hidden convexity in bilinear programs
- Fiacco (1983): sensitivity and stability analysis in NLP (local uniqueness via LICQ+SOSC)
- Luo, Pang, Ralph (1996): MPECs (Mathematical Programs with Equilibrium Constraints)
- Lacoste-Julien (2016): Frank-Wolfe convergence for non-convex objectives

## Practical notes

- LP solve time ~1s at extreme scale (100k markets), not 1ms. Annealing with 50 iterations = 50s. Warm-starting should help dramatically (simplex pivots from previous basis).
- Current SLP does 2 iterations with hand-wavy "it's enough". Frank-Wolfe gives principled convergence guarantees.
- The same temperature b controls LMSR smoothing (§§1-7) and annealing (§8). This is the paper's unifying idea.

## Files

- `design/paper.typ` — main paper draft (7 pages)
- `design/lmsr-proof.typ` — proof sketch with Frank-Wolfe + uniqueness (~12 pages)
- `design/problem-statement.md` — LP formulation
- `design/solution-approaches.md` — survey of 6 approaches to MM budgets
- `design/lmsr-frank-wolfe-notes.md` — this file
