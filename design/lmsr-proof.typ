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
  We prove that prediction market batch auctions with budget-constrained market makers are Fisher markets. The main result (Theorem 5): replacing linear market-maker welfare with logarithmic utility — reflecting the Kelly-criterion sizing that real institutional MMs use — transforms the NP-hard budget-constrained clearing problem into a convex program with a unique solution, solvable in polynomial time. Budget constraints vanish from the formulation entirely, absorbed into the Eisenberg-Gale objective. Prices, fills, and budget allocation emerge simultaneously from a single convex optimization.

  The paper develops in three acts. First, we establish that LP batch clearing and Hanson's LMSR are endpoints of a single parametric family connected by Fenchel duality (§2). Second, we prove that adding budget constraints to the risk-neutral model creates a Generalized Nash Equilibrium Problem that is provably non-convex (§3). Third, we show that correcting the economic model — from risk-neutral to risk-averse MMs — resolves the non-convexity completely (§§4–5). The modeling "trick" is not logarithmic utility; it is the linear assumption. When we fix it, the pathology fixes itself.
]

#v(1em)

= Introduction

Prediction market clearing is, at its core, an allocation problem: given a set of orders across multiple markets, find the fills and prices that maximize welfare subject to balance constraints and budget limits. Without budget constraints, this is a Linear Program — clean, fast, and well-understood. The sole source of difficulty is the _bilinear_ budget constraint: the capital consumed by a fill depends on the clearing price, and the clearing price depends on which fills happen.

This paper makes three contributions:

+ *LMSR–LP unification (§2).* We prove that Hanson's Logarithmic Market Scoring Rule and the LP batch auction are the same mathematical object at different temperatures, connected by Fenchel conjugation. The LP's minting cost $V(D) = max_k D_k$ smooths to the LMSR cost $C_b(D) = b ln sum exp(D_k\/b)$. Their conjugates — the simplex indicator and negative entropy — form a commutative duality diagram. This is a clean packaging of known results (Abernethy et al.~2013) in the batch auction setting.

+ *Impossibility for risk-neutral MMs (§3).* We prove that the budget-constrained clearing problem with linear welfare is a Generalized Nash Equilibrium Problem: the feasible set depends on the solution. Unconditional uniqueness of clearing prices is provably impossible (Proposition 3). The entropy curvature and the budget non-convexity both scale as $O(1\/b)$, creating an isospectral deadlock that no reformulation can break.

+ *Fisher market isomorphism (§§4–5).* We prove that replacing linear MM welfare with Kelly-criterion utility ($B_k ln U_k$) transforms the problem into a strictly convex Eisenberg-Gale program (Theorem 5). Budget constraints are _absorbed into the objective_ — they do not appear as constraints at all. The program has a unique optimum, unique clearing prices, and is polynomial-time solvable. This is not a mathematical convenience but an economic correction: we argue that logarithmic utility is the _right_ model for repeat-participation market makers, and that linear utility with hard budget caps is the approximation (§4).


= Batch Auction Clearing and LMSR <foundation>

This section establishes the technical foundation. The results are largely known (in various forms) but the specific framing through batch auction minting is new.

== Minting Cost and the LP

Consider a prediction market with $K$ mutually exclusive outcomes. In a batch auction, $N$ orders arrive. The _net demand_ — excess buy over sell volume — for each outcome is:

$
D_k = sum_(i in "buy"(k)) q_i - sum_(j in "sell"(k)) q_j
$

To clear the market, the exchange _mints_ complete sets: one share of every outcome at cost \$1 (exactly one resolves to \$1 at settlement). The minimum minting cost is $V(bold(D)) = max_k D_k$ — we need at least $max_k D_k$ mints to cover the highest-demand outcome. The welfare-maximizing clearing solves:

$ P: quad max_(bold(q) in [0, bold(overline(Q))]) quad sum_i w_i q_i - max_k D_k (bold(q)) $

This is an LP (introduce minting variable $mu >= D_k$ for each $k$). Its dual variables are clearing prices.

== Fenchel Duality: Prices from Conjugates

The structural properties of clearing prices follow from the Fenchel conjugate of the minting cost.

#block(inset: (left: 1em))[
  *Theorem 1* (Minting–Simplex Duality). _The Fenchel conjugate of $V(bold(D)) = max_k D_k$ is the indicator function of the probability simplex:_
  $ V^* (bold(p)) = delta_Delta (bold(p)) = cases(0 & "if" bold(p) in Delta, +infinity & "otherwise") $
  _where $Delta = {bold(p) >= 0 : sum_k p_k = 1}$._
]

_Proof._ $V^*(bold(p)) = sup_(bold(D)) {sum p_k D_k - max_k D_k}$. If $bold(p) in Delta$: convex combination $<=$ max, so sup $= 0$ (attained at $bold(D) = bold(0)$). If $sum p_k > 1$: take $bold(D) = (t, dots, t)$, get $t(sum p_k - 1) -> infinity$. If some $p_j < 0$: take $D_j -> -infinity$, others $= 0$. #h(1fr) $square$

*Interpretation.* Minting at \$1 per complete set _is_ the probability simplex. The axiom $sum p_k = 1$ is not imposed — it is the conjugate of the minting cost.

== Entropy Smoothing: LMSR as Soft LP

The LMSR cost function $C_b(bold(D)) = b ln sum exp(D_k\/b)$ is the smooth ($C^infinity$) approximation of $V$:

#block(inset: (left: 1em))[
  *Proposition 1* (LSE–Max Sandwich). $max_k D_k <= C_b(bold(D)) <= max_k D_k + b ln K$. The gap $b ln K$ is the maximum LMSR subsidy.
]

#block(inset: (left: 1em))[
  *Theorem 2* (LMSR–Entropy Duality). _The Fenchel conjugate of $C_b$ is negative Shannon entropy on the simplex: $C_b^*(bold(p)) = b sum p_k ln p_k$ for $bold(p) in Delta$, $+infinity$ otherwise._
]

_Proof._ Setting $nabla C_b = bold(p)$ gives the softmax $p_k = exp(D_k\/b) \/ sum exp(D_j\/b)$. Substituting back: $C_b^* = sum p_k D_k - C_b = b sum p_k ln p_k$. #h(1fr) $square$

The duality diagram:

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

As $b -> 0$: LMSR sharpens to LP, entropy penalty hardens to simplex indicator.

== LMSR Pricing and the Smoothed Batch Auction

Replacing $V$ with $C_b$ in the batch clearing:

$ P_b: quad max_(bold(q) in [0, bold(overline(Q))]) quad sum_i w_i q_i - C_b(bold(D)(bold(q))) $

#block(inset: (left: 1em))[
  *Theorem 3* (LMSR = Smoothed Batch Clearing). _At the optimum of $P_b$, clearing prices are $p_k^* = exp(D_k^*\/b) \/ sum exp(D_j^*\/b)$ — the LMSR marginal price. Furthermore: (1) $sum p_k = 1$; (2) as $b -> 0$, solutions converge to the LP optimum; (3) as $b -> infinity$, prices converge to uniform._
]

The KKT conditions give the standard Uniform Clearing Price rule: orders fill when their limit price exceeds the clearing price, regardless of $b$. The exponentials live in the _minting cost_, not the order book.

== Self-Financing and Scaling

#block(inset: (left: 1em))[
  *Theorem 4* (Self-Financing Minting). _In the LP ($b = 0$), the minting mechanism breaks even exactly: $"P&L" = sum p_k D_k - max_k D_k = 0$._
]

_Proof._ By complementary slackness, $p_k > 0$ only where $D_k = mu = max_j D_j$. So $sum p_k D_k = mu sum_(D_k = mu) p_k = mu$. #h(1fr) $square$

For $K$ mutually exclusive outcomes, the LP uses $O(K)$ balance constraints and one group mint variable. Combinatorial LMSR requires $O(2^K)$ state evaluations. The structural insight: mutual exclusivity collapses the joint state space from $2^K$ to $K$ states. Group minting exploits this directly.


= The Budget Obstacle <obstacle>

The machinery of §2 handles the LP core exactly. We now confront the _sole_ remaining difficulty: market maker budget constraints.

== The Bilinear Constraint

A market maker $k$ deposits balance $B_k$ and posts orders across multiple markets. The capital consumed by each fill depends on the clearing price:

$
"cap"_k(bold(p), bold(q)) = sum_(i in "MM"_k) c_i(p_(m(i))) dot q_i, quad c_i(p) = cases(p & "if BuyYes/SellNo", 1-p & "if SellYes/BuyNo")
$

The budget constraint $"cap"_k <= B_k$ is _bilinear_: $p$ is determined by $bold(q)$ through the clearing mechanism. The product $c(p(bold(q))) dot q$ makes the feasible set non-convex. This single constraint is what separates prediction market clearing from a standard LP.

== Unconditional Uniqueness Is Impossible

One might hope that despite the non-convexity, clearing prices are still unique (even if fills are not). We prove this is false.

#block(inset: (left: 1em))[
  *Proposition 3* (Cross-Price Obstruction). _For the risk-neutral budget-constrained problem, unconditional price uniqueness is impossible. The standard monotonicity argument fails due to cross-price budget violation._
]

_Proof._ Suppose two KKT points $(bold(q)^1, bold(p)^1, bold(mu)^1)$ and $(bold(q)^2, bold(p)^2, bold(mu)^2)$ exist with $bold(p)^1 != bold(p)^2$. The strict convexity of $C_b^*$ (Theorem 2) gives $chevron.l bold(D)^1 - bold(D)^2, bold(p)^1 - bold(p)^2 chevron.r > 0$. For a contradiction, we need the first fills to be budget-feasible at the second prices: $E_k(bold(q)^1, bold(p)^2) <= B_k$. But while $E_k(bold(q)^1, bold(p)^1) <= B_k$ holds, there is *no guarantee* that $E_k(bold(q)^1, bold(p)^2) <= B_k$. If $bold(p)^2$ is higher where MM $k$ is long, the cross-price evaluation exceeds the budget.

This is the signature of a _Generalized Nash Equilibrium Problem_ (GNEP): the feasible set depends on the dual variable. Both the entropy curvature and the cross-price violation scale as $O(1\/b)$, preventing either from dominating. #h(1fr) $square$

*Counterexample.* Two markets $A, B$ with identical order books and one MM with symmetric positions on both. By symmetry, there exist two KKT points: "fill $A$, starve $B$" and "fill $B$, starve $A$," producing different fills, demands, and prices. This requires exact parameter symmetry — a measure-zero set — but it proves that no purely mathematical trick can salvage unconditional uniqueness for the risk-neutral model.

*The structural diagnosis.* The problem is _not_ that we haven't found the right proof technique. The problem is that _linear welfare with hard budget caps_ is an internally inconsistent economic model (§4), and the mathematical pathology is a symptom of the modeling error.


= Why Market Makers Have Diminishing Returns <economic-case>

The standard assumption in prediction market theory is that market makers have _linear_ welfare: each additional share purchased contributes the same marginal value. We argue this is the unrealistic approximation, and that logarithmic (Kelly-criterion) utility is the correct model. Six independent arguments converge.

== Kelly Criterion Is a Survival Theorem

Breiman (1961) proved that among all repeated-game investment strategies, Kelly maximization (log utility) is the _unique_ strategy that: (1) maximizes long-run growth rate almost surely; (2) reaches any wealth target in minimum expected time; (3) dominates any other strategy in the long run with probability 1.

Any MM using a non-Kelly strategy — including linear utility — goes bankrupt with probability 1 given enough batches. This is not a preference; it is a theorem. An MM that sizes linearly (constant marginal value per share) will eventually encounter a loss sequence that wipes it out. An MM that sizes logarithmically will not.

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


= Risk-Averse Batch Clearing <risk-averse>

We now state and prove the main result: replacing linear MM welfare with Kelly-criterion utility transforms the budget-constrained clearing problem into a convex program.

#block(inset: (left: 1em))[
  *Theorem 5* (Risk-Averse Clearing Is Convex). _Define the risk-averse batch auction clearing program:_

  $ P_b^"RA": quad max_(bold(q) in cal(C)) quad underbrace(sum_k B_k ln U_k(bold(q)), "MM welfare") + underbrace(sum_(j in.not "MM") w_j q_j, "retail welfare") - underbrace(C_b(bold(D)(bold(q))), "minting cost") $

  _where $U_k(bold(q)) = sum_(i in "MM"_k) w_i q_i$ is MM $k$'s total weighted fill, $cal(C)$ is the LP feasible set (balance constraints, box bounds, minting), and $C_b$ is the smoothed minting cost. Then:_

  + _The objective is strictly concave on the feasible set._
  + _$P_b^"RA"$ has a unique optimal fill vector $bold(q)^*$._
  + _No budget constraints appear. At the optimum, each MM $k$ spends exactly $B_k$._
  + _Clearing prices $bold(p)^*$ are unique._
  + _The program is solvable in polynomial time._
]

_Proof._

*(1) Strict concavity.* Three terms:

- $B_k ln(sum_(i in "MM"_k) w_i q_i)$: composition of $ln$ (concave, increasing) with a linear function. Strictly concave when $U_k$ is non-constant on $cal(C)$.

- $sum_(j in.not "MM") w_j q_j$: linear, hence concave.

- $-C_b(bold(D)(bold(q)))$: $C_b$ is convex (log-sum-exp), $bold(D)$ is linear, so $-C_b (bold(D)(dot))$ is concave.

Sum of concave functions is concave. Strictly concave from the $ln$ term.

*(2) Uniqueness.* Strict concavity on a compact convex polytope $cal(C)$ gives a unique maximizer.

*(3) Budget absorption.* KKT for MM order $i$ of agent $k$ (interior fill):

$
(B_k w_i) / U_k = p_(m(i)) = c_i(bold(p))
$

Multiply by $q_i$, sum over $i in "MM"_k$:

$
sum_(i in "MM"_k) (B_k w_i q_i) / U_k = sum_(i in "MM"_k) c_i(bold(p)) q_i
$

Left side: $B_k / U_k dot sum w_i q_i = B_k / U_k dot U_k = B_k$. Therefore:

$ sum_(i in "MM"_k) c_i(bold(p)^*) q_i^* = B_k $

Each MM spends its _entire_ budget. The constraint is not imposed — it _emerges_ from the $B_k ln U_k$ objective. This is the Eisenberg-Gale mechanism.

*(4) Price uniqueness.* $p_k = "softmax"(bold(D)(bold(q)^*)\/b)$, a continuous function of the unique $bold(q)^*$.

*(5) Polynomial solvability.* Concave $C^infinity$ objective, polytope constraints. Interior-point methods solve this in polynomial time. #h(1fr) $square$

== The $C_b$ Interaction Is Trivial

In the risk-neutral model, the entropy smoothing ($C_b$) was the _only_ source of concavity, creating the isospectral deadlock with the budget non-convexity (Proposition 3). In the risk-averse model, $B_k ln U_k$ provides $b$-independent curvature of order $O(B_k\/U_k^2)$. The $-C_b$ term adds _more_ concavity.

Even at $b = 0$ (pure LP minting), the program remains strictly concave:

$ P_0^"RA": quad max_(bold(q) in cal(C)) quad sum_k B_k ln U_k + sum_(j in.not "MM") w_j q_j - max_k D_k $

The $ln$ provides strict concavity regardless of temperature. *Annealing is unnecessary.* No Frank-Wolfe iterations, no temperature schedules, no heuristics. One convex program, one unique solution.

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
    [Supply], [Fixed endowment], [Minting (endogenous, cost $C_b$)],
    [Utility], [$U_k = sum w_i x_(k i)$], [$U_k = sum w_i q_i$],
    [Objective], [$sum B_k ln U_k$], [$sum B_k ln U_k + "retail" - C_b$],
    [Prices], [Dual of supply constraint], [Dual of balance constraint = softmax],
  )
]

The prediction market extends Fisher markets in two ways: (1) supply is endogenous via minting, and (2) non-MM orders contribute linear welfare alongside log-utility MMs. Both preserve concavity.

== What Changes and What Doesn't

*What changes.* Under $P_b^"RA"$, MMs with larger $B_k$ have proportionally more influence on prices, but with diminishing marginal impact per market. The $ln$ naturally diversifies MM capital across markets — the first dollar of fill generates high marginal utility, the last dollar generates vanishing utility. This enforces the flash-liquidity pattern (spread capital thin) without protocol-level constraints.

*What doesn't change.* Non-MM orders are still matched by UCP at clearing prices. The minting mechanism is unchanged. Prices are still softmax (for $b > 0$) or LP duals (for $b = 0$). Volume is unaffected — the $ln$ is still increasing, so every profitable fill opportunity is taken. The only difference is in how MM fills are _sized_ relative to each other: proportional to welfare-per-dollar rather than absolute welfare.


= Discussion <discussion>

== Summary of Results

The paper establishes a hierarchy of results for budget-constrained prediction market clearing:

#align(center)[
  #table(
    columns: 4,
    align: (left, center, center, center),
    [*Result*], [*Condition*], [*What's unique*], [*Strength*],
    [Theorem 5], [Kelly utility], [Everything], [*Unconditional*],
    [Theorem A.3], [DIC holds at $b$], [Fills + prices], [Checkable certificate],
    [Prop.~A.4], [$bold(mu) = 0$], [Demands + prices], [Unconditional (slack)],
    [Theorem A.5], [Generic $theta$], [Everything], [Almost everywhere],
    [Proposition 3], [---], [---], [Impossible (risk-neutral)],
  )
]

The landscape is now complete: unconditional uniqueness is impossible for risk-neutral MMs (Proposition 3, §3) and unconditional uniqueness holds for risk-averse MMs (Theorem 5, §5). The risk-neutral results (Appendix A) provide partial fixes — checkable certificates, generic guarantees, heuristic algorithms — for those who need the exact linear-welfare model.

== Open Problems

+ *Risk-averse clearing in practice.* How much welfare does $P_b^"RA"$ sacrifice relative to the risk-neutral LP? When $B_k >> U_k$ (large budgets relative to fills), $ln U_k approx U_k\/U_k^0$ is nearly linear and the gap should vanish. Formalizing this convergence would quantify when risk-averse clearing is a practical drop-in replacement.

+ *Extension to non-exclusive groups.* When markets are correlated but not mutually exclusive, group minting doesn't apply directly. The exact connection to combinatorial LMSR in this regime is open.

+ *Risk-neutral hidden convexity.* Is there a reformulation of the risk-neutral model that recovers convexity? Proposition 3 shows it cannot come from the standard Fenchel dual. Devanur and Dudík (2015) proved budget additivity for sequential LMSR, hinting at hidden convexity — but extending this to simultaneous batch auctions remains open.

== Connection to Prior Work

*Eisenberg and Gale (1959)*: Convex program for Fisher market equilibrium. Our Theorem 5 establishes that prediction markets with risk-averse MMs are isomorphic to Fisher markets with endogenous supply. Proposition 3 identifies why the same approach fails for risk-neutral MMs.

*Abernethy, Chen, and Vaughan (2013)*: Cost-function market makers and convex conjugate duality. Our §2 is the batch-auction formulation of their framework. The minting cost $V = max_k D_k$ is the "simplest" cost function, with LMSR as its entropy smoothing.

*Breiman (1961)*: Optimal properties of the Kelly criterion. Our §4 uses Breiman's survival theorem to argue that log utility is not a convenience but a consequence of evolutionary selection among repeat-participation MMs.

*Devanur and Dudík (2015)*: Price uniqueness and budget additivity for sequential LMSR via Bregman divergence. Our Theorem 5 achieves unconditional uniqueness for batch auctions via the same Eisenberg-Gale structure underlying their budget additivity.

*Fortnow, Kilian, Pennock, and Wellman (2005)*: LP for combinatorial call markets. Our group minting provides the structural reason why LP works: it encodes mutual exclusivity in $O(K)$ vs $O(2^K)$.

*Chen and Pennock (2007)*: Bounded-loss market makers. The parameter $b$ in our framework is their loss bound. $b = 0$ (zero loss) is achievable in the batch setting (Theorem 4).

*Hofbauer and Sandholm (2007)*: Unique logit equilibrium for negative semidefinite games. Our prediction market is NSD (demand raises price). Their result covers the unconstrained case; our Theorem A.3 (Appendix) extends to budgets under DIC.

*McKelvey and Palfrey (1995)*: Quantal Response Equilibrium. The DIC (Appendix A) is the prediction-market analog of their high-temperature contraction condition.

*Budish, Cramton, and Shim (2015)*: Frequent Batch Auctions for equity markets. Our framework applies FBAs to prediction markets, where the information-driven price shocks are even more severe than in equities.

#pagebreak()

= Appendix A: The Risk-Neutral Limit <appendix>

For applications requiring the exact linear-welfare model, we provide algorithms and partial uniqueness results for the risk-neutral budget-constrained problem. These results are superseded by Theorem 5 when risk-averse clearing is acceptable.

== Frank-Wolfe with Lagrangian Budget Handling

Dualize the budget constraints with multipliers $mu_k >= 0$:

$
cal(L)(bold(q), bold(mu)) = f_b(bold(q)) + sum_k mu_k(B_k - "cap"_k(bold(q)))
$

The budget enters only through modified welfare coefficients $w'_i = w_i - sum_k mu_k dot c_i(p^t)$. The subproblem is a standard LP.

#block(inset: (left: 1em, right: 1em), fill: luma(245), radius: 3pt)[
  *Algorithm 1: Frank-Wolfe with Lagrangian Budget Handling*

  *Input:* Orders, markets, groups, MM budgets $B_1, dots, B_K$. Temperature $b > 0$.

  *Initialize:* Solve base LP (no budgets) $arrow.r bold(q)^0$, $bold(p)^0$. Set $bold(mu) = bold(0)$.

  *For* $t = 0, 1, 2, dots$:

  #h(1em) 1. *Prices.* $p_m^t = "softmax"(D_m(bold(q)^t)\/b)$.

  #h(1em) 2. *Budget evaluation.* $"cap"_k^t = sum_(i in "MM"_k) c_i(p^t) dot q_i^t$.

  #h(1em) 3. *Modified welfare.* $w'_i = w_i - sum_k mu_k dot c_i(p^t)$.

  #h(1em) 4. *Frank-Wolfe oracle.* Solve LP: $bold(s) = "argmax"_(bold(q) in cal(C)) sum w'_i q_i$.

  #h(1em) 5. *Step.* $bold(q)^(t+1) = (1 - gamma_t)bold(q)^t + gamma_t bold(s)$, $quad gamma_t = 2\/(t+2)$.

  #h(1em) 6. *Dual update.* $mu_k <- max(0, mu_k + eta_t("cap"_k^t - B_k))$.

  *Terminate* when $max_k("cap"_k - B_k)^+ < epsilon.$
]

#block(inset: (left: 1em, right: 1em), fill: luma(245), radius: 3pt)[
  *Algorithm 2: Annealed Frank-Wolfe*

  Set $b_0 >= ||A||_infinity\/2$, $b_"min"$, cooling $rho = 0.5$. Run Algorithm 1 for $T$ iterations per temperature, warm-starting. Total: $~50$ LP solves.
]

#block(inset: (left: 1em))[
  *Theorem A.1* (Convergence). _Algorithm 1 with $gamma_t = 2\/(t+2)$, $eta_t = 1\/sqrt(t)$ satisfies $cal(L)(bold(q)^*, bold(mu)^*) - cal(L)(bold(q)^t, bold(mu)^t) <= O(1\/sqrt(t))$._
]

== Budget Slippage Convexity

The capital function $"cap"(Q) = p(Q) dot Q$ can be convex — price slippage acts as a brake.

#block(inset: (left: 1em))[
  *Proposition A.1* (Binary Market). _$d^2(p dot Q)\/d Q^2 = (p(1-p))\/b dot [2 + Q(1-2p)\/b]$. Convex iff $Q(2p-1) <= 2b$._
]

#block(inset: (left: 1em))[
  *Proposition A.2* (Group). _For $K$ mutually exclusive outcomes, $bold(v)^top nabla^2 "cap" dot bold(v) = (1\/b) sum_k p_k(v_k - overline(v))^2(2 + (Q_k - overline(Q))\/b)$. PSD iff $overline(Q) - Q_k <= 2b$ for all $k$._
]

== The Diluted Influence Condition

#block(inset: (left: 1em))[
  *Definition A.1* (Diluted Influence). _DIC holds at temperature $b$ if: (1) for independent markets, $Q_m^k(2p_m - 1)^+ <= 2b$; (2) for groups, $overline(Q)_g^k <= 2b$._
]

#block(inset: (left: 1em))[
  *Theorem A.3* (Global Optimality under DIC). _If DIC holds: (1) $"cap"_k$ is convex; (2) the Lagrangian is strictly concave; (3) the KKT point is unique; (4) Algorithm 1 converges to the global optimum._
]

_Practical limitation:_ At realistic annealing temperatures ($b_0 = dollar 0.10$), DIC typically fails. $"cap"_"group" ~ dollar 140$ requires $b >= dollar 70$.

== Fenchel Dual: Unconstrained Price Uniqueness

#block(inset: (left: 1em))[
  *Proposition A.4* (Unconstrained Price Uniqueness). _Without budget constraints, the Fenchel dual $min_(bold(p) in Delta) [W^*(bold(p)) + C_b^*(bold(p))]$ is strictly convex. Clearing prices are unique for any $b > 0$, unconditionally._
]

== Demand Diameter Bound

#block(inset: (left: 1em))[
  *Proposition A.5* (Demand Diameter). _If $bold(q)^1, bold(q)^2$ both maximize $cal(L)(dot, bold(mu))$, then $||bold(D)^1 - bold(D)^2||_2 <= b sum_k mu_k overline(Q)_k \/ p_"min"$. If $bold(mu) = 0$: demands are unique._
]

_Proof._ Midpoint argument: entropy gain $delta >= (p_"min"\/(2b))||Delta bold(D)||^2$ vs cap perturbation $epsilon <= sum mu_k overline(Q)_k\/(4b) dot ||Delta bold(D)||$. Since midpoint $<=$ optimum: $delta <= epsilon$. #h(1fr) $square$

== Generic Uniqueness

#block(inset: (left: 1em))[
  *Theorem A.5* (Generic Uniqueness). _Fix $b > 0$. The set of parameters $theta$ with multiple KKT points has Lebesgue measure zero._
]

_Proof._ (1) Parametric Transversality (Abraham & Robbin, 1967; Sard's theorem): perturbing limit prices and budgets gives full-rank $D_theta F$, so KKT points are generically non-degenerate. (2) Homotopy from $b_0 > ||A||_infinity\/2$ (unique by contraction): KKT points trace smooth paths; new points appear only via saddle-node bifurcation (codimension-1). (3) Global max persistence (Berge): bifurcation-born maxima have welfare strictly below the global max, generically. #h(1fr) $square$

_Corollary._ Annealed Frank-Wolfe with $b_0 >= ||A||_infinity\/2$ converges to the global optimum for generic parameters.

== Convergence of Optimizers

#block(inset: (left: 1em))[
  *Theorem A.6* (Optimizer Convergence). _$"dist"(bold(q)_b^*, cal(Q)^*) -> 0$ as $b -> 0^+$. If the LP optimum is unique, $bold(q)_b^* -> bold(q)^*$._
]

#block(inset: (left: 1em))[
  *Theorem A.7* (Exponential Price Convergence). _$|p_(k^*)(b) - 1| <= (K-1) exp(-Delta\/b)$ and $p_k(b) <= exp(-Delta\/b)$ for $k != k^*$, where $Delta$ is the demand gap._
]


#v(2em)
#line(length: 100%)
#v(0.5em)
#text(size: 9pt, style: "italic")[
  Next steps: (1) Implement risk-averse clearing (Theorem 5) — single convex program, no annealing. (2) Empirical welfare comparison between $P_b^"RA"$ and risk-neutral LP across realistic order books. (3) Quantify $P_b^"RA" -> P_b$ convergence as $B_k -> infinity$.
]
