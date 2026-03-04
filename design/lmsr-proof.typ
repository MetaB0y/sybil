#set document(title: "Prediction Markets Are Fisher Markets")
#set text(font: "New Computer Modern", size: 10pt)
#set page(margin: (x: 1.5in, y: 1.2in), numbering: "1")
#set par(justify: true, leading: 0.55em)
#set heading(numbering: "1.")
#show heading.where(level: 1): it => block(above: 1.5em, below: 0.8em)[#it]
#show heading.where(level: 2): it => block(above: 1.2em, below: 0.6em)[#it]

// Theorem-like environments with auto-numbering and @label cross-referencing
#show figure.where(kind: "theorem"): it => align(left, it.body)
#show figure.where(kind: "proposition"): it => align(left, it.body)

#let theorem(name: none, body) = figure(
  kind: "theorem", supplement: [Theorem], numbering: "1", outlined: false,
  block(width: 100%, inset: (left: 1em))[
    *Theorem #context counter(figure.where(kind: "theorem")).display("1")*#if name != none [ (#name)]. #body
  ]
)

#let proposition(name: none, body) = figure(
  kind: "proposition", supplement: [Proposition], numbering: "1", outlined: false,
  block(width: 100%, inset: (left: 1em))[
    *Proposition #context counter(figure.where(kind: "proposition")).display("1")*#if name != none [ (#name)]. #body
  ]
)

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
  We prove that prediction market batch auctions with budget-constrained market makers are Fisher markets (@thm-main). Replacing linear market-maker welfare with logarithmic utility — reflecting the Kelly-criterion sizing that real institutional MMs use — transforms the budget-constrained clearing problem into a convex program with a unique solution, solvable in polynomial time. Budget constraints vanish from the formulation entirely, absorbed into the Eisenberg-Gale objective. Prices, fills, and budget allocation emerge simultaneously from a single convex optimization.
]

#v(1em)

= Introduction

The main result of this paper is that prediction market batch auctions are Fisher markets.

A prediction market exchange runs _batch auctions_: orders accumulate, then clear simultaneously at uniform prices. The clearing problem is an allocation problem — find the fills and prices that maximize welfare subject to balance constraints. Without budget constraints, this is a standard Linear Program.

Market makers introduce the main difficulty. An ordinary participant submits a single order: "buy 100 shares of Yes at \$0.60." The capital required (\$60) is known at submission and locked upfront — no budget constraint needed. A market maker, by contrast, posts _hundreds_ of orders across _dozens_ of markets simultaneously. The total capital consumed depends on which orders fill and at what prices — both determined by the auction. The MM deposits a finite balance $B_k$ and the exchange must ensure total spending does not exceed it. This budget constraint is _bilinear_: capital consumed depends on the clearing price, and the clearing price depends on which fills happen. This single constraint makes the feasible set non-convex.

We prove three things, in order:

+ *The framework (§2).* LP batch clearing and Hanson's LMSR are the same mathematical object at different temperatures, connected by Fenchel duality. This is the foundation for everything that follows. It is also where we prove that clearing prices are unique when budgets are absent — a fact that makes the budget obstacle precise.

+ *The obstacle (§3).* Adding budget constraints to the risk-neutral model makes the feasible set non-convex (bilinear constraints, indefinite Hessian). Standard convex optimization no longer applies, no polynomial-time algorithm is known, and uniqueness of clearing prices is an open question. The structural cause: linear welfare with hard budget caps is an internally inconsistent economic model.

+ *The resolution (§§4–5).* Replacing linear MM welfare with Kelly-criterion utility ($B_k ln U_k$) transforms the problem into a strictly convex Eisenberg-Gale program. Budget constraints are absorbed into the objective — they do not appear as constraints at all. The program has a unique optimum, unique clearing prices, and is polynomial-time solvable. This is the Fisher market isomorphism.

The modeling "trick" is not logarithmic utility — it is the linear assumption. When we fix it, the pathology fixes itself. §6 shows the result is practical: the program admits a conic formulation with $K$ exponential cones and decomposes across independent market groups via smooth budget allocation.


= Foundations: Batch Clearing and LMSR <foundation>

This section builds the mathematical framework for the main result. Readers familiar with cost-function market makers (Abernethy et al. 2013) will recognize these results in a new framing.

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

#theorem(name: "Minting–Simplex Duality")[
  _The Fenchel conjugate of $V(bold(D)) = max_k D_k$ is the indicator function of the probability simplex:_
  $ V^* (bold(p)) = delta_Delta (bold(p)) = cases(0 & "if" bold(p) in Delta, +infinity & "otherwise") $
  _where $Delta = {bold(p) >= 0 : sum_k p_k = 1}$._
] <thm-minting>

_Proof._ For $bold(p) in Delta$: convexity gives $sum p_k D_k <= max_k D_k$, so the supremum is $0$ (attained at $bold(D) = bold(0)$). For $bold(p) in.not Delta$: some deviation from the simplex exists, and scaling $bold(D)$ in the exploiting direction sends the supremum to $+infinity$. #h(1fr) $square$

The probability axiom $sum p_k = 1$ is not a modeling choice — it is a no-arbitrage condition. Any deviation from the simplex is an arbitrage opportunity (e.g., if $sum p_k > 1$, mint at \$1 and sell at $sum p_k$), and the conjugate enforces this as a hard constraint.

== Entropy Smoothing: LMSR as Soft LP

Hanson's LMSR cost function $C_b(bold(D)) = b ln sum exp(D_k\/b)$ is a smooth ($C^infinity$) version of the minting cost, parametrized by a temperature $b > 0$. The structural content is in its Fenchel conjugate: where $V^* = delta_Delta$ was a hard constraint (prices must be probabilities), $C_b^*$ is a soft penalty (prices near uniform are cheap, prices near a vertex are expensive).

#theorem(name: "LMSR–Entropy Duality")[
  _The Fenchel conjugate of $C_b$ is negative Shannon entropy on the simplex:_
  $ C_b^*(bold(p)) = cases(
    b sum_k p_k ln p_k quad & "if" bold(p) in Delta,
    +infinity & "otherwise"
  ) $
] <thm-lmsr>

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

#proposition(name: "LSE–Max Sandwich")[
  $max_k D_k <= C_b(bold(D)) <= max_k D_k + b ln K$. _The gap $b ln K$ is the maximum LMSR subsidy._
] <prop-sandwich>

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

Since $C_b$ is convex and smooth, and $bold(D)(bold(q))$ is linear, $P_b$ is a smooth concave maximization. Its first-order conditions are necessary and sufficient, and the exponentials in the clearing prices come from the minting cost, not the order book.

#theorem(name: "LMSR = Smoothed Batch Clearing")[
  _At the optimum of $P_b$, the clearing prices are the softmax of net demand:_
  $ p_k^* = (partial C_b) / (partial D_k) = exp(D_k^* \/ b) / (sum_j exp(D_j^* \/ b)) $
  _This is the LMSR marginal price function. By construction, $sum_k p_k^* = 1$._
] <thm-clearing>

The two limits are immediate: as $b -> 0$, $P_b -> P$ (by Berge's theorem on the compact feasible set, with uniform convergence from @prop-sandwich); as $b -> infinity$, $exp(D_k\/b) -> 1$ for all $k$, so $p_k -> 1\/K$.

The KKT conditions yield the standard _Uniform Clearing Price_ (UCP) rule: order $i$ fills if and only if its limit price exceeds the clearing price $p_k$. The entropy smoothing does not alter order-matching logic; it only changes how prices depend on quantities: continuously via softmax rather than discretely via the marginal order.

== Price Uniqueness Without Budgets

Without budget constraints, clearing prices are _always_ unique — for any $b > 0$, any order book, unconditionally. This is proved by passing to the Fenchel dual, where the entropy term provides strict convexity.
Write $W(bold(D))$ for the maximum welfare achievable at a given demand vector $bold(D)$ (the inner LP over fills $bold(q)$ with $bold(D)$ fixed). The primal problem in demand space is $max_(bold(D)) [W(bold(D)) - C_b (bold(D))]$, where $W$ is concave piecewise-linear. By Fenchel-Rockafellar duality, this is equivalent to minimizing over prices:

$
min_(bold(p) in Delta) [underbrace(W^*(bold(p)), "consumer surplus") + underbrace(C_b^* (bold(p)), "entropy penalty")]
$

where $W^*(bold(p))$ is the total surplus of orders whose limit prices exceed clearing prices, and $C_b^* (bold(p)) = b sum p_k ln p_k$ is the negative Shannon entropy (@thm-lmsr).

#theorem(name: "Unconstrained Price Uniqueness")[
  _The Fenchel dual of the unconstrained smoothed problem is strictly convex: $W^*$ is convex (piecewise linear) and $C_b^*$ is strictly convex on the interior of $Delta$. The clearing prices $bold(p)^*$ are therefore unique for any $b > 0$ and any order book, unconditionally._
] <thm-unique>

_Proof._ $W^*$ is convex (conjugate of concave $W$) and $C_b^*$ is strictly convex on the simplex interior: its Hessian is $b dot "diag"(1\/p_k)$, positive definite for $bold(p) > 0$. The minimizer lies in the interior because $nabla C_b^* = b(1 + ln p_k) -> -infinity$ as any $p_k -> 0^+$, which dominates the finite gradient of $W^*$ and pulls the minimizer away from the boundary. Strict convexity on the interior gives uniqueness. #h(1fr) $square$


= The Budget Obstacle <obstacle>

When market makers have budget constraints, the clearing problem becomes fundamentally harder. Without budgets, batch clearing is a standard convex program (@thm-unique). With budgets, the capital constraint is bilinear — capital consumed depends on clearing prices, which depend on fills — and standard convex optimization no longer applies.

== The Bilinear Constraint

A market maker $k$ deposits balance $B_k$ and posts orders across multiple markets. The capital consumed by each fill depends on the clearing price:

$
"cap"_k(bold(p), bold(q)) = sum_(i in "MM"_k) c_i(p_(m(i))) dot q_i, quad c_i(p) = cases(p & "if BuyYes/SellNo", 1-p & "if SellYes/BuyNo")
$

The budget constraint $"cap"_k <= B_k$ is _bilinear_: $p$ is determined by $bold(q)$ through the clearing mechanism. The product $c(p(bold(q))) dot q$ makes the feasible set non-convex in fill space. This single constraint is what separates prediction market clearing from a standard LP.

== Computational Consequences <computational>

The bilinear budget constraint has three consequences for computation:

*1. The feasible set is non-convex.* Define $h_k (bold(q)) = sum_(i in "MM"_k) p_(m(i))(bold(q)) dot q_i$ as MM $k$'s capital consumption. We show the Hessian is indefinite via the simplest non-trivial case: an MM with buy orders on two independent binary groups, one order per group, fill quantities $q_1, q_2$. Since groups are independent, $h = f(q_1) + f(q_2)$ where $f(q) = sigma(q\/b) dot q$ and $sigma$ is the logistic sigmoid. Differentiating:
$
f''(q) = (sigma(1-sigma)) / b [2 + q(1-2sigma) / b]
$
Since $sigma(1-sigma) > 0$, the sign depends on $2 + q(1-2sigma)\/b$. At $q = 0$: $f''(0) = 1\/(2b) > 0$ (convex). For large $q$: $sigma -> 1$ and the bracket becomes $2 - q\/b -> -infinity$ (concave). The inflection point $f''(q^*) = 0$ is at $q^* approx 2.4 b$. With fills straddling the threshold — $q_1 < q^*$ and $q_2 > q^*$ — the diagonal Hessian $"diag"(f''(q_1), f''(q_2))$ is indefinite. Sublevel sets ${h <= B_k}$ are not guaranteed convex.

*2. No standard convex algorithm applies.* The unconstrained problem $P_b$ is a smooth concave maximization (solved by interior point in polynomial time). Adding budget constraints ${h_k <= B_k}$ makes the feasible set non-convex. Standard convex optimization — interior point, projected gradient, Frank-Wolfe — requires convex feasible sets. The budget-constrained problem is a bilinear program, for which no polynomial-time algorithm is known in general.

*3. The uniqueness question is open.* The proof of @thm-unique (price uniqueness without budgets) uses strict convexity of the Fenchel dual. With budgets, the argument breaks: uniqueness at one price vector requires _cross-price budget feasibility_ ($"cap"_k (bold(p)^2, bold(q)^1) <= B_k$), which the bilinear constraint does not guarantee. Whether clearing prices are nonetheless unique for all LMSR instances is an open question. (In the degenerate case of two identical markets with a symmetric MM, two KKT points exist by symmetry — but this requires exact parameter tuning and does not extend to generic order books.)

#proposition(name: "Computational Obstruction")[
  _The budget-constrained risk-neutral clearing problem is a bilinear program: the feasible set ${bold(q) in cal(C) : h_k (bold(q)) <= B_k}$ is non-convex (indefinite constraint Hessian). Standard convex optimization does not apply. By contrast, the risk-averse program $P_b^"RA"$ (@thm-main) is a standard convex program solvable in polynomial time._
] <prop-obstruction>

In practice, exchanges handle budgets by iterative heuristics: solve the LP ignoring budgets, check for violations, adjust, repeat. Such methods have no convergence guarantee and can cycle. The risk-averse program eliminates this entirely — budgets are absorbed into the objective.

== The Expenditure Perspective

A natural idea: change variables to capital expenditures $e_i = c_i(p) dot q_i$, which linearizes the budget constraint to $sum_(i in "MM"_k) e_i <= B_k$. In Fisher markets, the Eisenberg-Gale program works precisely because this change of variables produces a convex program. Does it work here?

No. Welfare transforms to $sum_i w_i dot e_i \/ c_i(p)$ — a _rational_ function of prices. In Fisher markets (Eisenberg & Gale 1959), the analogous program is convex because agents have _diminishing-returns_ utilities: $sum B_k ln U_k$ provides curvature. Our MMs have constant marginal returns (linear welfare), providing none. The expenditure substitution linearizes the budget but introduces non-convexity into the objective.

The diagnosis is structural: _linear welfare with hard budget caps_ is an internally inconsistent economic model — agents simultaneously risk-neutral (constant marginal value) and risk-averse (capped exposure). The non-convexity is the computational symptom.

The mathematics tells us exactly what is needed: diminishing-returns utility to provide curvature. §4 argues that this is not a mathematical convenience but the independently correct economic model — the computational need and the economic reality coincide. §5 proves the resulting program is convex.


= Why Market Makers Have Diminishing Returns <economic-case>

The previous section showed that clearing requires diminishing-returns utility to be computationally tractable. This section argues that diminishing returns is also the independently correct economic model for repeat-participation MMs — the mathematical fix and the economic reality are the same object. We give three theoretical arguments and two empirical observations.

== Kelly Criterion Is a Survival Theorem

Breiman (1961) proved that Kelly maximization (log utility) is the _unique_ repeated-game strategy that maximizes long-run growth rate a.s., reaches any wealth target in minimum expected time, and dominates any other strategy with probability 1. An MM that sizes linearly faces ruin with probability 1 given enough batches.

A prediction market exchange runs _repeated_ batch auctions, compounding returns. The natural objective for a repeat participant is $max sum_t EE[ln(1 + r_t)]$ — exactly the Kelly criterion, exactly log utility per batch. Linear utility maximizes one-shot expected value but minimizes survival across batches. Since FBAs are inherently repeated, log utility is the only consistent objective.

Modeling MMs with log utility is not an arbitrary assumption but a consequence of competitive selection: the MMs that survive long enough to matter are the ones already using Kelly-like sizing.

== Budget Constraints _Are_ Risk Aversion

Why do MMs have budget constraints at all? A risk-neutral agent with positive expected value should bet everything. The existence of $B_k < "total wealth"$ is itself evidence of risk aversion: the MM limits exposure because it values capital preservation.

Linear welfare with a hard budget is internally inconsistent: "I value each share equally (risk-neutral) but I won't risk more than $B_k$ (risk-averse)." The budget is a crude piecewise-linear approximation of what log utility handles smoothly.

== Empirical Grounding

Real MMs post _ladders_ — multiple orders at decreasing limit prices with decreasing quantities — revealing concave demand directly through the order book. They also impose position limits to cap inventory risk: a linear-utility MM is indifferent between the 1st and 1,001st share, but real MMs are not. Both ladders and position limits are piecewise-linear approximations of diminishing returns. Log utility captures concave demand natively and penalizes concentration intrinsically.

== Synthesis

#align(center)[
  #table(
    columns: 3,
    align: (left, left, left),
    [*Argument*], [*Why linear fails*], [*Why log is right*],
    [Kelly/Breiman], [Bankrupt w.p.~1], [Unique growth-optimal],
    [Internal consistency], [Budget + risk-neutral contradicts], [Smooth risk aversion],
    [Order ladders], [Treats rungs as independent agents], [Captures concave demand],
    [Inventory risk], [Requires bolted-on position limits], [Concentration penalty intrinsic],
  )
]

The non-convexity of §3 is not a feature of prediction markets — it is an artifact of the linear welfare model.


= Risk-Averse Clearing Is a Fisher Market <risk-averse>

Replacing linear MM welfare with Kelly-criterion utility transforms the budget-constrained clearing problem into a convex program with a unique solution. Budget constraints disappear from the formulation — they are absorbed into the Eisenberg-Gale objective. The resulting program is structurally isomorphic to a quasi-linear Fisher market with endogenous supply.

== The Program

#theorem(name: "Risk-Averse Clearing Is Convex")[
  _Define the risk-averse batch auction clearing program with cash retention:_

  $ P_b^"RA": quad max_(bold(q) in cal(C), bold(s) >= 0) quad underbrace(sum_k [B_k ln(U_k(bold(q)) + s_k) - s_k], "MM welfare") + underbrace(sum_(j in.not "MM") w_j q_j, "retail welfare") - underbrace(C_b(bold(D)(bold(q))), "minting cost") $

  _where $"MM"_k^+$ is the set of MM $k$'s buy orders (sell orders from MMs enter the retail welfare term; see §5.5), $U_k(bold(q)) = sum_(i in "MM"_k^+) L_i q_i >= 0$ is MM $k$'s total weighted fill ($L_i > 0$ for all $i in "MM"_k^+$), $s_k >= 0$ is MM $k$'s retained cash, $cal(C) = {bold(q) in [0, bold(overline(Q))]: "balance constraints"}$ is the LP feasible set, and $C_b$ is the smoothed minting cost (@prop-sandwich). Define $mu_k = B_k \/ (U_k + s_k)$ as the shadow price of capital. Then:_

  + _The objective is strictly concave in the aggregates $(U_k, s_k, bold(D))$. The optimal aggregates, welfare, and clearing prices are unique. Fills $bold(q)^*$ are unique up to reallocation among MM buy orders with identical market and limit price._
  + _Limit orders are exact: $mu_k <= 1$, so no MM order fills at negative welfare ($L_i < p_k$)._
  + _No explicit budget constraints appear. At the optimum, each MM $k$ spends at most $B_k$: capital on fills plus retained cash $sum_(i in "MM"_k^+) p_(m(i)) q_i + s_k <= B_k$._
  + _Clearing prices $bold(p)^*$ are unique._
  + _The program is solvable in polynomial time by any standard convex optimizer._
  + _The program operates in two regimes per MM:_
    - _Capital-constrained ($U_k >= B_k$): $s_k = 0$, $mu_k = B_k\/U_k < 1$. The MM prioritizes highest-ROI fills, organically staying within budget._
    - _Over-capitalized ($U_k < B_k$): $s_k = B_k - U_k$, $mu_k = 1$. The MM clears identically to the risk-neutral LP, absorbing excess budget into cash._
] <thm-main>

== Proof

*(1) Strict concavity and uniqueness.* The objective is strictly concave in the aggregate variables: each $B_k ln(U_k + s_k)$ is strictly concave in $(U_k, s_k)$, $-C_b(bold(D))$ is strictly concave in $bold(D)$ (for $b > 0$; concave for $b = 0$), and the remaining terms are linear. Since $U_k$ and $bold(D)$ are linear functions of $bold(q)$, the composition is concave in $bold(q)$ and strictly concave in the aggregates $(U_k, s_k, bold(D))$. The objective tends to $-infinity$ as $s_k -> infinity$ (since $-s_k$ dominates $ln s_k$) and as $bold(q)$ approaches the boundary of $cal(C)$ from outside (box constraints). Therefore the superlevel sets ${(bold(q), bold(s)) : f^"RA" >= c}$ are compact for any $c > -infinity$, so the supremum is attained. The optimal aggregates $(U_k^*, s_k^*, bold(D)^*)$ — and hence prices, total welfare, and per-MM utility — are unique. Individual fills $bold(q)^*$ are unique whenever the map $bold(q) |-> (U_k, bold(D))$ is injective over the feasible set (i.e., no two MM buy orders from the same MM in the same market share the same limit price); otherwise, equivalent orders may be reallocated freely without affecting any observable. At the optimum, $U_k + s_k > 0$ for every MM with $B_k > 0$: the gradient $partial / (partial s_k)[B_k ln(U_k + s_k) - s_k] -> +infinity$ as $U_k + s_k -> 0^+$, so the boundary $U_k + s_k = 0$ is never optimal.

*(2) Limit order exactness.* The KKT condition for $s_k >= 0$ is:

$
partial / (partial s_k): quad mu_k - 1 <= 0, quad s_k >= 0, quad (mu_k - 1) s_k = 0
$

where $mu_k = B_k \/ (U_k + s_k)$. This gives $mu_k <= 1$ unconditionally. The KKT for fill $q_i$ of agent $k$ is $mu_k L_i - p_(m(i)) = lambda_i^+ - lambda_i^-$. A fill ($q_i > 0$) requires $mu_k L_i >= p_(m(i))$, hence $L_i >= p_(m(i)) \/ mu_k >= p_(m(i))$: the limit price must exceed the clearing price. No negative-welfare fill is possible.

The two regimes follow from complementary slackness: $s_k = 0$ when $mu_k < 1$ (i.e., $U_k > B_k$), and $s_k = B_k - U_k > 0$ when $mu_k = 1$ (i.e., $U_k < B_k$). In the over-capitalized regime ($mu_k = 1$), the fill condition $L_i >= p_(m(i))$ is exactly the risk-neutral UCP condition — the MM clears identically to the LP.

*(3) Budget absorption.* Multiply the fill KKT by $q_i$ and sum over $i in "MM"_k^+$. By complementary slackness ($lambda_i^- q_i = 0$):

$
mu_k underbrace(sum_(i in "MM"_k^+) L_i q_i, = U_k) = sum_(i in "MM"_k^+) p_(m(i)) q_i + underbrace(sum_(i in "MM"_k^+) lambda_i^+ q_i, >= 0)
$

So $sum p_(m(i)) q_i <= mu_k U_k$. Since $mu_k (U_k + s_k) = B_k$, we have $mu_k U_k = B_k - mu_k s_k <= B_k$:

$ sum_(i in "MM"_k^+) p_(m(i))^* q_i^* + s_k^* <= B_k $

Each MM's total deployment — capital on fills plus retained cash — is at most $B_k$. (The gap $sum lambda_i^+ q_i$ is infra-marginal surplus from fully-filled orders — profit that does not require additional capital.) The budget emerges from the objective, not from an explicit constraint. This is the Eisenberg-Gale mechanism extended to quasi-linear utilities: the $ln$ singularity absorbs the budget, and the cash variable absorbs the surplus.

*(4) Price uniqueness ($b > 0$).* Prices are $p_k = partial C_b \/ partial D_k = "softmax"(bold(D)\/b)$, a continuous function of the unique $bold(D)^*$. (At $b = 0$, aggregates are unique but prices are LP duals, which may not be unique when multiple $D_k$ tie at $max_j D_j$; see §5.3.)

*(5) Polynomial solvability.* For $b > 0$, the objective is concave and $C^infinity$. The feasible set is a polytope times $RR_+^(|cal(K)|)$. Standard interior-point methods solve this in polynomial time. At $b = 0$, the objective is concave but non-smooth; subgradient or bundle methods apply. #h(1fr) $square$

*Example.* Binary market ($K = 2$), $b -> 0$. An MM ($B = 50$) posts two buy-Yes orders: $A$ at $L_A = 0.70$, qty $100$; $B$ at $L_B = 0.55$, qty $100$. A retail trader posts buy-No at $L_C = 0.60$, qty $200$.

_LP (no budgets)._ All fill at balanced minting ($D_"Yes" = D_"No" = 200$). Welfare $= 45$. MM capital at any supporting price exceeds $B$: even at the lowest dual price $p_"Yes" = 0.40$, capital $= 0.40 times 200 = 80 > B$.

_Risk-averse._ The MM is capital-constrained ($U^"LP" = 125 > B = 50$, $s = 0$). Only order $A$ fills: $q_A = 100$, $q_B = 0$, $q_C = 100$. The retail order is partially filled, pinning $p_"No" = L_C = 0.60$, hence $p_"Yes" = 0.40$. Shadow price $mu = B\/U = 50\/70 approx 0.71$. Order $A$ fills: $mu L_A = 0.50 > p_"Yes" = 0.40$. Order $B$ does not fill: $mu L_B = 0.39 < 0.40$ — the shadow price throttles the lower-ROI order. Welfare $= 30$. Capital consumed $= 0.40 times 100 = 40 <= B$, with the gap of $10$ being infra-marginal surplus from order $A$.

_Welfare gap._ Actual gap $= 15$; exact bound $= Delta - B ln(1 + Delta\/B) approx 29$ (where $Delta = U^"LP" - B = 75$). The bound is conservative because the retail side contracts proportionally.

== Temperature Independence

In the risk-neutral model, entropy smoothing was the only source of concavity — insufficient against budget non-convexity (@prop-obstruction). Here, $B_k ln(U_k + s_k)$ provides $b$-independent curvature. Even at $b = 0$ (pure LP minting cost), the program remains strictly concave:

$ P_0^"RA": quad max_(bold(q) in cal(C), bold(s) >= 0) quad sum_k [B_k ln(U_k + s_k) - s_k] + sum_(j in.not "MM") w_j q_j - max_k D_k $

The $-max_k D_k$ term is concave (not strictly), but the $ln$ term provides strict concavity. Aggregate uniqueness, limit order exactness, and budget absorption hold at every temperature, including $b = 0$. Price uniqueness requires $b > 0$: at $b = 0$, prices are LP dual variables, which may not be unique when multiple outcomes $k$ tie at $D_k = max_j D_j$ (the subdifferential of $max$ is a face of the simplex). Any $b > 0$ restores uniqueness, with LMSR subsidy bounded by $b ln K$ (@prop-sandwich).

== Welfare Convergence

The cash retention variable makes the risk-averse program a strict generalization of the risk-neutral LP: when budgets are non-binding, the two programs agree exactly.

#proposition(name: "LP Recovery")[
  _Let $bold(q)^"LP"$ be the unconstrained LP optimum ($P_b$ without budgets). If $B_k >= U_k^"LP" = sum_(i in "MM"_k^+) L_i q_i^"LP"$ for every MM $k$, then the risk-averse optimum $(bold(q)^*, bold(s)^*)$ satisfies $bold(q)^* = bold(q)^"LP"$ and $s_k^* = B_k - U_k^"LP"$. Fills, prices, and welfare are identical._
] <prop-welfare>

_Proof._ When $B_k >= U_k^"LP"$ for all $k$, every MM is in the over-capitalized regime at $bold(q)^"LP"$: the optimizer sets $s_k = B_k - U_k^"LP" >= 0$ and $mu_k = 1$. The fill KKT reduces to $L_i - p_(m(i)) = lambda_i^+ - lambda_i^-$ — exactly the LP KKT. Since the LP and risk-averse programs have the same KKT conditions at $bold(q)^"LP"$, and the risk-averse program has a unique optimum, $bold(q)^* = bold(q)^"LP"$. #h(1fr) $square$

The cash variable $s_k$ acts as a _numeraire good_, eliminating the over-fill pathology of the naive $B_k ln U_k$ model (which forces all budget into fills, including unprofitable ones). The quasi-linear structure is essential: without $s_k$, over-capitalized MMs would distort fills to exhaust their budget.

#proposition(name: "Welfare Gap Bound")[
  _Let $W^"LP"$ and $W^"RA"$ denote the total welfare (LP objective value) at the LP and risk-averse optima respectively, and let $Delta_k = max(0, U_k^"LP" - B_k)$ be MM $k$'s budget shortfall. Then:_

  $ W^"LP" - W^"RA" <= sum_k [Delta_k - B_k ln(1 + Delta_k / B_k)] <= sum_k Delta_k^2 / (2 B_k) $

  _Over-capitalized MMs ($Delta_k = 0$) contribute nothing. The bound is quadratic in the shortfall._
] <prop-gap>

_Proof._ For any $bold(q)$ and cash choice $bold(s)$ with $V_k = U_k + s_k$, the LP and RA objectives differ by $f^"LP" (bold(q)) - f^"RA" (bold(q), bold(s)) = sum_k phi_k (V_k)$ where $phi_k (V) = V - B_k ln V$ (this follows from $U_k = V_k - s_k$ and cancellation of $s_k$). Decompose:

$
W^"LP" - W^"RA" = underbrace([f^"LP" (bold(q)^"LP") - f^"RA" (bold(q)^"LP", bold(s)^*)], = sum_k phi_k (V_k^"LP")) + underbrace([f^"RA" (bold(q)^"LP", bold(s)^*) - f^"RA" (bold(q)^"RA", bold(s)^"RA")], <= 0) + underbrace([f^"RA" (bold(q)^"RA", bold(s)^"RA") - f^"LP" (bold(q)^"RA")], = -sum_k phi_k (V_k^"RA"))
$

where $bold(s)^*$ is the optimal cash at $bold(q)^"LP"$ and the middle term is non-positive by RA optimality. The function $phi_k$ is convex with minimum $phi_k (B_k)$ at $V = B_k$. Since $(bold(q)^"RA", bold(s)^"RA")$ is the RA optimum, $mu_k = B_k \/ V_k^"RA" <= 1$ holds (part 2 of @thm-main), giving $V_k^"RA" >= B_k$ and hence $phi_k (V_k^"RA") >= phi_k (B_k)$:

$ W^"LP" - W^"RA" <= sum_k [phi_k (V_k^"LP") - phi_k (B_k)] $

At $bold(q)^"LP"$, the optimal cash is $s_k^* = max(0, B_k - U_k^"LP")$, so $V_k^"LP" = max(U_k^"LP", B_k)$. Over-capitalized MMs have $V_k^"LP" = B_k$ (zero contribution). Capital-constrained MMs contribute $phi_k(U_k^"LP") - phi_k(B_k) = Delta_k - B_k ln(1 + Delta_k\/B_k)$. The quadratic bound follows from $ln(1 + x) >= x - x^2\/2$. #h(1fr) $square$

The bound is tight when all orders are marginal ($L_i approx p_k$) and conservative for real order ladders, where spread-out limit prices give $V_k^"RA" >> B_k$. The exact expression grows sublinearly in $Delta_k$: the throttled fills are precisely the least profitable ones.

== The Fisher Market Isomorphism

In a Fisher market (Eisenberg & Gale 1959), $n$ consumers with budgets $B_k$ purchase divisible goods with fixed supply $s_j$ at prices $p_j$. The equilibrium is the unique solution to:

$ "EG": quad max_(bold(x) >= 0) quad sum_k B_k ln U_k (bold(x)_k) quad "s.t." quad sum_k x_(k j) <= s_j quad forall j $

where $U_k(bold(x)_k) = sum_j u_(k j) x_(k j)$ is consumer $k$'s linear utility over goods. The supply constraints have dual variables $p_j$ — the equilibrium prices. Budget constraints do not appear explicitly; they emerge from the $B_k ln U_k$ objective by the same telescoping argument as our proof of (3). Adding a cash variable $s_k$ with cost $-s_k$ yields the _quasi-linear_ Fisher market (Cole et al. 2017, Chen et al. 2009) — agents can retain unspent budget as cash rather than being forced to spend it all on goods. Quasi-linear Fisher markets are well-studied: equilibrium existence, uniqueness, and polynomial-time computation are known under mild conditions.

Program $P_b^"RA"$ is a quasi-linear Fisher market with two extensions:

#align(center)[
  #table(
    columns: 3,
    align: (left, center, center),
    stroke: none,
    [*Component*], [*Fisher market (EG)*], [*Batch auction ($P_b^"RA"$)*],
    [Consumers], [$n$ agents with budgets $B_k$], [MMs with budgets $B_k$],
    [Goods], [Divisible commodities], [Outcome shares],
    [Supply], [Fixed endowment $s_j$], [Endogenous: minting at cost $C_b$],
    [Utility], [$U_k = sum_j u_(k j) x_(k j)$], [$U_k = sum_(i in "MM"_k^+) L_i q_i$],
    [Cash], [Optional ($s_k >= 0$)], [Retained budget ($s_k >= 0$)],
    [Prices], [Dual of $sum_k x_(k j) <= s_j$], [Gradient of $C_b$ (softmax)],
  )
]

The prediction market extends the quasi-linear Fisher market in two ways: (1) supply is endogenous — shares are created by minting at cost $C_b(bold(D))$ rather than drawn from a fixed endowment, and (2) non-MM ("retail") orders contribute linear welfare alongside the log-utility MMs. Both extensions preserve concavity.

== What Changes and What Doesn't

*What changes.* Under $P_b^"RA"$, capital-constrained MMs ($mu_k < 1$) prioritize their highest-ROI fills rather than filling every profitable order equally. An MM concentrating capital on one market faces the $ln$ penalty: the first dollar of fill generates high marginal utility, additional dollars generate diminishing utility. This naturally diversifies MM capital across markets. Over-capitalized MMs ($mu_k = 1$) behave identically to the LP — the log model only affects sizing when the budget actually binds.

*What doesn't change.* Non-MM orders are still matched by UCP at clearing prices. The minting mechanism is unchanged. Prices are still softmax (for $b > 0$) or LP duals (for $b = 0$). Limit orders are exact: no order fills below its stated price.

*The buy/sell decomposition.* In each binary market, MM positions are netted per outcome before clearing: opposing buys and sells cancel, leaving a net directional position. Since "sell Yes at $L$" is economically equivalent to "buy No at $1-L$" (with capital cost $1-L$ per share), all net positions are expressed as buys. These net buy orders form $"MM"_k^+$ and their capital costs enter the budget accounting through $U_k$. The only MM sell orders that bypass the budget are liquidations of existing inventory — shares already held, not newly created short positions. Per outcome per batch, the MM is either net buying or net selling, never both.

*Extension to multiple groups and bundle orders.* Nothing in @thm-main requires a single mutually exclusive group. Consider $N$ groups, each with $K_j$ mutually exclusive outcomes. The joint state space is $cal(S) = product_j {1, dots, K_j}$ (exactly one joint state is realized). Bundle orders — orders whose payoffs depend on the joint state — create demand $D_s$ over $cal(S)$. The minting cost generalizes to $V(bold(D)) = max_s D_s$ (minting one complete set of all joint states costs \$1), and the smoothed version is $C_b = b ln sum_s exp(D_s\/b)$. Both are convex in $bold(D)$, and $bold(D)$ is linear in $bold(q)$. The proof of @thm-main goes through unchanged for any fixed state space representation: $B_k ln(U_k + s_k)$ is still strictly concave, $-C_b$ is still concave, budget absorption and limit order exactness still hold. The open question is whether the program can be solved without explicitly enumerating the joint states (see §7.2).

When no orders span multiple groups, the joint problem decomposes: $C_b = sum_j C_b^(G_j)$ (the product-LMSR factorization of Chen and Pennock 2007). Cross-group orders break this separability, coupling groups transitively — if orders link groups $A$–$B$ and $B$–$C$, the clearing problem requires the full $K_A times K_B times K_C$ state space. The mathematical obstruction is purely computational, not structural (see §7.2).


= Computation <computation>

== Conic Formulation

At $b = 0$, the minting cost $max_k D_k$ is modeled by LP epigraph constraints ($M >= D_k$). The EG program $P_0^"RA"$ then has a linear objective over LP constraints plus one logarithmic term per MM. Each constraint $t_k <= ln(V_k)$ is modeled by a single exponential cone: $(t_k, 1, V_k) in cal(K)_"exp" = {(x,y,z) : y exp(x\/y) <= z}$. The full program is a conic optimization problem solvable by standard interior-point solvers (Clarabel, MOSEK).

#proposition(name: "Conic Reformulation")[
  _At $b = 0$, $P_0^"RA"$ is equivalent to a conic program: minimize a linear objective over LP constraints and $n_"MM"$ exponential cone constraints (one per MM). Clearing prices are the dual variables of the minting epigraph constraints. For $b > 0$, the log-sum-exp minting cost adds $O(K)$ additional exponential cones (one per outcome); the augmented-LP approach below avoids this overhead._
] <prop-conic>

_Remark (Augmented LP)._ The EG Hessian $H = -sum_k (B_k \/ V_k^2) bold(ell)_k bold(ell)_k^T$ has rank at most $n_"MM"$ (the number of MMs), where $bold(ell)_k$ is MM $k$'s welfare weight vector. By Sherman–Morrison–Woodbury, each Newton step costs the same as an LP interior-point step plus an $O(n dot n_"MM")$ rank-$n_"MM"$ correction — dominated by the $O(n)$ LP cost when $n_"MM" << n$. The EG problem has the same asymptotic complexity as the LP.

== Decomposition Across Market Groups

When no orders span multiple market groups, the minting cost separates: $C_b = sum_j C_b^(G_j)$ (product-LMSR factorization). The EG program decomposes into independent per-group programs, coordinated only by MM budget allocation.

With log utility, splitting an MM's budget $B_k$ across components $m$ is a smooth concave problem: the coordination objective $max sum_m W_m^* (bold(B)^m)$ subject to $sum_m B_k^m = B_k$ has gradient $partial W_m^* \/ partial B_k^m = ln V_k^(m *)$ (envelope theorem), where $V_k^m = U_k^m + s_k^m$. The optimal allocation equalizes $V_k^(m *)$ across components: $V_k^(m *) = V_k^(m' *)$ for each MM $k$ active in components $m$ and $m'$. Standard mirror descent (multiplicative weights on budget shares) converges at rate $O(1\/t)$ (Beck and Teboulle 2003), with each iteration requiring only independent per-component solves.

With linear welfare, the same budget allocation is combinatorial: a hard constraint $"cap"_k <= B_k$ cannot be smoothly split across components. This is the computational payoff of the Fisher market structure — the same $B_k ln U_k$ objective that absorbs budget constraints in the monolithic program also enables decomposition across components.

_Validation._ An implementation of the LP, conic, and EG solvers confirms the theoretical predictions on synthetic order books with $50$ market groups, $~11,000$ single-market orders, and $3$ budget-constrained MMs. The quasi-Fisher (conic) and linear (LP) solvers produce near-identical welfare (gap $< 0.7%$ across all budget scales), confirming @prop-welfare and @prop-gap. When budgets are non-binding, the programs coincide exactly. Decomposition with mirror descent on budget shares converges toward the monolithic solution on single-market order books, as expected from the product-LMSR factorization.


= Discussion <discussion>

== Summary

Budget-constrained risk-neutral clearing is a non-convex bilinear program with no known polynomial-time algorithm (@prop-obstruction). Risk-averse clearing is a standard convex program with guaranteed uniqueness and exact limit orders (@thm-main). When budgets are non-binding, the two programs agree exactly (@prop-welfare). When they bind, the welfare gap is quadratic in the budget shortfall (@prop-gap).

== Open Problems

+ *Welfare gap tightness.* The quadratic bound of @prop-gap is conservative for typical order books: synthetic experiments show gaps under $0.7%$ even at $10 times$ budget oversubscription. Tighter instance-dependent bounds — exploiting the structure of real MM ladder orders — would sharpen the practical guarantees.

+ *Efficient combinatorial clearing.* The Fisher market isomorphism extends to joint state spaces (§5.5), but the state space $|cal(S)| = product K_j$ is exponentially large. Cross-group bundle orders couple groups transitively: if orders span $A$–$B$ and $B$–$C$, clearing requires the full $K_A times K_B times K_C$ space. In practice, a handful of bundle orders can connect all groups. Each order's payoff is a low-rank tensor (touching $<= 5$ groups), so the demand $D_s$ has exploitable structure. Can the EG program be solved in time polynomial in the number of orders rather than the state space? The analogous question for combinatorial LMSR (without budgets) is already open; budgets add a further layer.

+ *Risk-neutral hidden convexity.* Is the budget-constrained risk-neutral problem actually convex despite the bilinear constraints? The LMSR softmax prices grow fast enough that concentrated positions are budget-expensive, and we have found no instance with non-unique clearing prices. A proof of uniqueness (or a counterexample) would settle whether the computational obstruction of §3 is fundamental or merely an artifact of current proof techniques. Devanur and Dudík (2015) proved budget additivity for sequential LMSR, hinting at hidden convexity — but extending this to simultaneous batch auctions remains open.

== Connection to Prior Work

*Eisenberg and Gale (1959)*: The convex program for Fisher market equilibrium. Our @thm-main extends their framework to prediction markets with endogenous supply (minting) and mixed agent types (log-utility MMs alongside linear-welfare retail).

*Abernethy, Chen, and Vaughan (2013)*: Axiomatic foundation for cost-function market makers. Our §2 recasts their framework as batch-auction clearing: $V = max_k D_k$ is the simplest cost function, LMSR its entropy smoothing.

*Breiman (1961)*: Optimality of the Kelly criterion for repeated games. §4 uses this to argue log utility is not a convenience but a survival condition for repeat-participation MMs.

*Devanur and Dudík (2015)*: Price uniqueness and budget additivity for sequential LMSR. Their budget additivity hints at hidden convexity in the risk-neutral setting. Our @thm-main achieves unconditional uniqueness for batch auctions via EG; whether risk-neutral batches also have hidden convexity remains open.

*Fortnow, Kilian, Pennock, and Wellman (2005)*: LP for combinatorial call markets. Our group minting encodes mutual exclusivity in $O(K)$ constraints vs $O(2^K)$ enumeration.

*Chen and Pennock (2007)*: Bounded-loss market makers and the product-LMSR factorization. The parameter $b$ is their loss bound; $b = 0$ is achievable in batch clearing by complementary slackness.

*Cole et al. (2017), Chen et al. (2009)*: Quasi-linear Fisher markets. Our $P_b^"RA"$ is structurally a quasi-linear Fisher market; the contribution is the application to prediction markets with endogenous supply and LMSR pricing.

*Budish, Cramton, and Shim (2015)*: Frequent Batch Auctions for equity markets. Our framework applies FBAs to prediction markets.


#v(2em)
#line(length: 100%)
#v(0.5em)
#text(size: 9pt, style: "italic")[
  Implementation and experimental code available at the project repository. All solvers (LP, EG, conic) and the decomposition framework are implemented in Rust.
]
