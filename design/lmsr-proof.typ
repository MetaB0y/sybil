#set document(title: "Prediction Markets Are Fisher Markets")
#set text(font: "New Computer Modern", size: 10pt)
#set page(margin: (x: 1.5in, y: 1.2in), numbering: "1")
#set par(justify: true, leading: 0.55em)
#set heading(numbering: "1.")
#show heading.where(level: 1): it => block(above: 1.5em, below: 0.8em)[#it]
#show heading.where(level: 2): it => block(above: 1.2em, below: 0.6em)[#it]

#align(center)[
  #text(size: 15pt, weight: "bold")[
    Prediction Markets Are Fisher Markets
  ]
  #v(0.5em)
  #text(size: 11pt)[Batch Auction Clearing via Eisenberg-Gale Duality]
  #v(0.3em)
  #text(size: 9pt, style: "italic")[Draft — February 2026]
]

#v(1em)

#block(inset: (x: 2em))[
  #text(weight: "bold")[Summary.]
  We prove that prediction market batch auctions with budget-constrained market makers are Fisher markets (Theorem 8). Replacing linear market-maker welfare with logarithmic utility — reflecting the Kelly-criterion sizing that real institutional MMs use — transforms the budget-constrained clearing problem into a convex program with a unique solution, solvable in polynomial time. Budget constraints vanish from the formulation entirely, absorbed into the Eisenberg-Gale objective. Prices, fills, and budget allocation emerge simultaneously from a single convex optimization.
]

#v(1em)

= Introduction

The main result of this paper is that prediction market batch auctions are Fisher markets. Here is what that means and why it matters.

A prediction market exchange runs _batch auctions_: orders accumulate, then clear simultaneously at uniform prices. The clearing problem is an allocation problem — find the fills and prices that maximize welfare subject to balance constraints. Without budget constraints, this is a Linear Program: clean, fast, well-understood.

The difficulty comes from market makers. An ordinary participant submits a single order: "buy 100 shares of Yes at \$0.60." The capital required (\$60) is known at submission and locked upfront — no budget constraint needed. A market maker, by contrast, posts _hundreds_ of orders across _dozens_ of markets simultaneously. The total capital consumed depends on which orders fill and at what prices — both determined by the auction. The MM deposits a finite balance $B_k$ and the exchange must ensure total spending does not exceed it. This budget constraint is _bilinear_: capital consumed depends on the clearing price, and the clearing price depends on which fills happen. This single constraint makes the feasible set non-convex.

We prove three things, in order:

+ *The framework (§2).* LP batch clearing and Hanson's LMSR are the same mathematical object at different temperatures, connected by Fenchel duality. This is the foundation for everything that follows. It is also where we prove that clearing prices are unique when budgets are absent — a fact that makes the budget obstacle precise.

+ *The obstacle (§3).* Adding budget constraints to the risk-neutral model creates a Generalized Nash Equilibrium Problem. Unconditional uniqueness of clearing prices becomes provably impossible. The mathematical pathology has a structural cause: linear welfare with hard budget caps is an internally inconsistent economic model.

+ *The resolution (§§4–5).* Replacing linear MM welfare with Kelly-criterion utility ($B_k ln U_k$) transforms the problem into a strictly convex Eisenberg-Gale program. Budget constraints are absorbed into the objective — they do not appear as constraints at all. The program has a unique optimum, unique clearing prices, and is polynomial-time solvable. This is the Fisher market isomorphism.

The modeling "trick" is not logarithmic utility — it is the linear assumption. When we fix it, the pathology fixes itself.


= Foundations: Batch Clearing and LMSR <foundation>

This section builds the mathematical framework that the main result operates within. Readers familiar with cost-function market makers (Abernethy et al. 2013) will recognize these results in a new framing. We establish:

+ The _minting cost_ $V = max_k D_k$ that defines the batch clearing LP (§2.1).
+ The _Fenchel duality_ that forces clearing prices to be probabilities — not by fiat, but as a conjugate of the minting cost (§2.2).
+ The _LMSR smoothing_ $C_b = b ln sum exp(D_k\/b)$ and its entropy dual (§2.3).
+ How the _KKT conditions_ produce clearing prices: the exponentials come from minting, not the order book (§2.4).
+ That _without budgets, prices are always unique_ (Theorem 7) — making the budget obstacle of §3 precise (§2.8).

== Minting Cost and the LP

Consider a prediction market with $K$ mutually exclusive outcomes. In a batch auction, $N$ orders arrive. Each order $i$ has a limit price $L_i$, a maximum quantity $overline(Q)_i$, a side (buy or sell), and a target market $m(i)$. Some orders belong to market makers; we write $"MM"_k$ for the set of orders belonging to MM $k$.

The _net demand_ — excess buy over sell volume — for each outcome is:

$
D_k = sum_(i in "buy"(k)) q_i - sum_(j in "sell"(k)) q_j
$

where $q_i in [0, overline(Q)_i]$ is the fill quantity of order $i$.

To clear the market, the exchange _mints_ complete sets: one share of every outcome at cost \$1 (exactly one resolves to \$1 at settlement, so this is fairly priced). To supply $D_k$ net shares of each outcome, exactly $max_k D_k$ mints are needed — this covers the highest-demand outcome, with surplus shares of other outcomes left over. The minting cost is therefore $V(bold(D)) = max_k D_k$, and the welfare-maximizing clearing solves:

$ P: quad max_(bold(q) in [0, bold(overline(Q))]) quad sum_i w_i q_i - max_k D_k (bold(q)) $

where $w_i$ is the welfare coefficient of order $i$ ($+L_i$ for buyers, $-L_i$ for sellers) and $bold(overline(Q)) = (overline(Q)_1, dots, overline(Q)_N)$ is the vector of max fill quantities. Since $V$ is convex and $bold(D)$ is linear in $bold(q)$, the objective is concave — this is a convex optimization problem. Introducing an explicit minting variable $M >= D_k$ for each $k$, this is equivalent to the Linear Program:

$
max_(bold(q), M) quad sum_i w_i q_i - M quad "s.t." quad D_k (bold(q)) <= M quad forall k, quad bold(q) in [0, bold(overline(Q))]
$

Its dual variables are clearing prices.

== Fenchel Duality: Prices from Conjugates

The constraint that clearing prices must be probabilities ($sum p_k = 1$, $p_k >= 0$) is not imposed by fiat — it is the Fenchel conjugate of the minting cost. Minting at \$1 per complete set _encodes_ the probability axiom.

#block(inset: (left: 1em))[
  *Theorem 1* (Minting–Simplex Duality). _The Fenchel conjugate of $V(bold(D)) = max_k D_k$ is the indicator function of the probability simplex:_
  $ V^* (bold(p)) = delta_Delta (bold(p)) = cases(0 & "if" bold(p) in Delta, +infinity & "otherwise") $
  _where $Delta = {bold(p) >= 0 : sum_k p_k = 1}$._
]

_Proof._ We compute $V^*(bold(p)) = sup_(bold(D)) {sum p_k D_k - max_k D_k}$ in four cases.

*Case 1:* $bold(p) in Delta$. For any $bold(D)$, $sum_k p_k D_k <= max_k D_k$ (a convex combination cannot exceed the maximum). So the supremum is $<= 0$. Setting $bold(D) = bold(0)$ gives value $0$. Therefore $V^*(bold(p)) = 0$.

*Case 2:* $sum_k p_k > 1$. Take $bold(D) = (t, t, dots, t)$ for $t -> +infinity$. Then $sum p_k t - t = t(sum p_k - 1) -> +infinity$.

*Case 3:* $sum_k p_k < 1$ (with all $p_k >= 0$). Take $bold(D) = (-t, -t, dots, -t)$ for $t -> +infinity$. Then $sum p_k (-t) - (-t) = t(1 - sum p_k) -> +infinity$.

*Case 4:* Some $p_j < 0$. Take $D_j -> -infinity$ with all other $D_k = 0$. Then $p_j D_j -> +infinity$ while $max_k D_k = 0$, so the supremum diverges.

In Cases 2–4, $V^*(bold(p)) = +infinity$. #h(1fr) $square$

*Interpretation.* The conjugate takes only two values: $0$ (prices in $Delta$) and $+infinity$ (prices outside). There is no middle ground because the supremum over $bold(D)$ exploits any deviation from the simplex as an _arbitrage_: if $sum p_k > 1$, mint complete sets at cost \$1 and sell shares at total price $sum p_k > dollar 1$ — scale up for unbounded profit. The probability axiom $sum p_k = 1$ is not a modeling choice; it is a no-arbitrage condition enforced by the minting mechanism.

== Entropy Smoothing: LMSR as Soft LP

Hanson's LMSR cost function $C_b(bold(D)) = b ln sum exp(D_k\/b)$ is a smooth ($C^infinity$) version of the minting cost, parametrized by a temperature $b > 0$. The structural content is in its Fenchel conjugate: where $V^* = delta_Delta$ was a hard constraint (prices must be probabilities), $C_b^*$ is a soft penalty (prices near uniform are cheap, prices near a vertex are expensive).

#block(inset: (left: 1em))[
  *Theorem 2* (LMSR–Entropy Duality). _The Fenchel conjugate of $C_b$ is negative Shannon entropy on the simplex:_
  $ C_b^*(bold(p)) = cases(
    b sum_k p_k ln p_k quad & "if" bold(p) in Delta,
    +infinity & "otherwise"
  ) $
]

_Proof._ We compute $C_b^*(bold(p)) = sup_(bold(D)) {sum p_k D_k - b ln sum exp(D_k \/ b)}$ by setting the gradient to zero:
$
p_k - exp(D_k \/ b) / (sum_j exp(D_j \/ b)) = 0 quad arrow.r.double quad p_k = exp(D_k\/b) / (sum_j exp(D_j\/b))
$

This is the _softmax_ — exactly the LMSR marginal price. Inverting: $D_k = b ln p_k + b ln Z$ where $Z = sum exp(D_j\/b)$. Substituting back:
$
chevron.l bold(p), bold(D) chevron.r &= b sum_k p_k ln p_k + b ln Z, quad quad C_b (bold(D)) = b ln Z
$

So $C_b^*(bold(p)) = b sum_k p_k ln p_k$. This is finite only when $bold(p) in Delta$ (the softmax always yields a probability vector). #h(1fr) $square$

The approximation quality is controlled by $b$:

#block(inset: (left: 1em))[
  *Proposition 1* (LSE–Max Sandwich). $max_k D_k <= C_b(bold(D)) <= max_k D_k + b ln K$. _The gap $b ln K$ is the maximum LMSR subsidy._
]

_Proof._ $exp(max_k D_k\/b) <= sum exp(D_k\/b) <= K exp(max_k D_k\/b)$. Apply $b ln$. #h(1fr) $square$

The complete picture:

#align(center)[
  #table(
    columns: 3,
    align: center,
    stroke: none,
    [$V = max_k D_k$], [$stretch(arrow.l.r, size: #200%)^("Fenchel")$], [$V^* = delta_Delta$],
    [$arrow.t space b -> 0$], [], [$arrow.t space b -> 0$],
    [$C_b = b ln sum exp(D_k\/b)$], [$stretch(arrow.l.r, size: #200%)^("Fenchel")$], [$C_b^* = b sum p_k ln p_k$],
  )
]

As $b -> 0$, the smooth cost $C_b$ sharpens to the LP cost $V$, and the entropy penalty hardens to the simplex indicator.

== The Smoothed Batch Auction and Its KKT Conditions

Replace the minting cost $V$ with $C_b$ in the batch clearing:

$ P_b: quad max_(bold(q) in [0, bold(overline(Q))]) quad sum_i w_i q_i - C_b(bold(D)(bold(q))) $

Since $C_b$ is convex and smooth, and $bold(D)(bold(q))$ is linear, $P_b$ is a smooth concave maximization. Its first-order conditions are necessary and sufficient. The punchline: the exponentials come from the minting cost, not the order book.

#block(inset: (left: 1em))[
  *Theorem 3* (LMSR = Smoothed Batch Clearing). _At the optimum of $P_b$, the clearing prices are the softmax of net demand:_
  $ p_k^* = (partial C_b) / (partial D_k) = exp(D_k^* \/ b) / (sum_j exp(D_j^* \/ b)) $
  _This is the LMSR marginal price function. By construction, $sum_k p_k^* = 1$._
]

The two limits are immediate: as $b -> 0$, $P_b -> P$ (quantified by Theorems 5–6 below); as $b -> infinity$, $exp(D_k\/b) -> 1$ for all $k$, so $p_k -> 1\/K$.

To see exactly how these prices emerge, we write out the KKT system. For a buy order $i$ on outcome $k$ with welfare coefficient $w_i = L_i$:

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

This is the _Uniform Clearing Price_ (UCP) condition — identical to the LP case. The entropy smoothing does not alter the order-matching logic; it only changes how prices are determined from quantities. In the LP ($b = 0$), the clearing price is set by the marginal order — a discrete mechanism. In $P_b$ ($b > 0$), the price $p_k = exp(D_k\/b) \/ sum exp(D_j\/b)$ depends continuously on net demand. The order book determines _which_ orders fill (via UCP); the softmax determines _at what prices_ (via the minting cost gradient).

== Self-Financing Minting

#block(inset: (left: 1em))[
  *Theorem 4* (Self-Financing Minting). _In the LP ($b = 0$), the minting mechanism breaks even exactly: $"P&L" = sum p_k D_k - max_k D_k = 0$._
]

_Proof._ By complementary slackness, $p_k > 0$ only where $D_k = max_j D_j$. So $sum p_k D_k = (max_j D_j) dot sum_(D_k = max) p_k = max_j D_j$, since the active prices sum to 1 (Theorem 1). #h(1fr) $square$

For $b > 0$, the smoothed cost $C_b > V$ (Proposition 1), so $P_b$ collects less revenue than the LP. The gap $C_b - V <= b ln K$ is the LMSR subsidy — the cost of smooth pricing. In the LP limit, this subsidy vanishes.

== Convergence: LMSR Sharpens to LP

The convergence from LMSR to LP is quantified by two results:

#block(inset: (left: 1em))[
  *Theorem 5* (Optimizer Convergence). _Let $bold(q)_b^*$ denote an optimal fill vector of $P_b$ for each $b > 0$, and let $cal(Q)^*$ denote the set of optimal fill vectors of $P$ (the LP). Then:_
  $ lim_(b -> 0^+) "dist"(bold(q)_b^*, cal(Q)^*) = 0 $
  _If $P$ has a unique optimum $bold(q)^*$, then $bold(q)_b^* -> bold(q)^*$._
]

_Proof._ The feasible set $[0, bold(overline(Q))]$ is compact. By Proposition 1, the objectives $f_b (bold(q)) = sum w_i q_i - C_b (bold(D)(bold(q)))$ converge uniformly to $f(bold(q)) = sum w_i q_i - V(bold(D)(bold(q)))$ on this compact set. The result follows from Berge's Maximum Theorem: the argmax correspondence of a uniformly convergent sequence of continuous functions on a compact set is upper hemicontinuous. #h(1fr) $square$

#block(inset: (left: 1em))[
  *Theorem 6* (Exponential Price Convergence). _Let $bold(p)(b)$ denote the LMSR prices at temperature $b$, and let $k^* = "argmax"_k D_k$ with gap $Delta = D_(k^*) - max_(k != k^*) D_k > 0$. Then:_
  $ |p_(k^*)(b) - 1| <= (K - 1) dot exp(-Delta \/ b) $
  $ p_k (b) <= exp(-Delta \/ b) quad "for" k != k^* $
  _The prices converge exponentially fast in $1\/b$, with rate governed by the demand gap $Delta$._
]

_Proof._ Divide numerator and denominator of the softmax by $exp(D_(k^*)\/b)$:
$
p_k (b) = exp((D_k - D_(k^*))\/b) / (1 + sum_(j != k^*) exp((D_j - D_(k^*))\/b))
$
For $k != k^*$: the numerator is $<= exp(-Delta\/b)$ and the denominator is $>= 1$. For $k = k^*$: $1 - p_(k^*) = sum_(k != k^*) p_k <= (K-1) exp(-Delta\/b)$. #h(1fr) $square$

_Remark._ In the degenerate case $Delta = 0$ (tied demands), the softmax splits mass equally among tied outcomes. The LP can choose any split, so convergence is to the _set_ of LP optima (Theorem 5), not to a unique point. For generic instances (almost all order books), $Delta > 0$.

== Scaling

For $K$ mutually exclusive outcomes, the LP uses $O(K)$ balance constraints and one group mint variable. Combinatorial LMSR requires $O(2^K)$ state evaluations. The structural insight: mutual exclusivity collapses the joint state space from $2^K$ to $K$ states. Group minting exploits this directly.

== Price Uniqueness Without Budgets

Without budget constraints, clearing prices are _always_ unique — for any $b > 0$, any order book, unconditionally. This is proved by passing to the Fenchel dual, where the entropy term provides strict convexity. The result makes the budget obstacle (§3) precise: it is budgets and nothing else that can create non-uniqueness.

Write $W(bold(D))$ for the maximum welfare achievable at a given demand vector $bold(D)$ (the inner LP over fills $bold(q)$ with $bold(D)$ fixed). The primal problem in demand space is $max_(bold(D)) [W(bold(D)) - C_b (bold(D))]$, where $W$ is concave piecewise-linear. By Fenchel-Rockafellar duality, this is equivalent to minimizing over prices:

$
min_(bold(p) in Delta) [underbrace(W^*(bold(p)), "consumer surplus") + underbrace(C_b^* (bold(p)), "entropy penalty")]
$

where $W^*(bold(p))$ is the total surplus of orders whose limit prices exceed clearing prices, and $C_b^* (bold(p)) = b sum p_k ln p_k$ is the negative Shannon entropy (Theorem 2).

#block(inset: (left: 1em))[
  *Theorem 7* (Unconstrained Price Uniqueness). _The Fenchel dual of the unconstrained smoothed problem is strictly convex: $W^*$ is convex (piecewise linear) and $C_b^*$ is strictly convex on the interior of $Delta$. The clearing prices $bold(p)^*$ are therefore unique for any $b > 0$ and any order book, unconditionally._
]

_Proof._ $W^*$ is convex (conjugate of concave $W$) and $C_b^*$ is strictly convex on the simplex interior: its Hessian is $b dot "diag"(1\/p_k)$, positive definite for $bold(p) > 0$. The minimizer lies in the interior because $nabla C_b^* = b(1 + ln p_k) -> -infinity$ as any $p_k -> 0^+$, which dominates the finite gradient of $W^*$ and pulls the minimizer away from the boundary. Strict convexity on the interior gives uniqueness. #h(1fr) $square$

The result delineates the problem precisely: everything that goes wrong in §3 is caused by the budget constraint alone.


= The Budget Obstacle <obstacle>

_What this section proves:_ when market makers have budget constraints, the clearing problem becomes fundamentally harder. The budget constraint is bilinear (capital depends on price, price depends on fills), making the feasible set non-convex. We prove that unconditional uniqueness of clearing prices is impossible for the risk-neutral model. The diagnosis: linear welfare with hard budget caps is an economically inconsistent model, and the non-convexity is the mathematical symptom.

== The Bilinear Constraint

A market maker $k$ deposits balance $B_k$ and posts orders across multiple markets. The capital consumed by each fill depends on the clearing price:

$
"cap"_k(bold(p), bold(q)) = sum_(i in "MM"_k) c_i(p_(m(i))) dot q_i, quad c_i(p) = cases(p & "if BuyYes/SellNo", 1-p & "if SellYes/BuyNo")
$

The budget constraint $"cap"_k <= B_k$ is _bilinear_: $p$ is determined by $bold(q)$ through the clearing mechanism. The product $c(p(bold(q))) dot q$ makes the feasible set non-convex. This single constraint is what separates prediction market clearing from a standard LP.

== The Cross-Price Obstruction

One might hope that despite the non-convexity, clearing prices are still unique (even if fills are not). After all, prices were unconditionally unique without budgets (Theorem 7). We prove this hope is false.

#block(inset: (left: 1em))[
  *Proposition 2* (Cross-Price Obstruction). _For the risk-neutral budget-constrained problem, unconditional price uniqueness is impossible. The standard monotonicity argument fails due to cross-price budget violation._
]

_Proof._ Suppose two KKT points $(bold(q)^1, bold(p)^1, bold(mu)^1)$ and $(bold(q)^2, bold(p)^2, bold(mu)^2)$ exist with $bold(p)^1 != bold(p)^2$. The strict convexity of $C_b^*$ (Theorem 2) gives $chevron.l bold(D)^1 - bold(D)^2, bold(p)^1 - bold(p)^2 chevron.r > 0$ — the demand-price inner product is strictly positive for distinct prices. For a contradiction ($"LHS" > 0$ but $"RHS" <= 0$), we need the first fills to be budget-feasible at the second prices: $"cap"_k(bold(p)^2, bold(q)^1) <= B_k$. But while $"cap"_k(bold(p)^1, bold(q)^1) <= B_k$ holds (budget feasibility at own prices), there is *no guarantee* that $"cap"_k(bold(p)^2, bold(q)^1) <= B_k$. If $bold(p)^2$ is higher where MM $k$ is long, the cross-price evaluation exceeds the budget.

This is the signature of a _Generalized Nash Equilibrium Problem_ (GNEP): the feasible set depends on the dual variable. The standard uniqueness proof cannot go through because it requires cross-price budget feasibility, which the bilinear constraint does not guarantee. #h(1fr) $square$

*Counterexample.* Two markets $A, B$ with identical order books and one MM with symmetric positions on both. By symmetry, there exist two KKT points: "fill $A$, starve $B$" and "fill $B$, starve $A$," producing different fills, demands, and prices. This requires exact parameter symmetry — a measure-zero set — but it proves that no unconditional uniqueness theorem exists for the risk-neutral model. Theorem 8 resolves this by changing the model, not the proof technique.

== The Expenditure Perspective: Why the Obvious Fix Fails

A natural idea: change variables to capital expenditures $e_i = c_i(p) dot q_i$, which linearizes the budget constraint to $sum_(i in "MM"_k) e_i <= B_k$. In Fisher markets, the Eisenberg-Gale program works precisely because this change of variables produces a convex program. Does it work here?

No. Welfare transforms to $sum_i w_i dot e_i \/ c_i(p)$ — a _rational_ function of prices. In Fisher markets (Eisenberg & Gale 1959), the analogous program is convex because agents have _diminishing-returns_ utilities: $sum B_k ln U_k$ provides curvature. Our MMs have constant marginal returns (linear welfare), providing none. The expenditure substitution linearizes the budget but introduces non-convexity into the objective.

This is the structural diagnosis. The problem is not that we haven't found the right proof technique. The problem is that _linear welfare with hard budget caps_ is an internally inconsistent economic model — it models agents as simultaneously risk-neutral (constant marginal value) and risk-averse (capped exposure). The mathematical pathology is a symptom of the modeling error.

§4 argues that correcting the economic model (from linear to logarithmic utility) resolves the inconsistency, and §5 proves that the correction resolves the non-convexity.


= Why Market Makers Have Diminishing Returns <economic-case>

_What this section argues:_ the standard assumption that market makers have linear welfare (constant marginal value per share) is the unrealistic approximation. Logarithmic (Kelly-criterion) utility is the correct model for repeat-participation MMs. Five independent arguments converge on this conclusion.

== Kelly Criterion Is a Survival Theorem

Breiman (1961) proved that among all repeated-game investment strategies, Kelly maximization (log utility) is the _unique_ strategy that: (1) maximizes long-run growth rate almost surely; (2) reaches any wealth target in minimum expected time; (3) dominates any other strategy in the long run with probability 1.

An MM that sizes linearly — betting a fixed dollar amount per opportunity regardless of bankroll — faces ruin with probability 1 given enough batches. More precisely, Breiman showed that the Kelly strategy asymptotically dominates: the ratio of Kelly wealth to any other strategy's wealth grows without bound almost surely. Sub-Kelly strategies (betting less than Kelly) survive but grow strictly slower; over-Kelly strategies (betting more) face ruin.

When we model MMs as having log utility, we are not imposing an assumption. We are recognizing a _selection effect_: the MMs that survive long enough to matter are the ones already using Kelly-like sizing.

== Order Books Reveal Concave Demand

Real MMs do not post one order at one limit price for their entire budget. They post _ladders_: multiple orders at decreasing limit prices with decreasing quantities. Each additional tranche has lower welfare and smaller size — this IS diminishing returns, expressed through the order book.

The linear model treats each rung of the ladder as an independent agent with constant marginal value. The log model captures the _single agent_ expressing a concave demand curve. The linear model is the modeling fiction.

== Budget Constraints _Are_ Risk Aversion

Why do MMs have budget constraints at all? A risk-neutral agent with positive expected value should bet everything. The existence of $B_k < "total wealth"$ is itself evidence of risk aversion: the MM limits exposure because it values capital preservation.

Linear welfare with a hard budget is internally inconsistent: "I value each share equally (risk-neutral) but I won't risk more than $B_k$ (risk-averse)." The budget is a crude piecewise-linear approximation of what log utility handles smoothly:

$
"Linear + budget:" quad u(q) = cases(w dot q & "if cap" <= B, -infinity & "if cap" > B) quad quad quad "Log:" quad u(q) = B ln(w dot q)
$

The first is a discontinuous hack. The second is the smooth version of the same economic reality.

== Inventory Risk Makes Linear Utility Degenerate

An MM that buys 1,000 shares of Yes at \$0.50 has \$500 of inventory risk. If the market moves to \$0.30, they lose \$200. A linear-utility MM does not care — the 1,001st share has the same marginal value as the 1st. But this is absurd: the MM holds a concentrated position that could wipe out its budget.

In practice, every MM applies _position limits_ — hard caps on per-market exposure. These are another crude piecewise-linear approximation of diminishing returns. Log utility makes position limits unnecessary: the natural concavity of $ln$ penalizes concentration intrinsically.

== Repeated Batches Demand Kelly

A prediction market exchange runs _repeated_ batch auctions. An MM participates batch after batch, compounding returns. The natural objective for a repeat participant in multiplicative games is:

$
max EE[ln("wealth"_T)] = max sum_t EE[ln(1 + r_t)]
$

This is exactly the Kelly criterion, which is exactly log utility _per batch_. Linear utility per batch maximizes expected wealth after one batch but minimizes survival probability across many batches. Since FBAs are inherently repeated, log utility per batch is the only consistent objective for repeat participants.

== The Synthesis

#align(center)[
  #table(
    columns: 3,
    align: (left, left, left),
    [*Argument*], [*Why linear fails*], [*Why log is right*],
    [Kelly/Breiman], [Bankrupt w.p.~1], [Unique growth-optimal],
    [Order books], [Treats ladder rungs as independent], [Captures concave demand],
    [Internal consistency], [Budget + risk-neutral contradicts], [Smooth risk aversion],
    [Inventory risk], [Requires bolted-on position limits], [Concentration penalty intrinsic],
    [Repeated batches], [Maximizes one-shot EV only], [Unique consistent FBA objective],
  )
]

*The punch line:* linear welfare with budget constraints is a piecewise-linear caricature of risk-averse behavior. Log utility is the smooth, theoretically grounded version of the same economic reality. The non-convexity of §3 is not a feature of prediction markets — it is an artifact of the wrong model.


= The Main Result: Risk-Averse Clearing Is a Fisher Market <risk-averse>

_What this section proves:_ replacing linear MM welfare with Kelly-criterion utility transforms the budget-constrained clearing problem into a convex program with a unique solution. Budget constraints disappear from the formulation — they are absorbed into the Eisenberg-Gale objective. The resulting program is structurally isomorphic to a Fisher market with endogenous supply.

== The Program

#block(inset: (left: 1em))[
  *Theorem 8* (Risk-Averse Clearing Is Convex). _Define the risk-averse batch auction clearing program:_

  $ P_b^"RA": quad max_(bold(q) in cal(C)) quad underbrace(sum_k B_k ln U_k(bold(q)), "MM welfare") + underbrace(sum_(j in.not "MM") w_j q_j, "retail welfare") - underbrace(C_b(bold(D)(bold(q))), "minting cost") $

  _where $U_k(bold(q)) = sum_(i in "MM"_k) L_i q_i$ is MM $k$'s total weighted fill, $"MM"_k$ contains MM $k$'s buy orders (with $L_i > 0$; any sell orders from MMs contribute to the retail welfare term), $cal(C) = {bold(q) in [0, bold(overline(Q))]: "balance constraints"}$ is the LP feasible set, and $C_b$ is the smoothed minting cost (Proposition 1). Then:_

  + _The objective is strictly concave on the feasible set (whenever any MM has positive-welfare orders with $B_k > 0$)._
  + _$P_b^"RA"$ has a unique optimal fill vector $bold(q)^*$._
  + _No explicit budget constraints appear. At the optimum, each MM $k$ spends at most $B_k$, with equality when no fill is capacity-constrained._
  + _Clearing prices $bold(p)^*$ are unique._
  + _The program is solvable in polynomial time by any standard convex optimizer (interior point, projected gradient, etc.)._
]

== Proof

*(1) Strict concavity.* Since $"MM"_k$ contains only buy orders with $L_i > 0$, we have $U_k >= 0$ with $U_k > 0$ whenever any buy order fills (guaranteed by the $ln$ singularity at $U_k = 0$). Each $B_k ln U_k$ is the composition of $ln$ (strictly concave, increasing) with a positive linear function — strictly concave whenever $U_k$ is non-constant on $cal(C)$. The retail welfare $sum w_j q_j$ is linear. The minting cost $-C_b(bold(D)(bold(q)))$ is concave ($C_b$ is convex, $bold(D)$ is linear). The sum is strictly concave.

*(2) Uniqueness.* Strict concavity on a compact convex set ($cal(C)$ is a polytope) gives a unique maximizer.

*(3) Budget absorption.* This is the core mechanism — the Eisenberg-Gale trick. Since $"MM"_k$ contains only buy orders, every MM order $i$ has $w_i = L_i > 0$ and increases $D_(m(i))$ by $q_i$. The KKT condition for MM order $i$ of agent $k$ is:

$
(B_k L_i) / U_k - p_(m(i)) = lambda_i^+ - lambda_i^-
$

where $lambda_i^+, lambda_i^- >= 0$ are box constraint multipliers ($lambda_i^+ (q_i - overline(Q)_i) = 0$, $lambda_i^- q_i = 0$) and $p_(m(i)) = partial C_b \/ partial D_(m(i))$ is the clearing price — which is also the per-share capital cost for a buy order. Multiply by $q_i$ and sum over $i in "MM"_k$. By complementary slackness, $lambda_i^- q_i = 0$, so these terms vanish:

$
B_k / U_k dot underbrace(sum_(i in "MM"_k) L_i q_i, = U_k) = sum_(i in "MM"_k) p_(m(i)) q_i + underbrace(sum_(i in "MM"_k) lambda_i^+ q_i, >= 0)
$

The left side telescopes to $B_k$. So:

$ sum_(i in "MM"_k) p_(m(i))^* q_i^* = B_k - sum_(i in "MM"_k) lambda_i^+ q_i^* <= B_k $

Each MM's capital deployed on purchases is at most $B_k$, with equality when no fill hits its upper bound. The $ln$ singularity at $U_k = 0$ drives the optimizer to exhaust all available capacity. The budget constraint is not imposed — it emerges from the $B_k ln U_k$ objective. This is the Eisenberg-Gale mechanism.

*(4) Price uniqueness.* Prices are $p_k = partial C_b \/ partial D_k = "softmax"(bold(D)\/b)$, a continuous function of the unique $bold(q)^*$.

*(5) Polynomial solvability.* The objective is concave and $C^infinity$ (for $b > 0$, all components are smooth). The feasible set is a polytope. Standard interior-point methods solve this in polynomial time. #h(1fr) $square$

== Temperature Independence

In the risk-neutral model, the entropy smoothing ($C_b$) was the _only_ source of concavity, and Proposition 2 shows this is insufficient to overcome the budget non-convexity. In the risk-averse model, $B_k ln U_k$ provides $b$-independent curvature of order $O(B_k\/U_k^2)$. The $-C_b$ term adds more concavity — it helps, not hurts.

Even at $b = 0$ (pure LP minting cost), the program remains strictly concave:

$ P_0^"RA": quad max_(bold(q) in cal(C)) quad sum_k B_k ln U_k + sum_(j in.not "MM") w_j q_j - max_k D_k $

The $-max_k D_k$ term is concave (not strictly), but the $ln$ term provides strict concavity. Uniqueness holds at every temperature, including $b = 0$.

== The Fisher Market Isomorphism

Program $P_b^"RA"$ is structurally isomorphic to the Eisenberg-Gale convex program for Fisher market equilibrium:

#align(center)[
  #table(
    columns: 3,
    align: (left, center, center),
    stroke: none,
    [*Component*], [*Fisher market*], [*Risk-averse batch auction*],
    [Buyers], [Consumers with budgets $B_k$], [MMs with budgets $B_k$],
    [Goods], [Divisible commodities], [Outcome shares],
    [Supply], [Fixed endowment $s_j$], [Minting (endogenous, cost $C_b$)],
    [Utility], [$U_k = sum w_i x_(k i)$], [$U_k = sum w_i q_i$],
    [Objective], [$sum B_k ln U_k$], [$sum B_k ln U_k + "retail" - C_b$],
    [Prices], [Dual of supply constraint], [Dual of balance constraint = softmax],
  )
]

The prediction market extends Fisher markets in two ways: (1) supply is endogenous via minting (shares are created at cost $C_b$, not drawn from a fixed endowment), and (2) non-MM ("retail") orders contribute linear welfare alongside the log-utility MMs. Both extensions preserve concavity.

== What Changes and What Doesn't

*What changes.* Under $P_b^"RA"$, MMs with larger budgets $B_k$ have proportionally more influence on prices (through the $B_k$ weight), but with diminishing marginal impact on any single market. An MM concentrating capital on one market faces the $ln$ penalty: the first dollar of fill generates infinite marginal utility, the last dollar generates vanishing utility. This naturally diversifies MM capital across markets — the $ln$ enforces the flash-liquidity pattern (spread capital thin) without protocol-level constraints.

*What doesn't change.* Non-MM orders are still matched by UCP at clearing prices. The minting mechanism is unchanged. Prices are still softmax (for $b > 0$) or LP duals (for $b = 0$). Volume is unaffected — the $ln$ is still increasing, so every profitable fill opportunity is taken. The only difference is in how MM fills are _sized_ relative to each other: proportional to welfare-per-dollar rather than absolute welfare.

*The buy/sell decomposition.* The Fisher market isomorphism applies to MM buy orders — the orders that deploy capital and create demand. MM sell orders (if any) are treated as retail: filled by UCP with linear welfare, consuming no budget. This decomposition is natural: in a Fisher market, agents are consumers (buyers of goods). The MM's budget constrains purchasing; selling provides supply and frees capital. Sell-side risk management (collateral for short positions) is a separate concern not captured by the Eisenberg-Gale structure.


= Discussion <discussion>

== Summary

The paper establishes one main result and the framework necessary to state and prove it:

#align(center)[
  #table(
    columns: 3,
    align: (left, center, center),
    [*Section*], [*Result*], [*Role*],
    [§2], [LMSR = smoothed LP (Thms 1–7)], [Framework],
    [§3], [Impossibility (Prop.~2)], [Motivation],
    [§4], [Log utility is correct model], [Economic argument],
    [§5], [Fisher market isomorphism (Thm 8)], [*Main result*],
  )
]

The landscape: unconditional uniqueness is impossible for risk-neutral MMs (Proposition 2) and unconditional uniqueness holds for risk-averse MMs (Theorem 8). The transition from impossibility to tractability requires only a change of objective function — from $sum w_i q_i$ to $sum B_k ln U_k$ — reflecting the economic reality that repeat-participation market makers have diminishing returns.

== Open Problems

+ *Risk-averse clearing in practice.* How much welfare does $P_b^"RA"$ sacrifice relative to the risk-neutral LP? When $B_k >> U_k$ (large budgets relative to fills), $ln U_k approx U_k\/U_k^0$ is nearly linear and the gap should vanish. Formalizing this convergence would quantify when risk-averse clearing is a practical drop-in replacement.

+ *Extension to non-exclusive groups.* When markets are correlated but not mutually exclusive, group minting doesn't apply directly. The exact connection to combinatorial LMSR in this regime is open.

+ *Risk-neutral hidden convexity.* Is there a reformulation of the risk-neutral model that recovers convexity? Proposition 2 shows it cannot come from the standard Fenchel dual. Devanur and Dudík (2015) proved budget additivity for sequential LMSR, hinting at hidden convexity — but extending this to simultaneous batch auctions remains open.

== Connection to Prior Work

*Eisenberg and Gale (1959)*: Convex program for Fisher market equilibrium via expenditure variables. Our Theorem 8 establishes that prediction markets with risk-averse MMs are isomorphic to Fisher markets: the Eisenberg-Gale program with endogenous supply (minting cost $C_b$) is convex. §3 identifies why the same approach fails for risk-neutral MMs: constant marginal returns provide no curvature to counteract the budget non-convexity.

*Abernethy, Chen, and Vaughan (2013)*: Proved that cost-function market makers satisfying a set of axioms must price via a convex cost function, with LMSR corresponding to the entropy conjugate. Our §2 is the batch-auction formulation of their framework. The minting cost $V = max_k D_k$ is the "simplest" cost function, with LMSR as its entropy smoothing.

*Breiman (1961)*: Optimal properties of the Kelly criterion. Our §4 uses Breiman's survival theorem to argue that log utility is not a convenience but a consequence of evolutionary selection among repeat-participation MMs.

*Devanur and Dudík (2015)*: Price uniqueness and budget additivity for sequential LMSR via Bregman divergence. Their budget additivity is a manifestation of hidden convexity — the bilinear budget boundaries merge into a convex hull in the dual space. Our Theorem 8 achieves unconditional uniqueness for batch auctions via the same Eisenberg-Gale structure underlying their result. Whether the _risk-neutral_ batch auction also has hidden convexity remains open (§3 shows it cannot come from the standard Fenchel dual).

*Fortnow, Kilian, Pennock, and Wellman (2005)*: LP for combinatorial call markets. Our group minting provides the structural reason why LP works: it encodes mutual exclusivity in $O(K)$ vs $O(2^K)$.

*Chen and Pennock (2007)*: Bounded-loss market makers. The parameter $b$ in our framework is their loss bound. $b = 0$ (zero loss) is achievable in the batch setting (Theorem 4).

*Hofbauer and Sandholm (2007)*: Unique logit equilibrium for negative semidefinite games — games where increasing adoption of a strategy decreases its payoff. Our prediction market is exactly NSD (demand raises price). Their result covers the unconstrained case; our Fisher market isomorphism extends to budget-constrained agents unconditionally.

*McKelvey and Palfrey (1995)*: Quantal Response Equilibrium. The budget-constrained clearing problem is the prediction-market analog of QRE with budget constraints. Our impossibility result (Proposition 2) corresponds to the failure of high-temperature contraction when budgets bind; our resolution via log utility corresponds to adopting a different utility model entirely.

*Budish, Cramton, and Shim (2015)*: Frequent Batch Auctions for equity markets. Our framework applies FBAs to prediction markets, where the information-driven price shocks are even more severe than in equities.


#v(2em)
#line(length: 100%)
#v(0.5em)
#text(size: 9pt, style: "italic")[
  Next steps: (1) Implement risk-averse clearing (Theorem 8) — single convex program, no annealing. (2) Empirical welfare comparison between $P_b^"RA"$ and risk-neutral LP across realistic order books. (3) Quantify $P_b^"RA" -> P_b$ convergence as $B_k -> infinity$.
]
