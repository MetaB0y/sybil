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

The modeling "trick" is not logarithmic utility — it is the linear assumption. When we fix it, the pathology fixes itself.


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

Its dual variables are clearing prices. (Mutual exclusivity collapses the joint state space from $2^K$ to $K$; the LP uses $O(K)$ balance constraints and one group mint variable, versus $O(2^K)$ state evaluations for combinatorial LMSR.)

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

Applying KKT with box constraints $q_i in [0, overline(Q)_i]$ yields the familiar clearing rule (for a buy order $i$ on outcome $k$, the marginal welfare is $L_i - p_k$):
- $q_i = overline(Q)_i$ (fully filled) $quad arrow.l.r.double quad L_i >= p_k$ #h(1em) _(buyer's limit above price)_
- $q_i = 0$ (unfilled) $quad arrow.l.r.double quad L_i <= p_k$ #h(1em) _(buyer's limit below price)_
- $0 < q_i < overline(Q)_i$ (marginal) $quad arrow.l.r.double quad L_i = p_k$ #h(1em) _(buyer is the price-setter)_

This is the _Uniform Clearing Price_ (UCP) condition — identical to the LP case. The entropy smoothing does not alter order-matching logic; it only changes how prices depend on quantities: continuously via softmax rather than discretely via the marginal order.

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

*1. The feasible set is non-convex.* Define $h_k (bold(q)) = sum_(i in "MM"_k) p_(m(i))(bold(q)) dot q_i$ as MM $k$'s capital consumption. The Hessian of $h_k$ is _indefinite_: $det(H_(h_k)) = -(sigma(1-sigma))^2\/b^2 < 0$ (computed from the softmax price structure). Sublevel sets of functions with indefinite Hessians are not guaranteed convex.

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

§4 argues that correcting the economic model (from linear to logarithmic utility) resolves the inconsistency, and §5 proves that the correction resolves the non-convexity.


= Why Market Makers Have Diminishing Returns <economic-case>

The standard assumption that market makers have linear welfare is the unrealistic approximation. Logarithmic (Kelly-criterion) utility is the correct model for repeat-participation MMs. We give three theoretical arguments and two empirical observations that independently converge on this conclusion.

== Kelly Criterion Is a Survival Theorem

Breiman (1961) proved that Kelly maximization (log utility) is the _unique_ repeated-game strategy that maximizes long-run growth rate a.s., reaches any wealth target in minimum expected time, and dominates any other strategy with probability 1. An MM that sizes linearly faces ruin with probability 1 given enough batches.

A prediction market exchange runs _repeated_ batch auctions, compounding returns. The natural objective for a repeat participant is $max sum_t EE[ln(1 + r_t)]$ — exactly the Kelly criterion, exactly log utility per batch. Linear utility maximizes one-shot expected value but minimizes survival across batches. Since FBAs are inherently repeated, log utility is the only consistent objective.

Modeling MMs with log utility is not an assumption but a _selection effect_: the MMs that survive long enough to matter are the ones already using Kelly-like sizing.

== Budget Constraints _Are_ Risk Aversion

Why do MMs have budget constraints at all? A risk-neutral agent with positive expected value should bet everything. The existence of $B_k < "total wealth"$ is itself evidence of risk aversion: the MM limits exposure because it values capital preservation.

Linear welfare with a hard budget is internally inconsistent: "I value each share equally (risk-neutral) but I won't risk more than $B_k$ (risk-averse)." The budget is a crude piecewise-linear approximation of what log utility handles smoothly.

== Empirical Grounding

*Order books reveal concave demand.* Real MMs do not post one order at one limit price for their entire budget. They post _ladders_: multiple orders at decreasing limit prices with decreasing quantities. Each tranche has lower welfare and smaller size — diminishing returns, expressed directly through the order book. The linear model treats each rung as an independent agent with constant marginal value; the log model captures a single agent expressing a concave demand curve.

*Inventory risk penalizes concentration.* An MM that buys 1,000 shares of Yes at \$0.50 has \$500 of inventory risk. A linear-utility MM is indifferent — the 1,001st share has the same marginal value as the 1st. In practice, every MM applies position limits: hard caps on per-market exposure. These are another piecewise-linear approximation of diminishing returns. Log utility makes position limits unnecessary — the natural concavity of $ln$ penalizes concentration intrinsically.

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

The non-convexity of §3 is not a feature of prediction markets — it is an artifact of the wrong model.


= The Main Result: Risk-Averse Clearing Is a Fisher Market <risk-averse>

Replacing linear MM welfare with Kelly-criterion utility transforms the budget-constrained clearing problem into a convex program with a unique solution. Budget constraints disappear from the formulation — they are absorbed into the Eisenberg-Gale objective. The resulting program is structurally isomorphic to a quasi-linear Fisher market with endogenous supply.

== The Program

#theorem(name: "Risk-Averse Clearing Is Convex")[
  _Define the risk-averse batch auction clearing program with cash retention:_

  $ P_b^"RA": quad max_(bold(q) in cal(C), bold(s) >= 0) quad underbrace(sum_k [B_k ln(U_k(bold(q)) + s_k) - s_k], "MM welfare") + underbrace(sum_(j in.not "MM") w_j q_j, "retail welfare") - underbrace(C_b(bold(D)(bold(q))), "minting cost") $

  _where $U_k(bold(q)) = sum_(i in "MM"_k) L_i q_i$ is MM $k$'s total weighted fill, $s_k >= 0$ is MM $k$'s retained cash, $"MM"_k$ contains MM $k$'s buy orders (with $L_i > 0$; any sell orders from MMs contribute to the retail welfare term), $cal(C) = {bold(q) in [0, bold(overline(Q))]: "balance constraints"}$ is the LP feasible set, and $C_b$ is the smoothed minting cost (@prop-sandwich). Define $mu_k = B_k \/ (U_k + s_k)$ as the shadow price of capital. Then:_

  + _The objective is strictly concave. $P_b^"RA"$ has a unique optimum $(bold(q)^*, bold(s)^*)$._
  + _Limit orders are exact: $mu_k <= 1$, so no MM order fills at negative welfare ($L_i < p_k$)._
  + _No explicit budget constraints appear. At the optimum, each MM $k$ spends at most $B_k$: capital on fills plus retained cash $sum p_(m(i)) q_i + s_k <= B_k$._
  + _Clearing prices $bold(p)^*$ are unique._
  + _The program is solvable in polynomial time by any standard convex optimizer._
  + _The program operates in two regimes per MM:_
    - _Capital-constrained ($U_k >= B_k$): $s_k = 0$, $mu_k = B_k\/U_k < 1$. The MM prioritizes highest-ROI fills, organically staying within budget._
    - _Over-capitalized ($U_k < B_k$): $s_k = B_k - U_k$, $mu_k = 1$. The MM clears identically to the risk-neutral LP, absorbing excess budget into cash._
] <thm-main>

== Proof

*(1) Strict concavity and uniqueness.* Each $B_k ln(U_k + s_k)$ is the composition of $ln$ (strictly concave, increasing) with a positive affine function of $(bold(q), s_k)$ — strictly concave. The $-s_k$ term is linear. Retail welfare is linear. $-C_b$ is concave. The sum is strictly concave. The objective tends to $-infinity$ as $s_k -> infinity$ (since $-s_k$ dominates $ln s_k$), so the effective feasible set is compact. Strict concavity on a compact convex set gives a unique maximizer.

*(2) Limit order exactness.* The KKT condition for $s_k >= 0$ is:

$
partial / (partial s_k): quad mu_k - 1 <= 0, quad s_k >= 0, quad (mu_k - 1) s_k = 0
$

where $mu_k = B_k \/ (U_k + s_k)$. This gives $mu_k <= 1$ unconditionally. The KKT for fill $q_i$ of agent $k$ is $mu_k L_i - p_(m(i)) = lambda_i^+ - lambda_i^-$. A fill ($q_i > 0$) requires $mu_k L_i >= p_(m(i))$, hence $L_i >= p_(m(i)) \/ mu_k >= p_(m(i))$: the limit price must exceed the clearing price. No negative-welfare fill is possible.

The two regimes follow from complementary slackness: $s_k = 0$ when $mu_k < 1$ (i.e., $U_k > B_k$), and $s_k = B_k - U_k > 0$ when $mu_k = 1$ (i.e., $U_k < B_k$). In the over-capitalized regime ($mu_k = 1$), the fill condition $L_i >= p_(m(i))$ is exactly the risk-neutral UCP condition — the MM clears identically to the LP.

*(3) Budget absorption.* Multiply the fill KKT by $q_i$ and sum over $i in "MM"_k$. By complementary slackness ($lambda_i^- q_i = 0$):

$
mu_k underbrace(sum_(i in "MM"_k) L_i q_i, = U_k) = sum_(i in "MM"_k) p_(m(i)) q_i + underbrace(sum_(i in "MM"_k) lambda_i^+ q_i, >= 0)
$

So $sum p_(m(i)) q_i <= mu_k U_k$. Since $mu_k (U_k + s_k) = B_k$, we have $mu_k U_k = B_k - mu_k s_k <= B_k$:

$ sum_(i in "MM"_k) p_(m(i))^* q_i^* + s_k^* <= B_k $

Each MM's total deployment — capital on fills plus retained cash — is at most $B_k$. The budget emerges from the objective, not from an explicit constraint. This is the Eisenberg-Gale mechanism extended to quasi-linear utilities: the $ln$ singularity absorbs the budget, and the cash variable absorbs the surplus.

*(4) Price uniqueness.* Prices are $p_k = partial C_b \/ partial D_k = "softmax"(bold(D)\/b)$, a continuous function of the unique $bold(q)^*$.

*(5) Polynomial solvability.* The objective is concave and $C^infinity$ (for $b > 0$). The feasible set is a polytope times $RR_+^(|cal(K)|)$. Standard interior-point methods solve this in polynomial time. #h(1fr) $square$

== Temperature Independence

In the risk-neutral model, entropy smoothing was the only source of concavity — insufficient against budget non-convexity (@prop-obstruction). Here, $B_k ln(U_k + s_k)$ provides $b$-independent curvature. Even at $b = 0$ (pure LP minting cost), the program remains strictly concave:

$ P_0^"RA": quad max_(bold(q) in cal(C), bold(s) >= 0) quad sum_k [B_k ln(U_k + s_k) - s_k] + sum_(j in.not "MM") w_j q_j - max_k D_k $

The $-max_k D_k$ term is concave (not strictly), but the $ln$ term provides strict concavity. Uniqueness and limit order exactness hold at every temperature, including $b = 0$.

== Welfare Convergence

The cash retention variable makes the risk-averse program a strict generalization of the risk-neutral LP: when budgets are non-binding, the two programs agree exactly.

#proposition(name: "LP Recovery")[
  _Let $bold(q)^"LP"$ be the unconstrained LP optimum ($P_b$ without budgets). If $B_k >= U_k^"LP" = sum_(i in "MM"_k) L_i q_i^"LP"$ for every MM $k$, then the risk-averse optimum $(bold(q)^*, bold(s)^*)$ satisfies $bold(q)^* = bold(q)^"LP"$ and $s_k^* = B_k - U_k^"LP"$. Fills, prices, and welfare are identical._
] <prop-welfare>

_Proof._ When $B_k >= U_k^"LP"$ for all $k$, every MM is in the over-capitalized regime at $bold(q)^"LP"$: the optimizer sets $s_k = B_k - U_k^"LP" >= 0$ and $mu_k = 1$. The fill KKT reduces to $L_i - p_(m(i)) = lambda_i^+ - lambda_i^-$ — exactly the LP KKT. Since the LP and risk-averse programs have the same KKT conditions at $bold(q)^"LP"$, and the risk-averse program has a unique optimum, $bold(q)^* = bold(q)^"LP"$. #h(1fr) $square$

The cash variable $s_k$ acts as a _numeraire good_: the MM can "buy" cash at price \$1, receiving no market exposure. This eliminates the over-fill pathology of the naive $B_k ln U_k$ model (which forces all budget into fills, including unprofitable ones). The program interpolates smoothly between two regimes:

- _Capital-constrained_ ($U_k > B_k$, $s_k = 0$): the MM has more profitable opportunities than budget. Kelly sizing kicks in — the MM prioritizes the highest return-on-investment fills, deploying the full budget. This is where the log model departs from the LP.

- _Over-capitalized_ ($U_k < B_k$, $s_k > 0$): the MM has more budget than profitable fills. Excess capital is retained as cash ($mu_k = 1$), and fills match the LP exactly. No order fills below its limit price.

The welfare gap between $P_b^"RA"$ and the LP comes entirely from the capital-constrained regime: when $B_k < U_k^"LP"$, the MM throttles fills (under-filling relative to the LP, never over-filling). The gap is bounded by the total budget shortfall $sum_k max(0, U_k^"LP" - B_k)$ and vanishes as budgets grow.

== The Fisher Market Isomorphism

In a Fisher market (Eisenberg & Gale 1959), $n$ consumers with budgets $B_k$ purchase divisible goods with fixed supply $s_j$ at prices $p_j$. The equilibrium is the unique solution to:

$ "EG": quad max_(bold(x) >= 0) quad sum_k B_k ln U_k (bold(x)_k) quad "s.t." quad sum_k x_(k j) <= s_j quad forall j $

where $U_k(bold(x)_k) = sum_j u_(k j) x_(k j)$ is consumer $k$'s linear utility over goods. The supply constraints have dual variables $p_j$ — the equilibrium prices. Budget constraints do not appear explicitly; they emerge from the $B_k ln U_k$ objective by the same telescoping argument as our proof of (3). Adding a cash variable $s_k$ with cost $-s_k$ yields the _quasi-linear_ Fisher market — agents can retain unspent budget as cash rather than being forced to spend it all on goods.

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
    [Utility], [$U_k = sum_j u_(k j) x_(k j)$], [$U_k = sum_(i in "MM"_k) L_i q_i$],
    [Cash], [Optional ($s_k >= 0$)], [Retained budget ($s_k >= 0$)],
    [Prices], [Dual of $sum_k x_(k j) <= s_j$], [Gradient of $C_b$ (softmax)],
  )
]

The prediction market extends the quasi-linear Fisher market in two ways: (1) supply is endogenous — shares are created by minting at cost $C_b(bold(D))$ rather than drawn from a fixed endowment, and (2) non-MM ("retail") orders contribute linear welfare alongside the log-utility MMs. Both extensions preserve concavity. The minting cost replaces the fixed supply constraints: where EG has $sum_k x_(k j) <= s_j$ with dual prices, $P_b^"RA"$ has $C_b(bold(D))$ whose gradient _is_ the price vector. The cash variable $s_k$ ensures that over-capitalized MMs park excess budget rather than distorting fills — the quasi-linear structure is essential, not optional.

== What Changes and What Doesn't

*What changes.* Under $P_b^"RA"$, capital-constrained MMs ($mu_k < 1$) prioritize their highest-ROI fills rather than filling every profitable order equally. An MM concentrating capital on one market faces the $ln$ penalty: the first dollar of fill generates high marginal utility, additional dollars generate diminishing utility. This naturally diversifies MM capital across markets. Over-capitalized MMs ($mu_k = 1$) behave identically to the LP — the log model only affects sizing when the budget actually binds.

*What doesn't change.* Non-MM orders are still matched by UCP at clearing prices. The minting mechanism is unchanged. Prices are still softmax (for $b > 0$) or LP duals (for $b = 0$). Limit orders are exact: no order fills below its stated price.

*The buy/sell decomposition.* The Fisher market isomorphism applies to MM buy orders — the orders that deploy capital and create demand. MM sell orders (if any) are treated as retail: filled by UCP with linear welfare, consuming no budget. This is economically correct: buying creates exposure and depletes the trading budget; selling liquidates existing exposure and _frees_ budget. A sell fill returns capital to the MM rather than consuming it, so it should not appear inside $B_k ln(U_k + s_k)$. For naked shorts (selling shares not yet held), collateral comes from a separate margin pool — a distinct balance with its own risk management, not the MM's active trading budget $B_k$.

*Extension to multiple groups and bundle orders.* Nothing in @thm-main requires a single mutually exclusive group. Consider $N$ groups, each with $K_j$ mutually exclusive outcomes. The joint state space is $cal(S) = product_j {1, dots, K_j}$ (exactly one joint state is realized). Bundle orders — orders whose payoffs depend on the joint state — create demand $D_s$ over $cal(S)$. The minting cost generalizes to $V(bold(D)) = max_s D_s$ (minting one complete set of all joint states costs \$1), and the smoothed version is $C_b = b ln sum_s exp(D_s\/b)$. Both are convex in $bold(D)$, and $bold(D)$ is linear in $bold(q)$. The proof of @thm-main goes through unchanged: $B_k ln(U_k + s_k)$ is still strictly concave, $-C_b$ is still concave, budget absorption and limit order exactness still hold. The Fisher market isomorphism holds over arbitrary joint state spaces.

When no orders span multiple groups, the joint problem decomposes: $C_b = sum_j C_b^(G_j)$ (the product-LMSR factorization of Chen and Pennock 2007). Cross-group orders break this separability, coupling groups transitively — if orders link groups $A$–$B$ and $B$–$C$, the clearing problem requires the full $K_A times K_B times K_C$ state space. The mathematical obstruction is purely computational, not structural (see §6.2).


= Discussion <discussion>

== Summary

The paper establishes one main result and the framework necessary to state and prove it:

#align(center)[
  #table(
    columns: 3,
    align: (left, center, center),
    [*Section*], [*Result*], [*Role*],
    [§2], [LMSR = smoothed LP (Thms 1–4)], [Framework],
    [§3], [Computational obstruction (Prop.~2)], [Motivation],
    [§4], [Log utility is correct model], [Economic argument],
    [§5], [Fisher market isomorphism (Thm 5); welfare bound (Prop.~3)], [*Main result*],
  )
]

The landscape: budget-constrained risk-neutral clearing is a non-convex bilinear program with no known polynomial-time algorithm (@prop-obstruction); risk-averse clearing is a standard convex program with guaranteed uniqueness and exact limit orders (@thm-main). When budgets are non-binding, the two programs agree exactly (@prop-welfare). The transition from intractability to tractability requires only the change of objective from $sum w_i q_i$ to $sum B_k [ln(U_k + s_k) - s_k]$.

== Open Problems

+ *Welfare gap in practice.* @prop-welfare shows $P_b^"RA"$ matches the LP exactly when MM budgets are non-binding. The welfare gap in the capital-constrained regime (where log utility throttles fills relative to the LP) depends on the budget shortfall $sum max(0, U_k^"LP" - B_k)$. Quantifying this gap for realistic order book structures remains an empirical question.

+ *Efficient combinatorial clearing.* The Fisher market isomorphism extends to joint state spaces (§5.5), but the state space $|cal(S)| = product K_j$ is exponentially large. Cross-group bundle orders couple groups transitively: if orders span $A$–$B$ and $B$–$C$, clearing requires the full $K_A times K_B times K_C$ space. In practice, a handful of bundle orders can connect all groups. Each order's payoff is a low-rank tensor (touching $<= 5$ groups), so the demand $D_s$ has exploitable structure. Can the EG program be solved in time polynomial in the number of orders rather than the state space? The analogous question for combinatorial LMSR (without budgets) is already open; budgets add a further layer.

+ *Risk-neutral hidden convexity.* Is the budget-constrained risk-neutral problem actually convex despite the bilinear constraints? The LMSR softmax prices grow fast enough that concentrated positions are budget-expensive, and we have found no instance with non-unique clearing prices. A proof of uniqueness (or a counterexample) would settle whether the computational obstruction of §3 is fundamental or merely an artifact of current proof techniques. Devanur and Dudík (2015) proved budget additivity for sequential LMSR, hinting at hidden convexity — but extending this to simultaneous batch auctions remains open.

== Connection to Prior Work

*Eisenberg and Gale (1959)*: Convex program for Fisher market equilibrium via expenditure variables. Our @thm-main establishes that prediction markets with risk-averse MMs are isomorphic to Fisher markets: the Eisenberg-Gale program with endogenous supply (minting cost $C_b$) is convex. §3 identifies why the same approach fails for risk-neutral MMs: constant marginal returns provide no curvature to counteract the budget non-convexity.

*Abernethy, Chen, and Vaughan (2013)*: Proved that cost-function market makers satisfying a set of axioms must price via a convex cost function, with LMSR corresponding to the entropy conjugate. Our §2 is the batch-auction formulation of their framework. The minting cost $V = max_k D_k$ is the "simplest" cost function, with LMSR as its entropy smoothing.

*Breiman (1961)*: Optimal properties of the Kelly criterion. Our §4 uses Breiman's survival theorem to argue that log utility is not a convenience but a consequence of evolutionary selection among repeat-participation MMs.

*Devanur and Dudík (2015)*: Price uniqueness and budget additivity for sequential LMSR via Bregman divergence. Their budget additivity is a manifestation of hidden convexity — the bilinear budget boundaries merge into a convex hull in the dual space. Our @thm-main achieves unconditional uniqueness for batch auctions via the same Eisenberg-Gale structure underlying their result. Whether the _risk-neutral_ batch auction also has hidden convexity remains open (§3 shows it cannot come from the standard Fenchel dual).

*Fortnow, Kilian, Pennock, and Wellman (2005)*: LP for combinatorial call markets. Our group minting provides the structural reason why LP works: it encodes mutual exclusivity in $O(K)$ vs $O(2^K)$.

*Chen and Pennock (2007)*: Bounded-loss market makers. The parameter $b$ in our framework is their loss bound. $b = 0$ (zero loss) is achievable in the batch setting by complementary slackness: the minting mechanism breaks even exactly when prices are LP duals.

*Hofbauer and Sandholm (2007)*: Unique logit equilibrium for negative semidefinite games — games where increasing adoption of a strategy decreases its payoff. Our prediction market is exactly NSD (demand raises price). Their result covers the unconstrained case; our Fisher market isomorphism extends to budget-constrained agents unconditionally.

*McKelvey and Palfrey (1995)*: Quantal Response Equilibrium. The budget-constrained clearing problem is the prediction-market analog of QRE with budget constraints. Our impossibility result (@prop-obstruction) corresponds to the failure of high-temperature contraction when budgets bind; our resolution via log utility corresponds to adopting a different utility model entirely.

*Budish, Cramton, and Shim (2015)*: Frequent Batch Auctions for equity markets. Our framework applies FBAs to prediction markets, where the information-driven price shocks are even more severe than in equities.


#v(2em)
#line(length: 100%)
#v(0.5em)
#text(size: 9pt, style: "italic")[
  Next steps: (1) Implement risk-averse clearing (@thm-main) — single convex program, no annealing. (2) Empirical welfare comparison between $P_b^"RA"$ and risk-neutral LP across realistic order books, measuring the over-fill cost of @prop-welfare for typical MM ladder structures.
]
