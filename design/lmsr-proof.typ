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

== What Remains to Prove

This sketch establishes the structural connection. For a full paper, we would need:

+ *Formal epi-convergence proof* that the optimizers (fill vectors) of $P_b$ converge to those of $P$ as $b -> 0$, not just the objective values. This follows from standard convex analysis (Attouch's theorem) but the details require care when the LP has multiple optimal solutions.

+ *Rate of convergence* for the prices. The sandwich bound (Proposition 1) gives an $O(b ln K)$ welfare gap, but the price convergence rate may be faster for non-degenerate instances.

+ *Extension to non-exclusive groups.* When markets are correlated but not mutually exclusive, group minting doesn't apply directly. The marginal decomposition (paper §4) handles this approximately; the exact connection to combinatorial LMSR in this regime is open.

+ *SLP convergence for MM budgets.* The bilinear market maker budget constraint ($p times q <= B$) breaks the LP structure. The current SLP approach (paper §5) works empirically but lacks formal convergence guarantees. The entropy framework suggests an alternative: smoothing the bilinear constraint with an entropic penalty, then annealing $b -> 0$.

== Connection to Prior Work

*Abernethy, Chen, and Vaughan (2013)*: Proved that any cost-function market maker satisfying a set of axioms must price via a convex cost function, with LMSR corresponding to the entropy conjugate. Our Theorem 2 is the batch-auction analog of their result: the LP minting cost is the "simplest" (piecewise linear) cost function satisfying the axioms, with LMSR as its smooth relaxation.

*Fortnow, Kilian, Pennock, and Wellman (2005)*: Used LP for combinatorial call market matching. Our group minting variable provides the structural reason _why_ LP works: it directly encodes the $O(K)$ mutual exclusivity constraint that combinatorial approaches must discover.

*Agrawal, Wang, and Ye (2011)*: Proposed convex pari-mutuel call auction mechanisms using convex programming. Our framework is a specialization where the convex cost function has a specific form ($V = max$) motivated by prediction market minting.

*Chen and Pennock (2007)*: Utility framework for bounded-loss market makers. The parameter $b$ in our framework corresponds directly to their loss bound. Our contribution is showing that $b = 0$ (zero loss) is achievable in a batch auction setting.


#v(2em)
#line(length: 100%)
#v(0.5em)
#text(size: 9pt, style: "italic")[
  Next steps: (1) Verify the full proof in Lean4, focusing on Theorems 1–2 and the convergence claim. (2) Compute explicit rates for price convergence. (3) Explore whether the entropic smoothing framework suggests a principled approach to the MM budget constraint (replacing SLP with entropic annealing).
]
