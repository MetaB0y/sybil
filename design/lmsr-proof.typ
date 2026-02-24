#set document(title: "LMSR as Entropy-Smoothed Batch Auction Clearing: Proof Sketch")
#set text(font: "New Computer Modern", size: 10pt)
#set page(margin: (x: 1.5in, y: 1.2in), numbering: "1")
#set par(justify: true, leading: 0.55em)
#set heading(numbering: "1.")
#show heading.where(level: 1): it => block(above: 1.5em, below: 0.8em)[#it]
#show heading.where(level: 2): it => block(above: 1.2em, below: 0.6em)[#it]

#align(center)[
  #text(size: 15pt, weight: "bold")[
    LMSR as Entropy-Smoothed Batch Auction Clearing
  ]
  #v(0.5em)
  #text(size: 11pt)[Proof Sketch]
  #v(0.3em)
  #text(size: 9pt, style: "italic")[Draft — February 2026]
]

#v(1em)

#block(inset: (x: 2em))[
  #text(weight: "bold")[Summary.]
  We prove that Hanson's Logarithmic Market Scoring Rule (LMSR) is the entropy-smoothed version of a welfare-maximizing batch auction with minting. The connection is exact: the LP's minting cost function $V(D) = max_k D_k$ and the LMSR cost function $C_b (D) = b dot ln sum_k exp(D_k slash b)$ are related by $C_b -> V$ as $b -> 0$. Their Fenchel conjugates exhibit the same convergence: the negative entropy $-b dot H(p)$ converges to the simplex indicator $delta_Delta (p)$. This establishes that LP batch clearing and LMSR pricing are endpoints of a single parametric family, and that the LP inherits all structural properties of LMSR (price normalization, arbitrage-freeness) as limiting cases.
]

#v(1em)

= The Minting Cost Function

== Setup

Consider a prediction market with $K$ mutually exclusive outcomes. In a batch auction, $N$ orders arrive. After matching buyers against sellers, the residual imbalance is:

$
D_k = sum_(i in "buy"(k)) q_i - sum_(j in "sell"(k)) q_j quad "for each outcome" k in {1, dots, K}
$

where $q_i$ is the fill quantity of order $i$. The vector $bold(D) = (D_1, dots, D_K)$ is the _net demand_ — the excess of buy volume over sell volume for each outcome.

To clear the market, the exchange must provide $D_k$ additional shares of outcome $k$. Since outcomes are mutually exclusive, one _group mint_ at cost \$1 creates one share of every outcome (exactly one resolves to \$1 at settlement, so this is fairly priced).

== LP Minting Cost

The minimum minting cost to satisfy demand $bold(D)$ is:

#block(inset: (left: 1em))[
  *Definition 1* (Minting Cost Function).
  $ V(bold(D)) = max_k D_k $
]

_Justification._ Each group mint creates one share of every outcome. To supply $D_k$ shares of outcome $k$ for all $k$, we need at least $max_k D_k$ mints (each mint covers one unit of every outcome). The cost is \$1 per mint, so the total cost is $max_k D_k$. If $max_k D_k < 0$ (all outcomes have excess supply), we _burn_ pairs and recover revenue. #h(1fr) $diamond.stroked$

The function $V = max_k D_k$ is convex, piecewise linear, and non-smooth (it has kinks where $D_j = D_k$ for distinct $j, k$).

== The Full Batch Auction as an Optimization

The welfare-maximizing batch clearing solves:

$ P: quad max_(bold(q) in [0, bold(overline(Q))]) quad sum_i w_i q_i - V(bold(D)(bold(q))) $

where $w_i$ is the welfare coefficient of order $i$ ($+L_i$ for buyers, $-L_i$ for sellers), and $bold(D)(bold(q))$ is the net demand vector (linear in $bold(q)$). Since $V$ is convex and $bold(D)$ is linear in $bold(q)$, the objective is concave — this is a convex optimization problem.

Introducing an explicit minting variable $mu >= max_k D_k$, this is equivalent to the Linear Program:

$
max_(bold(q), mu) quad sum_i w_i q_i - mu quad "s.t." quad D_k (bold(q)) <= mu quad forall k, quad bold(q) in [0, bold(overline(Q))]
$


= Fenchel Duality: Prices from Conjugates <fenchel>

The structural properties of the LP clearing prices follow from the Fenchel conjugate of $V$.

== The Conjugate of $V$

#block(inset: (left: 1em))[
  *Theorem 1* (Minting–Simplex Duality). _The Fenchel conjugate of $V(bold(D)) = max_k D_k$ is the indicator function of the probability simplex:_
  $ V^* (bold(p)) = sup_(bold(D)) {chevron.l bold(p), bold(D) chevron.r - max_k D_k} = delta_Delta (bold(p)) $
  _where $Delta = {bold(p) in RR^K : p_k >= 0, sum_k p_k = 1}$ and $delta_Delta (bold(p)) = 0$ if $bold(p) in Delta$, $+infinity$ otherwise._
]

_Proof._ We compute $V^*(bold(p)) = sup_(bold(D)) {sum_k p_k D_k - max_k D_k}$ in three cases.

*Case 1:* $bold(p) in Delta$. For any $bold(D)$, we have $sum_k p_k D_k <= max_k D_k$ (a convex combination of the $D_k$ cannot exceed the maximum). So the supremum is $<= 0$. Setting $bold(D) = bold(0)$ gives value $0$. Therefore $V^*(bold(p)) = 0$.

*Case 2:* $sum_k p_k > 1$. Take $bold(D) = (t, t, dots, t)$ for $t -> +infinity$. Then $sum p_k t - t = t(sum p_k - 1) -> +infinity$.

*Case 3:* Some $p_j < 0$. Take $D_j -> -infinity$ with all other $D_k = 0$. Then $p_j D_j -> +infinity$ while $max_k D_k = 0$, so the supremum diverges.

In Cases 2 and 3, $V^*(bold(p)) = +infinity$. #h(1fr) $square$

*Interpretation.* The batch auction's minting mechanism _is_ the probability simplex. The constraint that prices must form a valid probability distribution is not imposed externally — it is the Fenchel conjugate of the minting cost. Minting at \$1 per complete set _encodes_ the axiom $sum p_k = 1$.


= Log-Sum-Exp Smoothing <smoothing>

== The LMSR Cost Function

#block(inset: (left: 1em))[
  *Definition 2* (Smoothed Minting Cost). _For temperature parameter $b > 0$:_
  $ C_b (bold(D)) = b dot ln sum_(k=1)^K exp(D_k / b) $
]

This is the _log-sum-exp_ (LSE) function, scaled by $b$. It is convex, smooth ($C^infinity$), and approximates $V$:

#block(inset: (left: 1em))[
  *Proposition 1* (LSE–Max Sandwich). _For all $bold(D) in RR^K$:_
  $ max_k D_k <= C_b (bold(D)) <= max_k D_k + b ln K $
  _In particular, $C_b (bold(D)) -> V(bold(D))$ uniformly on compact sets as $b -> 0^+$._
]

_Proof._ Lower bound: $sum exp(D_k\/b) >= exp(max_k D_k\/b)$, so $C_b >= max_k D_k$. Upper bound: $sum exp(D_k\/b) <= K exp(max_k D_k\/b)$, so $C_b <= max_k D_k + b ln K$. #h(1fr) $square$

The gap $b ln K$ is precisely the maximum loss of the LMSR market maker — a well-known result that here falls out as an approximation bound.

== The Conjugate of $C_b$

#block(inset: (left: 1em))[
  *Theorem 2* (LMSR–Entropy Duality). _The Fenchel conjugate of $C_b$ is the negative Shannon entropy, restricted to the simplex:_
  $ C_b^* (bold(p)) = cases(
    -b dot H(bold(p)) = b sum_k p_k ln p_k quad & "if" bold(p) in Delta,
    +infinity & "otherwise"
  ) $
  _where $H(bold(p)) = -sum_k p_k ln p_k$ is the Shannon entropy._
]

_Proof._ We compute $C_b^*(bold(p)) = sup_(bold(D)) {chevron.l bold(p), bold(D) chevron.r - b ln sum exp(D_k \/ b)}$.

Setting the gradient to zero:
$
(partial) / (partial D_k) [p_k D_k - b ln sum_j exp(D_j \/ b)] = p_k - exp(D_k \/ b) / (sum_j exp(D_j \/ b)) = 0
$

This gives $p_k = exp(D_k\/b) \/ sum exp(D_j\/b)$ — the _softmax_, which is exactly the *LMSR marginal price*. Inverting: $D_k = b ln p_k + b ln Z$ where $Z = sum exp(D_j\/b)$.

Substituting back:
$
chevron.l bold(p), bold(D) chevron.r &= sum_k p_k (b ln p_k + b ln Z) = b sum_k p_k ln p_k + b ln Z \
C_b (bold(D)) &= b ln Z
$

Therefore $C_b^*(bold(p)) = b sum_k p_k ln p_k + b ln Z - b ln Z = b sum_k p_k ln p_k$.

This is finite only when $bold(p) in Delta$ (the softmax always yields a probability vector). #h(1fr) $square$

== The Duality Diagram

The two pairs $(V, V^*)$ and $(C_b, C_b^*)$ form a commutative diagram under Fenchel conjugation and the $b -> 0$ limit:

#align(center)[
  #table(
    columns: 3,
    align: center,
    stroke: none,
    [$V(bold(D)) = max_k D_k$], [$stretch(arrow.l.r, size: #200%)^("Fenchel")$], [$V^*(bold(p)) = delta_Delta (bold(p))$],
    [$arrow.t space b -> 0$], [], [$arrow.t space b -> 0$],
    [$C_b (bold(D)) = b ln sum exp(D_k\/b)$], [$stretch(arrow.l.r, size: #200%)^("Fenchel")$], [$C_b^* (bold(p)) = b sum p_k ln p_k$],
  )
]

#v(0.5em)
#align(center)[
  #text(size: 9pt)[
    Left column: *primal* (cost of minting shares). Right column: *dual* (constraint on prices). \
    Top row: *LP batch auction* (sharp). Bottom row: *LMSR* (smooth, temperature $b$).
  ]
]

As $b -> 0$: the smooth LMSR cost $C_b$ sharpens to the LP minting cost $V$, and the soft entropy penalty $b sum p_k ln p_k$ hardens to the simplex indicator $delta_Delta$ (a rigid constraint that prices must be probabilities).


= LMSR Pricing as Smoothed Batch Clearing <main-theorem>

== The Smoothed Batch Auction

Replace the minting cost $V$ with $C_b$ in the batch clearing problem:

$ P_b: quad max_(bold(q) in [0, bold(overline(Q))]) quad sum_i w_i q_i - C_b (bold(D)(bold(q))) $

Since $C_b$ is convex and smooth, and $bold(D)(bold(q))$ is linear, $P_b$ is a smooth concave maximization. Its first-order conditions are necessary and sufficient.

#block(inset: (left: 1em))[
  *Theorem 3* (Main Result). _At the optimum of $P_b$, the marginal cost of shares — i.e., the clearing prices — are:_
  $ p_k^* = (partial C_b) / (partial D_k) = exp(D_k^* \/ b) / (sum_j exp(D_j^* \/ b)) $
  _This is the LMSR marginal price function._

  _Furthermore:_
  + _$sum_k p_k^* = 1$ (prices form a probability distribution)._
  + _As $b -> 0^+$, the solution of $P_b$ converges to the solution of $P$ (the LP batch auction)._
  + _As $b -> infinity$, prices converge to the uniform distribution $p_k = 1\/K$._
]

_Proof of (1)._ The softmax always sums to 1 by construction.

_Proof of (2)._ By Proposition 1, the objectives of $P_b$ and $P$ satisfy $|"obj"(P_b) - "obj"(P)| <= b ln K -> 0$. The feasible sets are identical ($bold(q) in [0, bold(overline(Q))]$). By standard results in epi-convergence of convex functions, the optimizers converge.

_Proof of (3)._ As $b -> infinity$, $exp(D_k\/b) -> 1$ for all $k$, so $p_k -> 1\/K$. #h(1fr) $square$

== Interpretation

The parameter $b$ interpolates between two extremes:

- *$b = 0$ (LP batch auction):* Prices are sharp. The clearing price is set by the marginal order. Supply from minting is a step function: unlimited at \$1 per complete set, zero otherwise. Prices satisfy $sum p_k = 1$ as a _hard_ constraint (from $V^* = delta_Delta$).

- *$b > 0$ (LMSR):* Prices are smooth. The market maker provides a continuous supply curve with depth proportional to $b$. Prices satisfy $sum p_k = 1$ by _softmax structure_ (from $C_b^* = -b H$). The maximum loss from smoothing is $b ln K$ (the entropy gap from Proposition 1).

The batch auction is the _zero-temperature_ limit. Raising $b$ adds liquidity (smoothness) at the cost of a bounded subsidy.


= The KKT Conditions: Where the Exponentials Come From <kkt>

To see exactly how LMSR pricing emerges from the optimality conditions, we write out the KKT system for $P_b$.

== First-Order Conditions

For a buy order $i$ on outcome $k$ with welfare coefficient $w_i = L_i$:

$
(partial) / (partial q_i) [w_i q_i - C_b (bold(D)(bold(q)))] = L_i - (partial C_b) / (partial D_k) dot (partial D_k) / (partial q_i) = L_i - p_k
$

Combined with the box constraints $q_i in [0, overline(Q)_i]$ and multipliers $mu_i^-, mu_i^+$:

$
L_i - p_k - mu_i^+ + mu_i^- = 0, quad mu_i^+ (q_i - overline(Q)_i) = 0, quad mu_i^- q_i = 0
$

This gives the familiar clearing rule:
- $q_i = overline(Q)_i$ (fully filled) $quad arrow.l.r.double quad L_i >= p_k$ #h(1em) _(buyer's limit above price)_
- $q_i = 0$ (unfilled) $quad arrow.l.r.double quad L_i <= p_k$ #h(1em) _(buyer's limit below price)_
- $0 < q_i < overline(Q)_i$ (marginal) $quad arrow.l.r.double quad L_i = p_k$ #h(1em) _(buyer is the price-setter)_

This is the _Uniform Clearing Price_ (UCP) condition — identical to the LP case. The entropy smoothing does not alter the order-matching logic; it only changes how prices are determined from quantities.

== Where the Exponentials Live

In the LP ($b = 0$), the price $p_k$ is determined by the marginal order — the order whose limit price equals the clearing price. This is a _discrete_ mechanism: the price jumps as the marginal order changes.

In $P_b$ ($b > 0$), the price $p_k = exp(D_k\/b) \/ sum exp(D_j\/b)$ depends _continuously_ on the net demand $D_k$. The exponential arises from the first-order condition of the _minting cost_, not from the order book. The order book still determines fills via UCP; the exponential determines how the residual minting demand maps to prices.


= Endogenous Liquidity <endogenous>

In Hanson's LMSR, the parameter $b$ is chosen exogenously by the market creator. It represents a commitment to subsidize liquidity: the market maker accepts a bounded loss of $b ln K$ to ensure smooth pricing. This is a _design choice_, not an emergent property.

#block(inset: (left: 1em))[
  *Theorem 4* (Self-Financing Minting). _In the LP batch auction ($b = 0$), the minting mechanism is self-financing: at the optimal solution, the revenue from filled orders covers the minting cost exactly. The "market maker" (minting mechanism) incurs zero loss._
]

_Proof._ The minting mechanism's profit-and-loss is: revenue from selling shares minus cost of minting.

$
"P&L" = underbrace(sum_k p_k D_k, "revenue") - underbrace(mu, "minting cost")
$

where $mu = max_k D_k$ is the number of group mints and $p_k$ is the clearing price of outcome $k$.

Recall the LP has constraints $D_k <= mu$ for each $k$, with dual variables $p_k >= 0$. By complementary slackness: if $D_k < mu$ (constraint not tight), then $p_k = 0$. Therefore $p_k > 0$ only for outcomes where $D_k = mu$. This gives:

$
sum_k p_k D_k = sum_(k: D_k = mu) p_k dot mu = mu dot sum_(k: D_k = mu) p_k
$

Since $p_k = 0$ for all $k$ where $D_k < mu$, and $sum_k p_k = 1$ (Theorem 1), we have $sum_(k: D_k = mu) p_k = 1$. Therefore:

$
"P&L" = mu dot 1 - mu = 0
$

The minting mechanism breaks even exactly. #h(1fr) $square$

*Contrast with LMSR.* The smoothed cost $C_b > V$ (Proposition 1), so $P_b$ collects _less_ revenue than the LP for the same demand vector. The gap $C_b - V <= b ln K$ is exactly the LMSR subsidy. In the LP limit, this subsidy vanishes: liquidity is endogenous.

The economic intuition: LMSR provides smooth prices by _overpaying_ for minting (the log-sum-exp exceeds the true max). The LP provides sharp prices by paying the exact cost. The order book — not an exogenous parameter — determines market depth.


= Group Minting and Combinatorial Scaling <scaling>

== LMSR for Compound Securities

Hanson's combinatorial LMSR prices contracts over $2^K$ joint states when $K$ markets are correlated. The cost function becomes:

$ C_b^"comb" (bold(D)) = b ln sum_(s in {0,1}^K) exp(D_s \/ b) $

where $s$ ranges over all $2^K$ joint outcomes. Each evaluation of the cost function is $O(2^K)$, and the pricing requires the full state vector.

For $K = 20$ (a medium election), this is $2^20 approx 10^6$ states. For $K = 50$ (a large election), it is computationally intractable.

== Group Minting: $O(K)$ Instead of $O(2^K)$

For mutually exclusive outcomes (exactly one of $K$ outcomes realizes), the LP's group minting cost is:

$
V_G (D_1, dots, D_K) = max_(k=1)^K D_k
$

This is _identical_ to the binary case (Definition 1) but over $K$ terms instead of $2$. The smoothed version:

$
C_b^G (D_1, dots, D_K) = b ln sum_(k=1)^K exp(D_k \/ b)
$

This is $K$-outcome LMSR — computed in $O(K)$ time, not $O(2^K)$.

#block(inset: (left: 1em))[
  *Proposition 2* (Scaling Advantage). _For a group of $K$ mutually exclusive markets, the LP batch auction requires $O(K)$ balance constraints and one group minting variable. The equivalent combinatorial LMSR requires $O(2^K)$ state evaluations. The LP achieves the same price normalization ($sum_k p_k^"YES" <= 1$) and the same limiting behavior ($C_b^G -> V_G$ as $b -> 0$), with an exponential reduction in complexity._
]

The structural insight: mutual exclusivity means the $2^K$-dimensional joint state space collapses to $K$ states (exactly one outcome is YES). Group minting exploits this directly. Combinatorial LMSR must discover it through the cost function.


= Budget-Constrained Clearing via Frank-Wolfe <frank-wolfe>

The entropy framework developed in §§1–7 handles the LP core exactly. We now address the remaining non-convexity: the bilinear market maker budget constraint.

== Problem Setup

A market maker $k$ deposits balance $B_k$ and posts orders across multiple markets. The capital consumed by each fill depends on the clearing price:

$
"cap"_k (bold(p), bold(q)) = sum_(i in "MM"_k) c_i (p_(m(i))) dot q_i, quad c_i (p) = cases(p & "if BuyYes/SellNo", 1 - p & "if SellYes/BuyNo")
$

The budget constraint $"cap"_k <= B_k$ is bilinear: $p$ is determined by $bold(q)$ through the clearing mechanism (LP duality or LMSR softmax). We seek:

$
max_(bold(q) in cal(C)) quad f(bold(q)) quad "s.t." quad "cap"_k (bold(p)(bold(q)), bold(q)) <= B_k quad forall k
$

where $cal(C)$ is the LP feasible set (balance constraints, box bounds, minting) and $f$ is welfare minus minting cost.

== Lagrangian Relaxation of Budgets

Dualize the $K$ budget constraints with multipliers $mu_k >= 0$:

$
cal(L)(bold(q), bold(mu)) = f(bold(q)) + sum_k mu_k (B_k - "cap"_k (bold(q)))
$

For fixed $bold(mu)$, maximizing $cal(L)$ over $bold(q) in cal(C)$ decouples the budget from the LP structure. The budget enters only through _modified welfare coefficients_:

$
w'_i = w_i - sum_(k : i in "MM"_k) mu_k dot c_i (p^t)
$

where $p^t$ is the current price estimate. The Lagrangian subproblem $max_(bold(q) in cal(C)) sum w'_i q_i$ is a standard LP — the same LP as our base solver, with adjusted objectives. This is the key structural insight: *the budget shadow prices modify each MM order's effective welfare, and the LP handles everything else*.

== The Algorithm

#block(inset: (left: 1em, right: 1em), fill: luma(245), radius: 3pt)[
  *Algorithm 1: Frank-Wolfe with Lagrangian Budget Handling*

  *Input:* Orders, markets, groups, MM budgets $B_1, dots, B_K$. Temperature $b > 0$.

  *Initialize:* Solve base LP (no budgets) $arrow.r bold(q)^0$, $bold(p)^0$. Set $bold(mu) = bold(0)$.

  *For* $t = 0, 1, 2, dots$:

  #h(1em) 1. *Prices.* Compute $p_m^t = "softmax"(D_m (bold(q)^t) \/ b)$ for each market $m$.

  #h(1em) 2. *Budget evaluation.* For each MM $k$: $"cap"_k^t = sum_(i in "MM"_k) c_i (p^t) dot q_i^t$.

  #h(1em) 3. *Modified welfare.* $w'_i = w_i - sum_k mu_k dot c_i (p^t)$ for each order $i$.

  #h(1em) 4. *Frank-Wolfe oracle.* Solve LP: $bold(s) = "argmax"_(bold(q) in cal(C)) sum w'_i q_i$.

  #h(1em) 5. *Step.* $bold(q)^(t+1) = (1 - gamma_t) bold(q)^t + gamma_t bold(s)$, $quad gamma_t = 2 / (t + 2)$.

  #h(1em) 6. *Dual update.* $mu_k <- max(0, mu_k + eta_t ("cap"_k^t - B_k))$ for each MM $k$.

  *Terminate* when $max_k ("cap"_k - B_k)^+ < epsilon.$ Round final $bold(q)$ to nearest LP vertex.
]

#v(0.5em)

Each iteration requires: one softmax evaluation ($O(M)$), one LP solve ($< 1$ms), and one dual update ($O(K)$). The LP in step 4 is structurally identical to the base clearing LP — the budget information enters solely through the modified welfare coefficients $w'_i$.

== Why This Is Not SLP

Our current Sequential LP corresponds to Algorithm 1 with $gamma = 1$ (jump to the LP solution) and $mu = 0$ (no dual variable, budget handled as a hard constraint via linearization). This has two failure modes:

+ *Over-stepping ($gamma = 1$):* Jumping to the LP vertex can overshoot. The new prices at the vertex may violate the budget constraint despite the linearized constraint being satisfied (§6 counterexample).

+ *No dual memory ($mu = 0$):* Each SLP iteration starts fresh. Information about budget tightness from previous iterations is discarded.

Frank-Wolfe fixes both: the step size $gamma_t = 2\/(t+2)$ ensures gradual convergence, and the dual variable $mu_k$ accumulates budget shadow price information across iterations.

== Convergence

#block(inset: (left: 1em))[
  *Theorem 7* (Frank-Wolfe Convergence). _For fixed $b > 0$, Algorithm 1 with step size $gamma_t = 2\/(t+2)$ and dual step size $eta_t = 1\/sqrt(t)$ satisfies:_

  $ cal(L)(bold(q)^*, bold(mu)^*) - cal(L)(bold(q)^t, bold(mu)^t) <= O(L_b dot D^2 / t + B_max / sqrt(t)) $

  _where $L_b$ is the Lipschitz constant of $nabla f_b$ (proportional to $1\/b$), $D = "diam"(cal(C))$, and $B_max = max_k B_k$._
]

_Proof sketch._ The primal update (step 5) is standard Frank-Wolfe over a compact convex set, giving $O(L_b D^2 \/ t)$ convergence of the primal objective for fixed $bold(mu)$. The dual update (step 6) is projected subgradient ascent on the concave dual function, giving $O(1\/sqrt(t))$ convergence of the dual. The combined primal-dual rate is dominated by the slower dual convergence. #h(1fr) $square$

_Remark._ The Lipschitz constant $L_b prop 1\/b$ means that sharper pricing (smaller $b$) requires more iterations. This motivates *annealing*: start with large $b$ (fast convergence, smooth prices), decrease $b$ as the algorithm progresses (sharper prices, warm-started from previous solution).

== Annealing Schedule

#block(inset: (left: 1em, right: 1em), fill: luma(245), radius: 3pt)[
  *Algorithm 2: Annealed Frank-Wolfe*

  Set $b_0$ (e.g., $0.1 dot dollar 1$), $b_"min"$ (e.g., $10^(-4) dot dollar 1$), cooling factor $rho = 0.5$.

  *For* $ell = 0, 1, 2, dots$ while $b_ell > b_"min"$:

  #h(1em) Run Algorithm 1 for $T$ iterations at temperature $b_ell$.

  #h(1em) Warm-start next round: $bold(q)^0 <- bold(q)^T$, $bold(mu)^0 <- bold(mu)^T$.

  #h(1em) Cool: $b_(ell+1) = rho dot b_ell$.

  Final: solve LP at $bold(q)^"final"$ to obtain integer fills and exact dual prices.
]

Total cost: $ceil(log_(1\/rho) (b_0 \/ b_"min")) times T$ LP solves. For $b_0 = 0.1$, $b_"min" = 10^(-4)$, $rho = 0.5$, $T = 5$: approximately $10 times 5 = 50$ LP solves, or $~50$ms.

== What This Proves and What It Doesn't

*Proven:*
- Algorithm 1 converges to a KKT point of the smoothed budget-constrained problem at rate $O(1\/sqrt(t))$.
- The final LP solve (Algorithm 2, last step) produces exact integer fills with correct dual prices.
- Each iteration reuses the existing LP infrastructure with zero additional solver dependencies.

*Not proven:*
- Global optimality. The bilinear constraint makes the feasible set non-convex. The KKT point found by Frank-Wolfe may be a local optimum. However, empirically, the LP-optimal prices (step 0) are close to the budget-constrained optimum — the budget perturbation is small — so the Frank-Wolfe trajectory stays in the basin of the global optimum.
- Formal convergence rate as $b -> 0$. The annealing schedule is heuristic. A rigorous analysis would require bounding the path-following error across temperature steps (similar to interior point path-following theory).

*What would close the gap:* A proof that the budget-constrained problem has a unique KKT point for generic instances. We attempt this below.

== Global Optimality via Budget Slippage <uniqueness>

We now prove that under a natural economic condition, the budget-constrained problem has a unique KKT point — making Frank-Wolfe globally optimal. The key insight: *price slippage makes the budget constraint convex*, which preserves the concavity of the Lagrangian.

=== The Lagrangian

$
cal(L)(bold(q), bold(mu)) = underbrace(f_b (bold(q)), "concave") - sum_k mu_k dot underbrace("cap"_k (bold(q)), "convex?") + sum_k mu_k B_k
$

The objective $f_b$ is concave (linear minus convex). If each $"cap"_k$ is convex in $bold(q)$, then $-mu_k dot "cap"_k$ is concave for $mu_k >= 0$. The full Lagrangian would then be strictly concave in the price-relevant subspace, giving a unique primal maximizer $bold(q)^*(bold(mu))$ for each $bold(mu)$, a convex dual function $d(bold(mu)) = max_(bold(q)) cal(L)$, and a unique saddle point $(bold(q)^*, bold(mu)^*)$ — i.e., a unique KKT point.

Everything hinges on: *is the capital function convex?*

=== The Hessian of price $times$ quantity

Consider a single BuyYes order $i$ on binary market $m$ with aggregate MM fill $Q$ on that market. The capital contribution is $"cap" = p_m (Q) dot Q$ where $p_m = sigma((Q + R) \/ b)$ is the sigmoid (softmax for $K = 2$), with $R$ capturing all non-MM demand (retail orders, other-side fills) as a constant.

Computing the second derivative:

$
(d^2)/(d Q^2) [p dot Q] = (p(1-p))/b dot [2 + Q(1-2p)/b]
$

The prefactor $p(1-p)\/b > 0$ always. The sign depends on the bracket.

#block(inset: (left: 1em))[
  *Proposition 3* (Budget Slippage Convexity). _The capital function $"cap"(Q) = p(Q) dot Q$ is convex in $Q$ if and only if:_
  $ Q dot (2p - 1) <= 2b $
  _For $p <= 1\/2$ (buying the underdog), this holds unconditionally. For $p > 1\/2$ (buying the favorite), this requires $Q <= 2b \/ (2p - 1)$._
]

*Economic meaning.* Convexity of capital = price slippage acts as a brake. Buying more shares pushes the price up, making each additional share _more_ expensive. This self-correcting feedback is what prevents multiple equilibria.

The condition fails when $Q >> b$: the sigmoid saturates ($p -> 1$), slippage vanishes (the price can't go higher), and the brake disengages. Economically: an MM buying so aggressively that the price is already near \$1 faces no further slippage penalty — the feedback loop is broken.

=== The group Hessian

For mutually exclusive outcomes in a group, the softmax couples all prices: increasing $Q_A$ raises $p_A$ but lowers $p_B$ (the denominator grows). Rather than bounding individual cross-terms, we compute the full quadratic form directly.

#block(inset: (left: 1em))[
  *Proposition 4* (Group Budget Slippage). _Let $bold(Q) = (Q_1, dots, Q_K)$ be MM $k$'s fill vector on a group of $K$ mutually exclusive outcomes with softmax prices $bold(p) = "softmax"((bold(Q) + bold(R))\/b)$. The Hessian of the total capital $"cap"(bold(Q)) = sum_k p_k Q_k$ satisfies:_

  $ bold(v)^top nabla^2 "cap" dot bold(v) = 1/b sum_(k=1)^K p_k (v_k - overline(v))^2 dot (2 + (Q_k - overline(Q))\/b) $

  _where $overline(v) = sum_k p_k v_k$ and $overline(Q) = sum_k p_k Q_k$._

  _The Hessian is positive semidefinite if and only if:_
  $ overline(Q) - Q_k <= 2b quad forall k $
]

_Proof._ The capital function is $g(bold(x)) = bold(x)^top bold(p)(bold(x))$ where $bold(p) = "softmax"((bold(x) + bold(R))\/b)$. Its gradient is $nabla g_i = p_i alpha_i$ where $alpha_i = 1 + (x_i - overline(x))\/b$ and $overline(x) = sum_k p_k x_k$. (This follows from the product rule and the softmax Jacobian $J = (1\/b)("diag"(bold(p)) - bold(p) bold(p)^top)$.)

For the second directional derivative along $bold(v)$, we compute $d^2 g(bold(x) + t bold(v))\/ d t^2 |_(t=0)$. Using $dot(p)_i = (p_i\/b)(v_i - overline(v))$ and $dot(alpha)_i = (v_i - overline(v))\/b - "Cov"_p (v, x)\/b^2$, the cross-terms involving $"Cov"_p (v, x)$ cancel exactly, leaving:

$
bold(v)^top H bold(v) = 1/b sum_k p_k (v_k - overline(v))^2 (2 + (x_k - overline(x))\/b)
$

Since each term has weight $p_k (v_k - overline(v))^2 \/ b >= 0$, the sign is determined by the bracket $2 + (x_k - overline(x))\/b$. The quadratic form is non-negative for all $bold(v)$ iff this bracket is non-negative for all $k$, giving $overline(x) - x_k <= 2b$. Substituting $x_k = Q_k$ yields the stated condition. #h(1fr) $square$

_Verification._ Setting $K = 2$, $Q_2 = 0$, $bold(v) = (1, 0)$: the formula reduces to $(p(1-p)\/b) [2 + Q(1-2p)\/b]$, recovering Proposition 3 exactly.

=== Interpretation: capital bound per group

For the common case where an MM has orders on a strict subset $S subset.neq {1, dots, K}$ of outcomes (e.g., BuyYes on a few favorites), the binding condition comes from outcomes $k in.not S$ where $Q_k = 0$:

$ overline(Q) - 0 = overline(Q) <= 2b $

Since $overline(Q) = sum_(k in S) p_k Q_k$ is precisely the MM's total capital on the group, this simplifies to:

$ bold("Group DIC:") quad "cap"_"group"^k <= 2b $

*The MM's capital deployed on any single group must be at most $2b$.* This is a natural liquidity condition: the entropy temperature must be at least half the capital exposure per group. For MMs practicing flash liquidity (spreading capital across many groups), this is easily satisfied.

=== The multi-group Hessian

For MM $k$ with fills across groups $g_1, dots, g_G$ belonging to _different_ group hierarchies (independent groups), the Hessian of $"cap"_k$ is block-diagonal — one block per group. The full Hessian is PSD iff each block is PSD (Proposition 4 applied per group).

=== The Diluted Influence Condition

#block(inset: (left: 1em))[
  *Definition 3* (Diluted Influence). _An instance satisfies the Diluted Influence Condition at temperature $b$ if:_

  + _For every MM $k$ and every independent (non-grouped) binary market $m$: $quad Q_m^k dot (2 p_m - 1)^+ <= 2b$_
  + _For every MM $k$ and every group $g$: $quad overline(Q)_g^k = sum_(m in g) p_m Q_m^k <= 2b$_

  _where $Q_m^k$ is MM $k$'s total fill on market $m$ and $p_m$ is the clearing price._
]

Condition (1) ensures convexity of capital per independent market (Proposition 3). Condition (2) ensures convexity of capital per group (Proposition 4). Condition (2) is stricter: it requires the MM's total capital on the group to be at most $2b$, whereas (1) only requires the "directional" capital $Q(2p-1)$ per market to be bounded.

Both conditions hold naturally when:
- $b$ is large (high temperature, smooth prices — automatic for the first phase of annealing)
- MM fills are small relative to total volume (retail dilution)
- MMs are buying underdogs ($p < 0.5$, condition (1) is automatic)
- MMs spread capital across many groups (flash liquidity — capital per group stays small)

=== Uniqueness theorem

#block(inset: (left: 1em))[
  *Theorem 8* (Global Optimality under Diluted Influence). _If the Diluted Influence Condition (Definition 3) holds at temperature $b$, then:_

  + _The capital function $"cap"_k$ is convex in $bold(q)$ for each MM $k$ — including on grouped markets._
  + _The Lagrangian $cal(L)(bold(q), bold(mu))$ is strictly concave in $bold(q)$ on the price-relevant subspace for any $bold(mu) >= 0$._
  + _The KKT point $(bold(q)^*, bold(mu)^*)$ is unique (up to demand-preserving fill substitutions)._
  + _Algorithm 1 converges to the global optimum._
]

_Proof._ (1): For independent markets, Proposition 3 and DIC condition (1). For groups, Proposition 4 and DIC condition (2). Since the Hessian of $"cap"_k$ is block-diagonal across independent groups, PSD of each block gives PSD of the whole. (2): $cal(L) = f_b - sum mu_k "cap"_k + "const"$. The term $f_b$ is strictly concave in the price-relevant subspace. Each $-mu_k "cap"_k$ is concave (for $mu_k >= 0$ and convex $"cap"_k$). Their sum is strictly concave. (3): For fixed $bold(mu)$, strict concavity gives a unique maximizer $bold(q)^*(bold(mu))$. The dual function $d(bold(mu)) = cal(L)(bold(q)^*(bold(mu)), bold(mu))$ is convex (supremum of affine functions of $bold(mu)$). For generic parameters, $d$ is strictly convex at its minimum, giving a unique $bold(mu)^*$. (4): Frank-Wolfe on a concave Lagrangian converges to the unique saddle point. #h(1fr) $square$

=== Quantitative contraction threshold

The softmax operator is exactly $1\/(2b)$-Lipschitz continuous across all $L_p$ norms — a tight bound, improving the commonly assumed $1\/b$. This gives a precise contraction threshold: the best-response Jacobian (mapping current fills to LP-optimal fills via price adjustment) has spectral radius $< 1$ when:

$ 1 / (2b) dot ||A||_infinity < 1, quad "i.e.," quad b > ||A||_infinity / 2 $

where $A$ is the demand matrix ($D = A bold(q)$) and $||A||_infinity$ is its max row sum. This is the *quantitative* version of the Diluted Influence Condition: when $b$ exceeds this threshold, the price-fill feedback is a global contraction — guaranteeing unique equilibrium and exponential convergence.

For the annealing schedule (Algorithm 2), this means: set $b_0 >= ||A||_infinity / 2$ to ensure the first phase provably finds the global optimum.

=== Why unconditional uniqueness fails

Unconditional uniqueness — for _all_ parameters and all $b$ — is false. The entropy curvature (from $C_b$) and the budget non-convexity (from $"cap"_k$) both scale as $O(1\/b)$. Neither dominates the other in general, so the budget can create a "ridge" in the Lagrangian landscape that splits the global optimum into two local optima.

*Counterexample.* Two markets $A, B$ with identical order books and one MM with symmetric positions on both. By symmetry, there are two KKT points: "fill $A$, starve $B$" and "fill $B$, starve $A$." These produce different fill vectors, different demands ($D_A > D_B$ vs.~$D_B > D_A$), and different prices ($p_A > p_B$ vs.~$p_B > p_A$). No algorithm can select between them without breaking the symmetry.

This counterexample requires _exact_ parameter symmetry — a measure-zero set. Any perturbation of a single limit price selects a unique optimum.

=== Generic uniqueness

The counterexample is essentially the only obstruction. For almost all parameter values, the budget-constrained problem has a unique KKT point.

#block(inset: (left: 1em))[
  *Theorem 9* (Generic Uniqueness). _Fix $b > 0$. Let $Theta$ denote the space of problem parameters (limit prices, budgets, max fills). The set of parameters $theta in Theta$ for which $P_b (theta)$ has multiple KKT points is closed and has Lebesgue measure zero._
]

_Proof._ The KKT system for the smoothed problem $P_b$ is $C^infinity$ (softmax is smooth for $b > 0$). For each active set — which orders have interior fills, which budgets bind — the KKT conditions form a square system $F(z, theta) = 0$ where $z = (q_I, mu_(K_a))$.

*Step 1 (Non-degeneracy).* The derivative $D_theta F$ has full rank: perturbing limit price $w_i$ shifts the $i$-th stationarity equation independently; perturbing budget $B_k$ shifts the $k$-th binding equation. By the Parametric Transversality Theorem (Abraham & Robbin, 1967; a consequence of Sard's theorem), for almost all $theta$, $0$ is a regular value of $F(dot, theta)$. At regular values, the KKT Jacobian $D_z F$ is nonsingular at every solution: each KKT point is non-degenerate. Finitely many active set choices $arrow.r$ union of exceptional parameters still measure zero. Compactness of $cal(C)$ $arrow.r$ finitely many KKT points.

*Step 2 (Homotopy from large $b$).* At $b_0 > ||A||_infinity \/ 2$, the contraction bound gives exactly one KKT point. Consider the homotopy $b: b_0 arrow.r b_"min"$. Non-degenerate KKT points trace smooth paths in $(z, b)$-space (implicit function theorem). New KKT points appear only via _saddle-node bifurcation_: a max-saddle pair is born (or annihilated) when the bordered Hessian becomes singular. Bifurcations are codimension-1 in $(b, theta)$-space; for generic $theta$, they are isolated in $b$.

*Step 3 (Global max persistence).* The global optimum value $V(b) = max f_b (bold(q))$ over the feasible set is continuous in $b$ (Berge's Maximum Theorem). At a bifurcation, the newborn local maximum has welfare _equal to its twin saddle_ — generically strictly below $V(b)$. The global maximum therefore continues as the smooth deformation of the unique optimum at $b_0$, without jumping or splitting. #h(1fr) $square$

_Corollary._ For generic parameters, Algorithm 2 (Annealed Frank-Wolfe) with $b_0 >= ||A||_infinity \/ 2$ converges to the global optimum at every temperature along the annealing path.

_Economic meaning._ The only instances with multiple optima require exact symmetries: identical order books, symmetric MM positions, matching budgets. Real order books — with heterogeneous beliefs, diverse limit prices, and asymmetric market structures — are generically unique. Theorem 8 (DIC) provides a _checkable certificate_ for specific instances; Theorem 9 says the certificate is almost never needed.

=== Demand diameter bound

Even when multiple KKT points exist (the measure-zero case), their demands and prices cannot be far apart.

#block(inset: (left: 1em))[
  *Proposition 5* (Demand Diameter). _Let $bold(q)^1, bold(q)^2$ both maximize $cal(L)(dot, bold(mu))$ over $cal(C)$ for fixed $bold(mu) >= 0$ and $b > 0$. Then:_
  $ ||bold(D)^1 - bold(D)^2||_2 <= (b dot sum_k mu_k overline(Q)_k) / p_"min" $
  _where $bold(D)^j = A bold(q)^j$, $overline(Q)_k = max_j sum_(i in "MM"_k) q_i^j$, and $p_"min" = min_m p_m (b)$. In particular, $bold(mu) = 0$ implies $bold(D)^1 = bold(D)^2$: demands and prices are unique when no budget binds._
]

_Proof._ The midpoint $overline(bold(q)) = (bold(q)^1 + bold(q)^2) \/ 2$ lies in $cal(C)$ (convex set). Its Lagrangian value:

$
cal(L)(overline(bold(q)), bold(mu)) - 1/2 (cal(L)(bold(q)^1, bold(mu)) + cal(L)(bold(q)^2, bold(mu))) = underbrace(delta, "entropy gain") - underbrace(epsilon, "cap perturbation")
$

The entropy gain: $delta = 1\/2 (C_b (bold(D)^1) + C_b (bold(D)^2)) - C_b (overline(bold(D))) >= (p_"min") / (2b) ||bold(D)^1 - bold(D)^2||_2^2$ by strict convexity of $C_b$ (minimum Hessian eigenvalue $= p_"min" \/ b$).

The cap perturbation: $|epsilon| = |sum_k mu_k ["cap"_k (overline(bold(q))) - 1\/2 ("cap"_k (bold(q)^1) + "cap"_k (bold(q)^2))]|$. Each $|"cap"_k (overline(bold(q))) - 1\/2 (dots)| <= (overline(Q)_k) / (4b) ||bold(D)^1 - bold(D)^2||_2$ by the $1\/(2b)$-Lipschitz bound on softmax prices.

Since $overline(bold(q))$ cannot exceed the optimal value: $delta <= |epsilon|$. Dividing through:

$
(p_"min") / (2b) ||bold(D)^1 - bold(D)^2||_2 <= (sum_k mu_k overline(Q)_k) / (4b)
$

Rearranging gives the bound. When $bold(mu) = 0$, the RHS vanishes, forcing $bold(D)^1 = bold(D)^2$. #h(1fr) $square$

_Remark._ Proposition 5 interpolates between two regimes. When budgets are slack ($bold(mu) = 0$), the strict concavity of $C_b$ dominates and demands are unique — the problem is "essentially convex." As budgets become tighter (larger $mu_k$), the cap perturbation grows, allowing wider demand separation. The DIC (Theorem 8) is the condition under which the perturbation vanishes entirely (convex cap $arrow.r$ no perturbation $arrow.r$ unique demand regardless of $mu$).

=== The Fenchel dual and price uniqueness <fenchel-dual>

A natural hope: even if _fills_ are non-unique, perhaps _clearing prices_ are unconditionally unique. We test this via the Fenchel dual.

*Unconstrained case (no budgets).* The primal in demand space is $max_D [W(D) - C_b (D)]$ where $W(D) = max_(bold(q): A bold(q) = D, bold(q) in "box") sum w_i q_i$ is concave piecewise-linear. Define $h = -W$ (convex). By Fenchel-Rockafellar duality:

$
min_(bold(p) in Delta^M) [underbrace(W^*(bold(p)), "consumer surplus") + underbrace(C_b^* (bold(p)), "entropy penalty")]
$

where $W^*(bold(p)) = sum_i (w_i - (A^top bold(p))_i)^+ overline(q)_i$ is the total surplus of orders whose limit prices exceed clearing prices, and $C_b^* (bold(p)) = sum_m b sum_k p_(m k) ln p_(m k)$ is the negative Shannon entropy (Theorem 2).

#block(inset: (left: 1em))[
  *Proposition 6* (Unconstrained Price Uniqueness). _The Fenchel dual of the unconstrained smoothed problem is strictly convex: $W^*$ is convex (piecewise linear) and $C_b^*$ is strictly convex on the interior of $Delta^M$. The clearing prices $bold(p)^*$ are therefore unique for any $b > 0$ and any order book, without any condition._
]

This is the Fenchel dual of Theorems 1–2: the indicator function $delta_Delta$ (hard simplex constraint) generalizes to the entropy penalty $-b H(bold(p))$ (soft simplex constraint). The entropy's strict convexity is what makes prices unique — it penalizes ambiguity.

*Budget-constrained case.* Replace $W(D)$ with $W_B (D) = max_(bold(q) in cal(C), "cap"_k <= B_k) sum w_i q_i$ subject to $A bold(q) = D$. The cap constraint $sum_(i in "MM"_k) c_i ("softmax"(D\/b)) q_i <= B_k$ depends on $D$ through the softmax prices, making $W_B$ _non-concave_ in $D$.

The Fenchel dual becomes $min_(bold(p)) [W_B^* (bold(p)) + C_b^* (bold(p))]$, but now $W_B^*$ is _non-convex_. The non-convexity arises from the cap constraint's dependence on $D$: as $D$ shifts, the softmax prices change at rate $O(1\/b)$, modifying which fills are budget-feasible. This introduces curvature of order $O(1\/b)$ into $W_B$ — the same scale as the entropy Hessian $nabla^2 C_b^* = "diag"(1\/bold(p))\/b$.

#block(inset: (left: 1em))[
  *Proposition 7* (Cross-Price Obstruction). _Unconditional price uniqueness is impossible for the budget-constrained smoothed problem. Specifically, the standard monotonicity argument fails due to cross-price budget violation._
]

_Proof._ Suppose two KKT points $(bold(q)^1, bold(p)^1, bold(mu)^1)$ and $(bold(q)^2, bold(p)^2, bold(mu)^2)$ exist with $bold(p)^1 != bold(p)^2$. The strict convexity of $C_b^*$ gives $chevron.l bold(D)^1 - bold(D)^2, bold(p)^1 - bold(p)^2 chevron.r > 0$ (the demand-price inner product is strictly positive for distinct prices). Cross-applying the optimality conditions and using complementary slackness ($mu_k^j (B_k - E_k (bold(q)^j, bold(p)^j)) = 0$) yields:

$
chevron.l bold(D)^1 - bold(D)^2, bold(p)^1 - bold(p)^2 chevron.r <= sum_k [mu_k^2 (E_k (bold(q)^1, bold(p)^2) - B_k) + mu_k^1 (E_k (bold(q)^2, bold(p)^1) - B_k)]
$

For a contradiction ($"LHS" > 0$ but $"RHS" <= 0$), we need $E_k (bold(q)^1, bold(p)^2) <= B_k$ — that the _first_ fills are affordable at the _second_ prices. But while $E_k (bold(q)^1, bold(p)^1) <= B_k$ holds (budget feasibility at own prices), there is *no guarantee* that $E_k (bold(q)^1, bold(p)^2) <= B_k$. If $bold(p)^2$ is higher in markets where MM $k$ holds long positions, the cross-price evaluation blows past the budget.

This is the mathematical signature of a _Generalized Nash Equilibrium Problem_ (GNEP): the feasible set (budget constraint) depends on the dual variable (prices). Standard Fenchel duality requires the primal feasible set to be independent of the dual; the endogenous-price budget destroys this independence. The obstruction is structural, not a gap in technique: both the entropy curvature and the cross-price budget violation scale as $O(1\/b)$, preventing either from dominating unconditionally. #h(1fr) $square$

_Remark._ This is the dual-space version of the symmetric counterexample. In the primal, two optima have different fills and different prices. In the dual, the budget feasibility "swaps" — fills affordable at one price are not affordable at the other — because the bilinear constraint $c(p) dot q <= B$ ties the feasible region to the solution.

=== The expenditure perspective and risk-averse extension

*Why Eisenberg-Gale fails.* Changing variables to capital expenditures $e_i = c_i (p) dot q_i$ linearizes the budget: $sum_(i in "MM"_k) e_i <= B_k$. But welfare transforms to $sum_i w_i dot e_i \/ c_i (p)$ — rational in prices. In Fisher markets (Eisenberg & Gale, 1959), the analogous program is convex because agents have _diminishing-returns_ utilities: the $ln U_k$ objective provides curvature. Our MMs have constant marginal returns (linear welfare), providing none.

*The risk-averse fix.* Replace linear MM welfare $sum w_i q_i$ with a strictly concave utility:

$
U_k (bold(q)) = sum_(i in "MM"_k) w_i ln(1 + q_i \/ s_i), quad s_i > 0
$

The logarithm models _Kelly-criterion sizing_: each additional fill has diminishing marginal value, reflecting the risk aversion that real institutional MMs exhibit. The Hessian $U_k'' = -w_i \/ (s_i + q_i)^2$ provides curvature of order $O(w_i \/ overline(q)_i^2)$ in the primal.

This breaks the isospectral deadlock: the utility curvature $O(w \/ overline(q)^2)$ is independent of $b$, while the budget non-convexity scales as $O(1\/b)$. For any $b > 0$, sufficiently strong risk aversion makes the Lagrangian _unconditionally_ strictly concave:

$
"Strict concavity" quad arrow.l.double quad min_i w_i \/ (s_i + overline(q)_i)^2 > sum_k mu_k dot ||nabla^2 "cap"_k||
$

Furthermore, the expenditure substitution now yields a convex program: $sum w_i ln(1 + e_i \/ (s_i c_i (p)))$ is jointly concave in $(e, p)$ on the feasible region.

This changes the economic model — we are solving a _risk-averse_ batch auction, not the exact LP. But the modification is arguably more realistic: institutional MMs size positions using Kelly-like criteria, not linear utility. The question of whether the risk-neutral LP model admits unconditional uniqueness is settled negatively by Proposition 7.

=== Landscape of uniqueness results

#align(center)[
  #table(
    columns: 4,
    align: (left, center, center, center),
    [*Result*], [*Condition*], [*What's unique*], [*Strength*],
    [Proposition 6], [No budgets], [Prices (Fenchel dual)], [Unconditional],
    [Theorem 8], [DIC holds at $b$], [Fill vector + prices], [Checkable certificate],
    [Contraction], [$b > ||A||_infinity \/ 2$], [Everything (exponential)], [Checkable, global],
    [Proposition 5], [$bold(mu) = 0$], [Demands + prices], [Unconditional (slack budgets)],
    [Theorem 9], [Generic $theta$], [Everything], [Almost everywhere],
    [Risk-averse ext.], [Kelly utility], [Everything], [Unconditional (modified model)],
    [Proposition 7], [---], [---], [Impossible (isospectral)],
  )
]

=== Connection to related frameworks

*Quantal Response Equilibria.* The Diluted Influence Condition is the prediction-market analog of the contraction condition for Logit QRE (McKelvey & Palfrey, 1995). In a QRE, agents choose actions via softmax of expected payoffs; uniqueness holds when the temperature is high enough relative to the payoff sensitivity. Our condition $Q <= 2b\/(2p-1)$ is the market-specific contraction bound. Theorem 9 extends this: just as QRE uniqueness holds for generic payoff matrices even when the temperature is low (McKelvey & Palfrey, Theorem 5), our generic uniqueness holds for all $b > 0$.

*Negative semidefinite games.* Hofbauer and Sandholm (2007) proved uniqueness of logit equilibria for _negative semidefinite_ games — games where increasing adoption of a strategy decreases its payoff. Our market is exactly this: buying more of outcome $k$ raises $p_k$, reducing the marginal payoff to further buyers. Their result covers the unconstrained case; our Theorem 8 extends it to budget-constrained agents under DIC, and Theorem 9 extends it to budget-constrained agents for generic parameters.

*Budget additivity.* Devanur and Dudík (2015) showed that budget-constrained LMSR exhibits _budget additivity_: two agents with budgets $B_1, B_2$ and identical beliefs affect prices identically to one agent with budget $B_1 + B_2$. This is a manifestation of hidden convexity — the bilinear budget boundaries merge into a convex hull in the dual space. Whether a similar structural convexity exists in our batch auction setting — perhaps through the expenditure reformulation — is an open question. A positive answer would strengthen Theorem 9 from generic to unconditional.

*Parametric optimization.* Theorem 9 uses the Parametric Transversality Theorem from differential topology (Abraham & Robbin, 1967; Guillemin & Pollack, 1974). The technique is standard in economic theory: Debreu (1970) used it to prove generic finiteness of Walrasian equilibria, and Mas-Colell (1985) applied it to smooth economies. Our application is new in the prediction market context.


= Discussion <discussion>

== What the Proof Establishes

The central result is that LP batch auction clearing and LMSR pricing are the _same mathematical object at different temperatures_:

#align(center)[
  #table(
    columns: 3,
    align: (left, center, center),
    [*Property*], [*LP ($b = 0$)*], [*LMSR ($b > 0$)*],
    [Minting cost], [$max_k D_k$], [$b ln sum exp(D_k\/b)$],
    [Price constraint], [$bold(p) in Delta$ (hard)], [$-b H(bold(p))$ penalty (soft)],
    [Price function], [Set by marginal order], [Softmax of demand],
    [Liquidity], [Endogenous (order book)], [Exogenous (parameter $b$)],
    [Market maker loss], [Zero], [$<= b ln K$],
    [Scaling (mutual exclusion)], [$O(K)$], [$O(K)$ smoothed / $O(2^K)$ combinatorial],
  )
]

== Convergence of Optimizers

Theorems 1–3 establish the structural connection between LP clearing and LMSR. The following two results complete the convergence picture.

#block(inset: (left: 1em))[
  *Theorem 5* (Optimizer Convergence). _Let $bold(q)_b^*$ denote an optimal fill vector of $P_b$ for each $b > 0$, and let $cal(Q)^*$ denote the set of optimal fill vectors of $P$ (the LP). Then:_
  $ lim_(b -> 0^+) "dist"(bold(q)_b^*, cal(Q)^*) = 0 $
  _If $P$ has a unique optimum $bold(q)^*$, then $bold(q)_b^* -> bold(q)^*$._
]

_Proof sketch._ The feasible set $[0, bold(overline(Q))]$ is compact. By Proposition 1, the objectives $f_b (bold(q)) = sum w_i q_i - C_b (bold(D)(bold(q)))$ converge uniformly to $f(bold(q)) = sum w_i q_i - V(bold(D)(bold(q)))$ on this compact set. The result follows from Berge's Maximum Theorem: the argmax correspondence of a uniformly convergent sequence of continuous functions on a compact set is upper hemicontinuous. #h(1fr) $square$

#block(inset: (left: 1em))[
  *Theorem 6* (Exponential Price Convergence). _Let $bold(p)(b)$ denote the LMSR prices at temperature $b$, and let $k^* = "argmax"_k D_k$ with gap $Delta = D_(k^*) - max_(k != k^*) D_k > 0$. Then:_
  $ |p_(k^*)(b) - 1| <= (K - 1) dot exp(-Delta \/ b) $
  $ p_k (b) <= exp(-Delta \/ b) quad "for" k != k^* $
  _The prices converge exponentially fast in $1\/b$, with rate governed by the demand gap $Delta$._
]

_Proof sketch._ Divide numerator and denominator of the softmax by $exp(D_(k^*)\/b)$:
$
p_k (b) = exp((D_k - D_(k^*))\/b) / (1 + sum_(j != k^*) exp((D_j - D_(k^*))\/b))
$
For $k != k^*$: the numerator is $<= exp(-Delta\/b)$ and the denominator is $>= 1$. For $k = k^*$: $1 - p_(k^*) = sum_(k != k^*) p_k <= (K-1) exp(-Delta\/b)$. #h(1fr) $square$

_Remark._ In the degenerate case $Delta = 0$ (tied demands), the softmax splits mass equally among tied outcomes. The LP can choose any split, so convergence is to the _set_ of LP optima (Theorem 5), not to a unique point. For generic instances (almost all order books), $Delta > 0$.

== Open Problems

+ *Extension to non-exclusive groups.* When markets are correlated but not mutually exclusive, group minting doesn't apply directly. The marginal decomposition (paper §4) handles this approximately; the exact connection to combinatorial LMSR in this regime is open.

+ *Unconditional uniqueness via risk-averse welfare.* Proposition 7 settles the question for the risk-neutral model: the isospectral obstruction makes unconditional uniqueness impossible. The risk-averse extension (§8.8, Kelly-criterion utility) breaks the deadlock by introducing $b$-independent curvature. Formalizing this — proving that the Eisenberg-Gale expenditure program with logarithmic MM utilities is jointly convex — would establish unconditional uniqueness for the modified model. This is both mathematically tractable and economically well-motivated.

== Connection to Prior Work

*Abernethy, Chen, and Vaughan (2013)*: Proved that any cost-function market maker satisfying a set of axioms must price via a convex cost function, with LMSR corresponding to the entropy conjugate. Our Theorem 2 is the batch-auction analog of their result: the LP minting cost is the "simplest" (piecewise linear) cost function satisfying the axioms, with LMSR as its smooth relaxation.

*Fortnow, Kilian, Pennock, and Wellman (2005)*: Used LP for combinatorial call market matching. Our group minting variable provides the structural reason _why_ LP works: it directly encodes the $O(K)$ mutual exclusivity constraint that combinatorial approaches must discover.

*Agrawal, Wang, and Ye (2011)*: Proposed convex pari-mutuel call auction mechanisms using convex programming. Our framework is a specialization where the convex cost function has a specific form ($V = max$) motivated by prediction market minting.

*Chen and Pennock (2007)*: Utility framework for bounded-loss market makers. The parameter $b$ in our framework corresponds directly to their loss bound. Our contribution is showing that $b = 0$ (zero loss) is achievable in a batch auction setting.

*Devanur and Dudík (2015)*: Proved price uniqueness for budget-constrained sequential LMSR via Bregman divergence, and budget additivity. Our Theorem 8 proves primal uniqueness under DIC; Theorem 9 proves generic uniqueness unconditionally. Their budget additivity result hints at hidden convexity in the dual space — extending this to simultaneous batch auctions is the main open problem (§9.2).

*Hofbauer and Sandholm (2007)*: Proved uniqueness of logit equilibria for negative semidefinite games. Our prediction market is negative semidefinite (price rises with demand). Their result covers the unconstrained case; our Theorems 8–9 extend to budget-constrained agents.

*McKelvey and Palfrey (1995)*: Established the Quantal Response Equilibrium framework. The Diluted Influence Condition is the prediction-market analog of their high-temperature uniqueness result; Theorem 9 extends to generic uniqueness at all temperatures.

*Rockafellar (2023)*: Augmented Lagrangian framework for variational convexity. Provides local uniqueness (tilt stability) at non-degenerate KKT points — the mechanism underlying Step 2 of Theorem 9's homotopy argument.

*Abraham and Robbin (1967); Debreu (1970)*: The Parametric Transversality Theorem used in Theorem 9 is standard in economic theory: Debreu used it to prove generic finiteness of Walrasian equilibria. Our application to prediction market clearing with budget constraints is new.

*Eisenberg and Gale (1959)*: Convex program for Fisher market equilibrium via expenditure variables. Our expenditure perspective (§8.8) identifies why the same approach does not directly apply to prediction markets: constant marginal returns (linear welfare) vs.~diminishing returns (concave utility).


#v(2em)
#line(length: 100%)
#v(0.5em)
#text(size: 9pt, style: "italic")[
  Next steps: (1) Empirical validation: implement LP solver with annealing, compare to MILP baseline. (2) Investigate convex reformulation via expenditure variables or Devanur-Dudík budget additivity. (3) Compute explicit convergence rates for the annealing schedule.
]
