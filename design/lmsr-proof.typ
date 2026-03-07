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
  We identify the exact reduced-form utility of a budgeted market maker with retained cash (@prop-reduced): affine when capital is slack and logarithmic when capital binds. This converts prediction market batch auctions into a Fisher-market clearing program (@thm-main). A deployed-value lift $V_k = U_k + s_k$ derives the reduced form exactly and yields a concave batch-clearing program with exact KKT interpretation and polynomial-time conic formulations. Budget constraints vanish from the formulation entirely, absorbed into the objective. For every $b > 0$, clearing prices are unique; at $b = 0$, prices can be non-unique as in the LP. Prices, fills, and capital deployment emerge simultaneously from a single optimization, although order-level fills need not be unique without additional nondegeneracy assumptions.
]

#v(1em)

= Introduction

The main result of this paper is that prediction market batch auctions admit a Fisher-market formulation. The decisive observation is that a budgeted MM with retained cash has a simple reduced-form utility: affine when capital is slack, logarithmic when capital binds.

A prediction market exchange runs _batch auctions_: orders accumulate, then clear simultaneously at uniform prices. The clearing problem is an allocation problem — find the fills and prices that maximize welfare subject to balance constraints. Without budget constraints, this is a standard Linear Program.

Market makers introduce the main difficulty. An ordinary participant submits a single order: "buy 100 shares of Yes at \$0.60." The capital required (\$60) is known at submission and locked upfront — no budget constraint needed. A market maker, by contrast, posts _hundreds_ of orders across _dozens_ of markets simultaneously. The total capital consumed depends on which orders fill and at what prices — both determined by the auction. The MM deposits a finite balance $B_k$ and the exchange must ensure total spending does not exceed it. This budget constraint is _bilinear_: capital consumed depends on the clearing price, and the clearing price depends on which fills happen. This single constraint makes the feasible set non-convex.

The key object is therefore not the hard budget constraint but the MM's reduced-form utility. Let $U_k$ be the weighted fill obtained by MM $k$. Once retained cash is allowed, optimizing over that cash induces a utility @prop-reduced that is affine below budget and logarithmic above it. The deployed-value variable $V_k = U_k + s_k$ is the technical lift that derives this reduced form exactly and later makes the KKT system and conic formulation transparent. This single observation explains the whole paper: LP recovery when budgets do not bind, endogenous budget absorption when they do, and a clean price dual.

We prove three things, in order:

+ *The framework (§2).* LP batch clearing and Hanson's LMSR are the same mathematical object at different temperatures, connected by Fenchel duality. This is the foundation for everything that follows. It is also where we prove that clearing prices are unique when budgets are absent — a fact that makes the budget obstacle precise.

+ *The obstacle (§3).* Adding budget constraints to the risk-neutral model can make the feasible set non-convex. Standard convex optimization no longer applies, no polynomial-time algorithm is known, and uniqueness of clearing prices is an open question. The structural cause: linear welfare with hard budget caps is an internally inconsistent economic model.

+ *The resolution (§§4–6).* Retained cash induces an exact MM utility @prop-reduced: affine when capital is slack, logarithmic when capital binds. This reduced form yields a concave batch-clearing program, exact limit-order KKT conditions, endogenous budget absorption, unique smoothed prices for $b > 0$, and polynomial-time conic formulations. The deployed-value lift is the proof and solver layer behind this reduced form. This is the Fisher market isomorphism.

The modeling "trick" is not merely logarithmic utility — it is identifying the reduced-form MM utility induced by budget plus retained cash. When we replace the linear-with-hard-cap model by that reduced form, the pathology fixes itself. §6 shows the result is practical: the program admits a conic formulation with $K$ exponential cones and suggests a smooth budget-allocation decomposition across independent market groups.


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

*Choosing $b$ in practice.* The parameter $b$ is a smoothing knob, and @prop-sandwich gives its dollar-scale cost directly: the maximum extra minting subsidy is $b ln K$. A practical rule is to choose $b$ so that $b ln K$ is below the venue's tolerated per-batch subsidy or below one economically meaningful price tick. Then the smoothed auction remains close to LP clearing while still delivering unique clearing prices for every $b > 0$.

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
  _The Fenchel dual of the unconstrained smoothed problem is strictly convex on the simplex: $W^*$ is convex and $C_b^* (bold(p)) = b sum_k p_k ln p_k$ is strictly convex on $Delta$ (with the convention $0 ln 0 := 0$). The clearing prices $bold(p)^*$ are therefore unique for any $b > 0$ and any order book, unconditionally._
] <thm-unique>

_Proof._ $W^*$ is convex because it is a Fenchel conjugate. The scalar function $x |-> x ln x$ is strictly convex on $[0, +infinity)$, so $sum_k p_k ln p_k$ is strictly convex on the convex set $Delta$. Therefore $W^* + C_b^*$ is strictly convex on $Delta$. Since $Delta$ is compact and the dual objective is lower-semicontinuous, a minimizer exists; strict convexity implies it is unique. #h(1fr) $square$


= The Budget Obstacle <obstacle>

When market makers have budget constraints, the clearing problem becomes fundamentally harder. Without budgets, batch clearing is a standard convex program (@thm-unique). With budgets, the capital constraint is bilinear — capital consumed depends on clearing prices, which depend on fills — and standard convex optimization no longer applies.

== The Bilinear Constraint

A market maker $k$ deposits balance $B_k$ and posts orders across multiple markets. The capital consumed by each fill depends on the clearing price:

$
"cap"_k(bold(p), bold(q)) = sum_(i in "MM"_k) c_i(p_(m(i))) dot q_i, quad c_i(p) = cases(p & "if BuyYes/SellNo", 1-p & "if SellYes/BuyNo")
$

The budget constraint $"cap"_k <= B_k$ is _bilinear_: $p$ is determined by $bold(q)$ through the clearing mechanism. The product $c(p(bold(q))) dot q$ can make the feasible set non-convex in fill space. This single constraint is what separates prediction market clearing from a standard LP.

== Computational Consequences <computational>

The bilinear budget constraint has three consequences for computation:

*1. The feasible set can be non-convex.* Define $h_k (bold(q)) = sum_(i in "MM"_k) p_(m(i))(bold(q)) dot q_i$ as MM $k$'s capital consumption. We start with the simplest non-trivial case: an MM with buy orders on two independent binary groups, one order per group, fill quantities $q_1, q_2$. Since groups are independent, $h = f(q_1) + f(q_2)$ where $f(q) = sigma(q\/b) dot q$ and $sigma$ is the logistic sigmoid. Differentiating:
$
f''(q) = (sigma(1-sigma)) / b [2 + q(1-2sigma) / b]
$
Since $sigma(1-sigma) > 0$, the sign depends on $2 + q(1-2sigma)\/b$. At $q = 0$: $f''(0) = 1\/(2b) > 0$ (convex). For large $q$: $sigma -> 1$ and the bracket becomes $2 - q\/b -> -infinity$ (concave). The inflection point $f''(q^*) = 0$ is at $q^* approx 2.4 b$. So $h$ is neither convex nor concave.

This witness is not knife-edge in $b$. Writing $f_b(q) = sigma(q\/b) q$, we have the scaling identity $f_b(b t) = b f_1(t)$ and therefore $h_b(bold(q)) = b h_1(bold(q)\/b)$. So for _every_ $b > 0$, the points
$
x_b = (2 b, 9 b), quad y_b = (9 b, 2 b), quad m_b = (5.5 b, 5.5 b)
$
satisfy
$
h_b(x_b) = h_b(y_b) approx 10.7605 b, quad h_b(m_b) approx 10.9552 b
$
Hence for budget $B = 10.8 b$, both endpoints satisfy $h_b <= B$ while the midpoint violates it. Therefore the sublevel set ${h_b <= B}$ is non-convex for every $b > 0$.

*2. No standard convex algorithm applies directly.* The unconstrained problem $P_b$ is a smooth concave maximization (solved by interior point in polynomial time). Adding budget constraints ${h_k <= B_k}$ can make the feasible set non-convex. Standard convex optimization — interior point, projected gradient, Frank-Wolfe — requires convex feasible sets. The budget-constrained problem is a bilinear program, for which no polynomial-time algorithm is known in general.

*3. The uniqueness question is open.* The proof of @thm-unique (price uniqueness without budgets) uses strict convexity of the Fenchel dual. With budgets, the argument breaks: uniqueness at one price vector requires _cross-price budget feasibility_ ($"cap"_k (bold(p)^2, bold(q)^1) <= B_k$), which the bilinear constraint does not guarantee. Whether clearing prices are nonetheless unique for all LMSR instances is an open question. (In the degenerate case of two identical markets with a symmetric MM, two KKT points exist by symmetry — but this requires exact parameter tuning and does not extend to generic order books.)

#proposition(name: "Computational Obstruction")[
  _For every $b > 0$, the budget-constrained risk-neutral clearing problem admits instances whose feasible set ${bold(q) in cal(C) : h_k (bold(q)) <= B_k}$ has non-convex sublevel sets, so the problem is a bilinear program and standard convex optimization does not apply directly. By contrast, the risk-averse program $P_b^"RA"$ (@thm-main) is a standard convex program with conic formulations solvable in polynomial time._
] <prop-obstruction>

In practice, exchanges handle budgets by iterative heuristics: solve the LP ignoring budgets, check for violations, adjust, repeat. Such methods have no convergence guarantee and can cycle. The risk-averse program eliminates this entirely — budgets are absorbed into the objective.

== The Expenditure Perspective

A natural idea: change variables to capital expenditures $e_i = c_i(p) dot q_i$, which linearizes the budget constraint to $sum_(i in "MM"_k) e_i <= B_k$. In Fisher markets, the Eisenberg-Gale program works precisely because this change of variables produces a convex program. Does it work here?

No. Welfare transforms to $sum_i w_i dot e_i \/ c_i(p)$ — a _rational_ function of prices. In Fisher markets (Eisenberg & Gale 1959), the analogous program is convex because agents have _diminishing-returns_ utilities: $sum B_k ln U_k$ provides curvature. Our MMs have constant marginal returns (linear welfare), providing none. The expenditure substitution linearizes the budget but introduces non-convexity into the objective.

The diagnosis is structural: _linear welfare with hard budget caps_ is an internally inconsistent economic model — agents simultaneously risk-neutral (constant marginal value) and risk-averse (capped exposure). The non-convexity is the computational symptom.

The mathematics tells us exactly what is needed: a concave reduced-form MM objective with endogenous cash retention. §4 argues that diminishing returns is not merely a mathematical convenience but a plausible model for repeat-participation MMs. §5 derives the resulting utility, and §6 uses it to build the clearing program.


= Why Diminishing Returns Is a Plausible MM Model <economic-case>

The previous section showed that clearing requires diminishing-returns utility to be computationally tractable. This section is a modeling argument rather than part of the proof spine: it gives reasons diminishing returns is a plausible reduced-form model for repeat-participation MMs.

== Kelly as Repeated-Game Motivation

Breiman (1961) showed that Kelly-style log utility is growth-optimal under classical repeated favorable-bet assumptions. This does not imply that every real MM literally maximizes log utility, but it does provide a principled repeated-game foundation for diminishing returns.

A prediction market exchange runs _repeated_ batch auctions, compounding returns. If an MM evaluates batches through long-run capital growth, an objective of the form $max sum_t EE[ln(1 + r_t)]$ is natural. Linear utility remains appropriate for one-shot welfare, but it misses sizing and survival tradeoffs across batches.

The modeling claim is therefore modest: log utility is not arbitrary; it is a defensible reduced form for repeat participants who care about growth and drawdown.

== Budget Limits Encode Risk Aversion

Why do MMs have budget constraints at all? A risk-neutral agent with positive expected value should bet everything. The existence of $B_k < "total wealth"$ is itself evidence of risk aversion: the MM limits exposure because it values capital preservation.

Linear welfare with a hard budget is internally inconsistent: "I value each share equally (risk-neutral) but I won't risk more than $B_k$ (risk-averse)." The budget is a crude piecewise-linear approximation of what log utility handles smoothly.

== Empirical Grounding

In practice, many MMs post _ladders_ — multiple orders at decreasing limit prices with decreasing quantities — revealing concave demand directly through the order book. They also impose position limits to cap inventory risk: a linear-utility MM is indifferent between the 1st and 1,001st share, but real MMs are not. Both ladders and position limits are piecewise-linear approximations of diminishing returns. Log utility captures concave demand natively and penalizes concentration intrinsically.

== Synthesis
These arguments are motivational, not part of the theorem spine. The paper does _not_ need Kelly to be the only correct behavioral model of real MMs. It only needs a concave MM objective with endogenous cash retention, and log utility gives the cleanest reduced form. The question is therefore: what utility does a budgeted MM with retained cash actually have? The answer is the affine-to-log function derived in the next section. The non-convexity of §3 is better viewed as a pathology of the linear-with-hard-caps model than as an inherent feature of prediction markets.

= Reduced-Form Utility and Fisher-Market Clearing <risk-averse>

Replacing linear MM welfare with Kelly-style diminishing returns transforms the budget-constrained clearing problem into a concave optimization problem. The main object is the reduced-form MM utility @prop-reduced. The deployed-value variable $V_k$ is the exact lift behind it: useful for derivation, KKT analysis, and conic computation, but not the main conceptual payload. Once the reduced form is identified, LP recovery, welfare-gap control, and the decomposition gradient all become consequences of one function.

== Reduced-Form MM Utility

#proposition(name: "Reduced-Form MM Utility")[
  _Optimizing over the deployed-value variable yields the MM utility_
  $ psi_(B_k)(U) = cases(
    U + B_k ln B_k - B_k & "if" U <= B_k,
    B_k ln U & "if" U >= B_k
  ) $
  _This utility is concave and $C^1$, with derivative $psi_(B_k)'(U) = min(1, B_k \/ U)$. It satisfies the affine-envelope bound $psi_(B_k)(U) <= U + B_k ln B_k - B_k$, with equality exactly on the slack-budget region $U <= B_k$. So MM utility is affine when budget is slack and logarithmic when capital binds._
] <prop-reduced>

_Proof._ Given fill value $U$, maximize over deployed value:
$
psi_(B_k)(U) = max_(V >= U) [B_k ln V - V + U]
$
The derivative and curvature are $B_k \/ V - 1$ and $-B_k \/ V^2 < 0$, so the maximizer is $V^* = max(U, B_k)$. Substituting $V^*$ gives the piecewise formula, and the derivative follows from the two branches. #h(1fr) $square$

This is the paper's reduced form. LP recovery, the welfare-gap bound, and the decomposition gradient in the companion note are all consequences of the two regimes encoded by $psi_B$: affine below budget, logarithmic above it.

#proposition(name: "Deployed-Value Lift")[
  _The reduced-form clearing problem_
  $ max_(bold(q) in cal(C)) quad sum_k psi_(B_k)(U_k(bold(q))) + sum_(j in.not "MM") w_j q_j - C_b(bold(D)(bold(q))) $
  _is equivalent to the deployed-value lift_
  $ max_(bold(q) in cal(C), bold(V) >= bold(U)(bold(q))) quad sum_k [B_k ln V_k - V_k + U_k(bold(q))] + sum_(j in.not "MM") w_j q_j - C_b(bold(D)(bold(q))) $
  _because each MM term is the pointwise maximum over $V_k >= U_k$ of the lifted objective._
] <prop-vlift>

_Proof._ Immediate from @prop-reduced, applied MM-by-MM. #h(1fr) $square$

== Reduced-Form Clearing Program

#theorem(name: "Reduced-Form Clearing with Budgeted Market Makers")[
  For each MM $k$, let $psi_(B_k)$ be the reduced-form utility of @prop-reduced and consider

  $ P_b^"RA": quad max_(bold(q) in cal(C)) quad underbrace(sum_k psi_(B_k)(U_k(bold(q))), "MM welfare") + underbrace(sum_(j in.not "MM") w_j q_j, "retail welfare") - underbrace(C_b(bold(D)(bold(q))), "minting cost") $

  where $"MM"_k^+$ is the set of MM $k$'s buy orders (sell orders from MMs enter the retail welfare term; see Buy/Sell Reduction below), $U_k(bold(q)) = sum_(i in "MM"_k^+) L_i q_i >= 0$ is MM $k$'s total weighted fill ($L_i > 0$ for all $i in "MM"_k^+$), $cal(C) = {bold(q) in [0, bold(overline(Q))]: "balance constraints"}$ is the feasible fill polytope, and $C_b$ is the smoothed minting cost (@prop-sandwich). At any optimum $bold(q)^*$, define deployed value $V_k^* = max(U_k(bold(q)^*), B_k)$ and capital-scarcity factor $alpha_k = B_k \/ V_k^* = psi_(B_k)'(U_k(bold(q)^*)) <= 1$. Then:

  + The objective is concave, the feasible set is convex, and an optimum exists.
  + The reduced-form program is equivalent to the deployed-value lift of @prop-vlift.
  + Limit orders are exact: if MM order $i$ fills, then $alpha_k L_i >= p_(m(i))$, hence $L_i >= p_(m(i))$. No negative-welfare MM fill is possible.
  + No explicit budget constraints appear. Yet at the optimum, each MM $k$ spends at most $B_k$: capital on fills plus retained cash $sum_(i in "MM"_k^+) p_(m(i)) q_i + (V_k^* - U_k(bold(q)^*)) <= B_k$.
  + The program operates in two regimes per MM. If $U_k(bold(q)^*) < B_k$, then $alpha_k = 1$ and the MM clears exactly as in the risk-neutral LP. If $U_k(bold(q)^*) > B_k$, then $alpha_k = B_k \/ U_k(bold(q)^*) < 1$ and lower-ROI fills are throttled.
  + For $b > 0$, clearing prices are unique and equal the softmax rule $p_k = (partial C_b) / (partial D_k)$. At $b = 0$, clearing prices are LP dual variables and may be non-unique.
  + The program is polynomial-time solvable through the conic reformulations in the Computation section.
] <thm-main>

#block(
  inset: (x: 0.9em, y: 0.7em),
  stroke: 0.4pt,
)[
  *Proof Map.*
  #table(
    columns: 3,
    align: (left, left, left),
    stroke: none,
    [*Move*], [*Object*], [*Payoff*],
    [Identify utility], [@prop-reduced], [Affine-below-budget, log-above-budget MM objective],
    [Lift to $V$], [@prop-vlift], [Exact derivation, KKT system, and solver form],
    [Read KKT], [$alpha_k = B_k \/ V_k$], [Exact limit orders and budget absorption],
    [Project to demand], [@prop-ra-value and $W^"RA"(bold(D))$], [Concave demand-space objective],
    [Dualize prices], [@prop-ra-dual], [Unique clearing prices for every $b > 0$],
    [Lift to cones], [Exponential-cone reformulation], [Polynomial-time solvability],
  )
]

The next auxiliary proposition is used only in part (4) of the proof, where the reduced-form clearing program is projected into demand space and dualized over prices.

#proposition(name: "Demand-Space Concavity")[
  _Define the feasible demand polytope_
  $ cal(D) = {bold(D) : exists bold(q) " with " bold(q) in cal(C), bold(D)(bold(q)) = bold(D)} $
  _and the inner value function_
  $ W^"RA"(bold(D)) = max_(bold(q) : bold(q) in cal(C), bold(D)(bold(q)) = bold(D)) { sum_k psi_(B_k)(U_k(bold(q))) + sum_(j in.not "MM") w_j q_j } $
  _for $bold(D) in cal(D)$, extended by $-infinity$ outside $cal(D)$. Then $W^"RA"$ is concave._
] <prop-ra-value>

_Proof._ Let $bold(D)^1, bold(D)^2 in cal(D)$ and let $bold(q)^1$ and $bold(q)^2$ attain the maxima defining $W^"RA"(bold(D)^1)$ and $W^"RA"(bold(D)^2)$. For any $theta in [0,1]$, convexity of $cal(C)$ gives $bold(q)^theta = theta bold(q)^1 + (1-theta) bold(q)^2 in cal(C)$. Linearity of $bold(D)$ and $bold(U)$ gives
$
bold(D)(bold(q)^theta) = theta bold(D)^1 + (1-theta) bold(D)^2, quad bold(U)(bold(q)^theta) = theta bold(U)(bold(q)^1) + (1-theta) bold(U)(bold(q)^2)
$
so $bold(q)^theta$ is feasible for $theta bold(D)^1 + (1-theta) bold(D)^2$. The inner objective
$
G(bold(q)) = sum_k psi_(B_k)(U_k(bold(q))) + sum_(j in.not "MM") w_j q_j
$
is concave in $bold(q)$, so
$
W^"RA"(theta bold(D)^1 + (1-theta) bold(D)^2) >= G(bold(q)^theta) >= theta G(bold(q)^1) + (1-theta) G(bold(q)^2)
$
which equals $theta W^"RA"(bold(D)^1) + (1-theta) W^"RA"(bold(D)^2)$. #h(1fr) $square$

== Proof

*(1) Concavity, existence, and lift.* By @prop-reduced, each $psi_(B_k)$ is concave. Since $U_k$ and $bold(D)$ are linear in $bold(q)$, the reduced-form objective of @thm-main is concave on the compact polytope $cal(C)$, so an optimum exists by Weierstrass. The deployed-value lift follows from @prop-vlift, and at any optimum the lifted variable is $V_k^* = max(U_k, B_k)$ with
$
alpha_k = B_k \/ V_k^* = psi_(B_k)'(U_k) <= 1
$

*(2) Limit order exactness and two regimes.* Work in the equivalent deployed-value lift of @prop-vlift. Let $eta_k >= 0$ be the multiplier for the constraint $U_k(bold(q)) - V_k <= 0$. Stationarity with respect to $V_k$ gives
$
B_k \/ V_k - 1 + eta_k = 0
$
so $eta_k = 1 - alpha_k$. The KKT condition for fill $q_i$ of MM $k$ is then
$
alpha_k L_i - p_(m(i)) = lambda_i^+ - lambda_i^-
$
because the objective contributes $+L_i$ through $U_k$, while the active-value constraint subtracts $eta_k L_i$. A fill ($q_i > 0$) requires $alpha_k L_i >= p_(m(i))$, hence $L_i >= p_(m(i)) \/ alpha_k >= p_(m(i))$: the limit price must exceed the clearing price. No negative-welfare fill is possible.

The two regimes are immediate from $V_k^* = max(U_k, B_k)$. If $U_k > B_k$, then $V_k = U_k$, so $s_k = 0$ and $alpha_k = B_k \/ U_k < 1$. If $U_k < B_k$, then $V_k = B_k$, so $s_k = B_k - U_k > 0$ and $alpha_k = 1$. In the over-capitalized regime, the fill condition $L_i >= p_(m(i))$ is exactly the risk-neutral UCP condition — the MM clears identically to the LP.

*(3) Budget absorption.* Multiply the fill KKT by $q_i$ and sum over $i in "MM"_k^+$. By complementary slackness ($lambda_i^- q_i = 0$):
$
alpha_k underbrace(sum_(i in "MM"_k^+) L_i q_i, = U_k) = sum_(i in "MM"_k^+) p_(m(i)) q_i + underbrace(sum_(i in "MM"_k^+) lambda_i^+ q_i, >= 0)
$

So $sum p_(m(i)) q_i <= alpha_k U_k$. Since $alpha_k V_k = B_k$ and $s_k = V_k - U_k$, we have
$
alpha_k U_k = B_k - alpha_k (V_k - U_k) = B_k - alpha_k s_k <= B_k
$
and therefore
$
sum_(i in "MM"_k^+) p_(m(i))^* q_i^* + s_k^* <= B_k
$

Each MM's total deployment — capital on fills plus retained cash — is at most $B_k$. (The gap $sum lambda_i^+ q_i$ is infra-marginal surplus from fully-filled orders — profit that does not require additional capital.) The budget emerges from the objective, not from an explicit constraint. This is the Eisenberg-Gale mechanism extended to quasi-linear utilities: the $ln$ singularity absorbs the budget, and the retained-cash variable absorbs the surplus.

*(4) Price extraction and uniqueness for $b > 0$.* For $b > 0$, prices are the gradient of the smoothed minting cost:
$
p_k = partial C_b \/ partial D_k = "softmax"(bold(D)\/b)
$
By @prop-ra-value, the demand-space value function $W^"RA"$ is concave. Therefore the primal problem is
$
max_(bold(D)) [W^"RA"(bold(D)) - C_b(bold(D))]
$
and Fenchel-Rockafellar duality gives the price problem
$
min_(bold(p) in Delta) [W_"RA"^*(bold(p)) + C_b^*(bold(p))]
$
where $W_"RA"^*$ is convex and $C_b^*(bold(p)) = b sum_k p_k ln p_k$ is strictly convex on $Delta$ by @thm-lmsr. Hence the dual objective is strictly convex on the simplex, so the minimizing price vector is unique. At $b = 0$, prices are the LP dual variables of the epigraph constraints $M >= D_k$, which may be non-unique when multiple outcomes tie at $max_j D_j$.

*(5) Polynomial solvability.* At $b = 0$, @prop-conic gives an exponential-cone formulation of $P_0^"RA"$, so standard interior-point solvers apply in polynomial time. For $b > 0$, the smoothed minting cost likewise admits a conic lift with $O(K)$ additional exponential cones (see Conic Formulation below), or can be handled directly by smooth convex optimization. #h(1fr) $square$

== Price Dual and Temperature

#proposition(name: "Risk-Averse Price Dual")[
  _Let $W^"RA"$ be the concave demand-space value function of @prop-ra-value. Then the clearing-price problem is_
  $ min_(bold(p) in Delta) [W_"RA"^*(bold(p)) + C_b^*(bold(p))] $
  _where $C_b^*(bold(p)) = b sum_k p_k ln p_k$ for $b > 0$ and $C_0^* = delta_Delta$. Equivalently, at $b = 0$ the price set is the minimizer set of $W_"RA"^*$ over the simplex. For $b > 0$, the minimizer is unique._
] <prop-ra-dual>

_Proof._ This is exactly the dual characterization from part (4) of @thm-main together with @thm-minting and @thm-lmsr. For $b > 0$, uniqueness follows from strict convexity of the entropy term on $Delta$. #h(1fr) $square$

*Example.* Binary market ($K = 2$), $b -> 0$. An MM ($B = 50$) posts two buy-Yes orders: $A$ at $L_A = 0.70$, qty $100$; $B$ at $L_B = 0.55$, qty $100$. A retail trader posts buy-No at $L_C = 0.60$, qty $200$.

_LP (no budgets)._ All fill at balanced minting ($D_"Yes" = D_"No" = 200$). Welfare $= 45$. MM capital at any supporting price exceeds $B$: even at the lowest dual price $p_"Yes" = 0.40$, capital $= 0.40 times 200 = 80 > B$.

_Risk-averse._ The MM is capital-constrained ($U^"LP" = 125 > B = 50$, $s = 0$). Only order $A$ fills: $q_A = 100$, $q_B = 0$, $q_C = 100$. The retail order is partially filled, pinning $p_"No" = L_C = 0.60$, hence $p_"Yes" = 0.40$. The capital-scarcity factor is $alpha = B\/U = 50\/70 approx 0.71$. Order $A$ fills: $alpha L_A = 0.50 > p_"Yes" = 0.40$. Order $B$ does not fill: $alpha L_B = 0.39 < 0.40$ — the capital-scarcity factor throttles the lower-ROI order. Welfare $= 30$. Capital consumed $= 0.40 times 100 = 40 <= B$, with the gap of $10$ being infra-marginal surplus from order $A$.

_Welfare gap._ Actual gap $= 15$; exact bound $= Delta - B ln(1 + Delta\/B) approx 29$ (where $Delta = U^"LP" - B = 75$). The bound is conservative because the retail side contracts proportionally.

In the risk-neutral model, entropy smoothing was the only source of concavity — insufficient against budget non-convexity (@prop-obstruction). Here, the reduced-form MM utility provides curvature at every temperature, and the deployed-value lift of @prop-vlift makes that curvature explicit. Even at $b = 0$ (pure LP minting cost), the program remains a well-posed concave optimization problem:

$ P_0^"RA": quad max_(bold(q) in cal(C), bold(V) >= bold(U)(bold(q))) quad sum_k [B_k ln V_k - V_k + U_k(bold(q))] + sum_(j in.not "MM") w_j q_j - max_k D_k $

The temperature dependence is therefore:

#align(center)[
  #table(
    columns: 3,
    align: (left, center, center),
    stroke: none,
    [*Property*], [*$b > 0$*], [*$b = 0$*],
    [Concave formulation and existence], [Yes], [Yes],
    [Reduced-form utility / budget absorption], [Yes], [Yes],
    [Exact limit-order KKT conditions], [Yes], [Yes],
    [Clearing prices], [Unique softmax], [Simplex-valued LP duals; may be non-unique],
    [Conic solvability], [Yes], [Yes],
  )
]

The $-max_k D_k$ term is concave (not strictly), and the log term continues to regularize capital deployment. So existence, limit-order exactness, budget absorption, and conic solvability all survive at $b = 0$. What fails at zero temperature is uniqueness: the entropy term disappears from @prop-ra-dual, prices become LP-type dual variables, and multiple outcomes can tie at $D_k = max_j D_j$. Any $b > 0$ restores a strictly convex price dual, with subsidy bounded by $b ln K$ (@prop-sandwich).

== Consequences of the Affine Envelope

The reduced-form utility of @prop-reduced is exactly an affine LP objective minus a non-negative penalty on the capital-binding region. Define the affine-envelope gap
$
g_B(U) = U + B ln B - B - psi_B(U) = cases(
  0 & "if" U <= B,
  U - B - B ln(U\/B) & "if" U >= B
)
$
Then $g_B(U) >= 0$ for all $U$, with equality exactly on the slack-budget region. For any feasible fill vector,
$
sum_k psi_(B_k)(U_k(bold(q))) = sum_k [U_k(bold(q)) + B_k ln B_k - B_k] - sum_k g_(B_k)(U_k(bold(q)))
$
So, up to the constant $sum_k (B_k ln B_k - B_k)$, the reduced-form clearing objective is the LP objective minus the envelope gap.

#proposition(name: "LP Recovery")[
  _Let $bold(q)^"LP"$ be any unconstrained LP optimum ($P_b$ without budgets). If $B_k >= U_k^"LP" = sum_(i in "MM"_k^+) L_i q_i^"LP"$ for every MM $k$, then $bold(q)^"LP"$ is optimal for the reduced-form risk-averse program. In the deployed-value lift, the corresponding retained cash is $s_k = B_k - U_k^"LP"$. In particular, the LP and risk-averse programs share an optimum whenever all MM budgets are non-binding._
] <prop-welfare>

_Proof._ If $B_k >= U_k^"LP"$ for all $k$, then $g_(B_k)(U_k^"LP") = 0$ for every MM. So at $bold(q)^"LP"$ the reduced-form objective equals the LP objective plus the constant $sum_k (B_k ln B_k - B_k)$. For any other feasible $bold(q)$, the reduced-form objective is that same LP objective minus the non-negative penalty $sum_k g_(B_k)(U_k(bold(q)))$. Since $bold(q)^"LP"$ maximizes the LP objective, it also maximizes the reduced-form objective. The corresponding deployed values are $V_k = B_k$, equivalently $s_k = B_k - U_k^"LP"$. #h(1fr) $square$

The retained-cash variable $s_k$ acts as a _numeraire good_, eliminating the over-fill pathology of the naive $B_k ln U_k$ model (which forces all budget into fills, including unprofitable ones). The quasi-linear structure is essential: without $s_k$, over-capitalized MMs would distort fills to exhaust their budget.

#proposition(name: "Welfare Gap Bound")[
  _Let $W^"LP"$ and $W^"RA"$ denote the total welfare (LP objective value) at the LP and risk-averse optima respectively, and let $Delta_k = max(0, U_k^"LP" - B_k)$ be MM $k$'s budget shortfall. Then:_

  $ W^"LP" - W^"RA" <= sum_k [Delta_k - B_k ln(1 + Delta_k / B_k)] <= sum_k Delta_k^2 / (2 B_k) $

  _Over-capitalized MMs ($Delta_k = 0$) contribute nothing. The bound is quadratic in the shortfall._
] <prop-gap>

_Proof._ Write $F(bold(q))$ for the LP welfare objective (the unconstrained batch-clearing objective with the same minting cost $C_b$). By the envelope decomposition above, the reduced-form objective is
$
F(bold(q)) + sum_k (B_k ln B_k - B_k) - sum_k g_(B_k)(U_k(bold(q)))
$
Since $bold(q)^"RA"$ maximizes this penalized objective,
$
F(bold(q)^"RA") - sum_k g_(B_k)(U_k(bold(q)^"RA")) >= F(bold(q)^"LP") - sum_k g_(B_k)(U_k^"LP")
$
Rearranging and using $g_B >= 0$ gives
$
W^"LP" - W^"RA" <= sum_k g_(B_k)(U_k^"LP")
$
If $U_k^"LP" = B_k + Delta_k$, then
$
g_(B_k)(U_k^"LP") = Delta_k - B_k ln(1 + Delta_k / B_k)
$
while over-capitalized MMs have $Delta_k = 0$ and contribute zero. The quadratic bound follows from $ln(1 + x) >= x - x^2 / 2$. #h(1fr) $square$

The bound is tight when all orders are marginal ($L_i approx p_k$) and conservative for real order ladders, where spread-out limit prices give $V_k^"RA" >> B_k$. The exact expression grows sublinearly in $Delta_k$: the throttled fills are precisely the least profitable ones.

== Fisher Interpretation

In a Fisher market (Eisenberg & Gale 1959), $n$ consumers with budgets $B_k$ purchase divisible goods with fixed supply $s_j$ at prices $p_j$. A market equilibrium can be characterized as a solution to:

$ "EG": quad max_(bold(x) >= 0) quad sum_k B_k ln U_k (bold(x)_k) quad "s.t." quad sum_k x_(k j) <= s_j quad forall j $

where $U_k(bold(x)_k) = sum_j u_(k j) x_(k j)$ is consumer $k$'s linear utility over goods. The supply constraints have dual variables $p_j$ — the equilibrium prices. Budget constraints do not appear explicitly; they emerge from the $B_k ln U_k$ objective by the same telescoping argument as our proof of (3). Adding a cash variable $s_k$ with cost $-s_k$ yields the _quasi-linear_ Fisher market (Cole et al. 2017, Chen et al. 2009) — agents can retain unspent budget as cash rather than being forced to spend it all on goods. Quasi-linear Fisher markets are well-studied: equilibrium existence and polynomial-time computation are known under mild conditions, while uniqueness typically concerns prices or utility profiles under additional regularity.

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

*What changes.* Under $P_b^"RA"$, capital-constrained MMs ($alpha_k < 1$) prioritize their highest-ROI fills rather than filling every profitable order equally. An MM concentrating capital on one market faces the $ln$ penalty: the first dollar of fill generates high marginal utility, additional dollars generate diminishing utility. This naturally diversifies MM capital across markets. Over-capitalized MMs ($alpha_k = 1$) behave identically to the LP — the log model only affects sizing when the budget actually binds.

*What doesn't change.* Non-MM orders are still matched by UCP at clearing prices. The minting mechanism is unchanged. Prices are still softmax (for $b > 0$) or LP duals (for $b = 0$). Limit orders are exact: no order fills below its stated price.

== Buy/Sell Reduction

In each binary market, MM positions are netted per outcome before clearing: opposing buys and sells cancel, leaving a net directional position. Since "sell Yes at $L$" is economically equivalent to "buy No at $1-L$" (with capital cost $1-L$ per share), all net positions are expressed as buys. These net buy orders form $"MM"_k^+$ and their capital costs enter the budget accounting through $U_k$. The only MM sell orders that bypass the budget are liquidations of existing inventory — shares already held, not newly created short positions. Per outcome per batch, the MM is either net buying or net selling, never both.

== Multiple Groups and Bundle Orders

Nothing in @thm-main requires a single mutually exclusive group. Consider $N$ groups, each with $K_j$ mutually exclusive outcomes. The joint state space is $cal(S) = product_j {1, dots, K_j}$ (exactly one joint state is realized). Bundle orders — orders whose payoffs depend on the joint state — create demand $D_s$ over $cal(S)$. The minting cost generalizes to $V(bold(D)) = max_s D_s$ (minting one complete set of all joint states costs \$1), and the smoothed version is $C_b = b ln sum_s exp(D_s\/b)$. Both are convex in $bold(D)$, and $bold(D)$ is linear in $bold(q)$. The proof of @thm-main goes through unchanged for any fixed state space representation: each MM term $B_k ln V_k - V_k + U_k$ remains concave, $-C_b$ remains concave, and budget absorption and limit order exactness still hold. The open question is whether the program can be solved without explicitly enumerating the joint states (see Open Problems).

When no orders span multiple groups, the joint problem decomposes: $C_b = sum_j C_b^(G_j)$ (the product-LMSR factorization of Chen and Pennock 2007). Cross-group orders break this separability, coupling groups transitively — if orders link groups $A$–$B$ and $B$–$C$, the clearing problem requires the full $K_A times K_B times K_C$ state space. The mathematical obstruction is purely computational, not structural; the companion decomposition note proves exact componentwise decomposition when the coupling graph splits and analyzes mirror-descent coordination of shared MM budgets.


= Computation <computation>

== Conic Formulation

At $b = 0$, the minting cost $max_k D_k$ is modeled by LP epigraph constraints ($M >= D_k$). In the deployed-value lift of @prop-vlift, all nonlinearity is then isolated in the terms $ln V_k$: after introducing epigraph variables $t_k <= ln(V_k)$, the remaining objective and constraints are linear. Each constraint $t_k <= ln(V_k)$ is modeled by a single exponential cone: $(t_k, 1, V_k) in cal(K)_"exp" = {(x,y,z) : y exp(x\/y) <= z}$. The full program is a conic optimization problem solvable by standard interior-point solvers (Clarabel, MOSEK).

#proposition(name: "Conic Reformulation")[
  _At $b = 0$, $P_0^"RA"$ is equivalent to a conic program: minimize a linear objective over LP constraints and $n_"MM"$ exponential cone constraints (one per MM). Clearing prices are the dual variables of the minting epigraph constraints. For $b > 0$, the log-sum-exp minting cost adds $O(K)$ additional exponential cones (one per outcome); the augmented-LP approach below avoids this overhead._
] <prop-conic>

_Remark (Augmented LP)._ The EG Hessian $H = -sum_k (B_k \/ V_k^2) bold(ell)_k bold(ell)_k^T$ has rank at most $n_"MM"$ (the number of MMs), where $bold(ell)_k$ is MM $k$'s welfare weight vector. By Sherman–Morrison–Woodbury, each Newton step costs the same as an LP interior-point step plus an $O(n dot n_"MM")$ rank-$n_"MM"$ correction — dominated by the $O(n)$ LP cost when $n_"MM" << n$. The EG problem has the same asymptotic complexity as the LP.

== Decomposition for Exponential State Spaces

When no orders span multiple market groups, the minting cost separates: $C_b = sum_j C_b^(G_j)$ (product-LMSR factorization). This suggests a decomposition of the EG program into independent per-group subproblems, coordinated by MM budget allocation.

With log utility, splitting an MM's budget $B_k$ across components $m$ leads to a smooth concave coordination problem: the coordination objective $max sum_m W_m^* (bold(B)^m)$ subject to $sum_m B_k^m = B_k$ has gradient $partial W_m^* \/ partial B_k^m = ln V_k^(m *)$ (envelope theorem), where $V_k^m = U_k^m + s_k^m$. The first-order condition equalizes these component values across active components. Standard mirror descent (multiplicative weights on budget shares) is then a natural algorithmic candidate, with each iteration requiring only independent per-component solves. A companion note develops this decomposition in detail.

With linear welfare, the same budget allocation is combinatorial: a hard constraint $"cap"_k <= B_k$ cannot be smoothly split across components. This is the computational payoff of the Fisher market structure — the same log-utility mechanism that absorbs budget constraints in the monolithic program also enables decomposition across components.

_Validation._ An implementation of the LP, conic, and EG solvers confirms the theoretical predictions on synthetic order books with $50$ market groups, $~11,000$ single-market orders, and $3$ budget-constrained MMs. The quasi-Fisher (conic) and linear (LP) solvers produce near-identical welfare (gap $< 0.7%$ across all budget scales), consistent with @prop-welfare and @prop-gap. When budgets are non-binding, the programs share an optimum. Decomposition with mirror descent on budget shares converges toward the monolithic solution on single-market order books, as expected from the product-LMSR factorization.

Current timing benchmarks should be read in regime context. On independent binary markets, monolithic LP clearing is already cheap because the state space is small, so decomposed solving pays the overhead of repeated coordination iterations without gaining asymptotic advantage. In those tests, decomposed LP nearly matches monolithic welfare but is somewhat slower wall-clock, while decomposed conic solving is currently limited by numerical conditioning when some component budgets become very small. The decomposition architecture is aimed at the opposite regime: bundle orders and coupled market groups, where monolithic enumeration becomes exponential and coordination overhead is the cheaper problem.

Concrete regime picture: if the bundle-coupling graph on $20$ binary groups splits into four connected components of five groups each, monolithic joint-state clearing sees $2^20 approx 10^6$ states, while componentwise clearing sees only $4 dot 2^5 = 128$ states plus a smooth budget-coordination layer. That is the setting decomposition is for. The current benchmarks do not yet probe that regime.


= Discussion <discussion>

== Main Contributions

+ *Reduced-form MM utility.* The paper identifies the exact MM utility induced by budget plus retained cash: affine when capital is slack and logarithmic when capital binds (@prop-reduced). The deployed-value lift @prop-vlift is the exact derivation and solver form behind this reduced form.

+ *Observable market outcomes.* The risk-averse program has exact limit-order KKT conditions, endogenous budget absorption, and an explicit price dual @prop-ra-dual. For every $b > 0$, the dual is strictly convex, so clearing prices are unique; at $b = 0$, the remaining ambiguity is the familiar LP dual degeneracy.

+ *Algorithmic consequence.* The formulation admits exponential-cone representations and an augmented-LP interpretation (@prop-conic). Decomposition is most useful in the coupled multi-group regime where the joint state space is exponential, not in the small independent-market benchmarks where monolithic LP is already cheap.

== Open Problems

+ *Welfare gap tightness.* The quadratic bound of @prop-gap is conservative for typical order books: synthetic experiments show gaps under $0.7%$ even at $10 times$ budget oversubscription. Tighter instance-dependent bounds — exploiting the structure of real MM ladder orders — would sharpen the practical guarantees.

+ *Efficient combinatorial clearing.* The Fisher market isomorphism extends to joint state spaces (see Multiple Groups and Bundle Orders), but the state space $|cal(S)| = product K_j$ is exponentially large. Cross-group bundle orders couple groups transitively: if orders span $A$–$B$ and $B$–$C$, clearing requires the full $K_A times K_B times K_C$ space. In practice, a handful of bundle orders can connect all groups. Each order's payoff is a low-rank tensor (touching $<= 5$ groups), so the demand $D_s$ has exploitable structure. Can the EG program be solved in time polynomial in the number of orders rather than the state space?

+ *Risk-averse observables at $b = 0$.* The reduced-form program and its deployed-value lift prove existence, limit-order exactness, and budget absorption at every temperature, and unique clearing prices for every $b > 0$. What remains open is the zero-temperature edge case: under what additional regularity are prices, aggregate demands, or projected fills unique when the minting cost is $max_k D_k$ and the LP dual is set-valued?

== Connection to Prior Work

*Eisenberg and Gale (1959)*: The convex program for Fisher market equilibrium. Our @thm-main extends their framework to prediction markets with endogenous supply (minting) and mixed agent types (log-utility MMs alongside linear-welfare retail), with reduced-form utility @prop-reduced and deployed value $V_k$ as the bridge variables.

*Abernethy, Chen, and Vaughan (2013)*: Axiomatic foundation for cost-function market makers. Our §2 recasts their framework as batch-auction clearing: $V = max_k D_k$ is the simplest cost function, LMSR its entropy smoothing.

*Breiman (1961)*: Optimality of the Kelly criterion for repeated games. §4 uses this as a repeated-game motivation for diminishing returns, not as a literal behavioral theorem about every MM.

*Devanur and Dudík (2015)*: Budget constraints and additivity for sequential LMSR. Their budget additivity hints at hidden convexity in the risk-neutral setting. Our contribution is a concave EG-type formulation for batch auctions; whether risk-neutral batches also have hidden convexity remains open.

*Fortnow, Kilian, Pennock, and Wellman (2005)*: LP for combinatorial call markets. Our group minting encodes mutual exclusivity in $O(K)$ constraints vs $O(2^K)$ enumeration.

*Chen and Pennock (2007)*: Bounded-loss market makers and the product-LMSR factorization. The parameter $b$ is their loss bound; $b = 0$ is achievable in batch clearing by complementary slackness.

*Cole et al. (2017), Chen et al. (2009)*: Quasi-linear Fisher markets. Our $P_b^"RA"$ is structurally a quasi-linear Fisher market; the contribution is the deployed-value / reduced-form-utility view for prediction markets with endogenous supply and LMSR pricing.

*Budish, Cramton, and Shim (2015)*: Frequent Batch Auctions for equity markets. Our framework applies FBAs to prediction markets.


#v(2em)
#line(length: 100%)
#v(0.5em)
#text(size: 9pt, style: "italic")[
  Implementation and experimental code available at the project repository. All solvers (LP, EG, conic) and the decomposition framework are implemented in Rust.
]
