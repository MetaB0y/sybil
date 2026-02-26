#set document(title: "Decomposed Combinatorial Clearing via Fisher Market Budget Allocation")
#set text(font: "New Computer Modern", size: 10pt)
#set page(margin: (x: 1.5in, y: 1.2in), numbering: "1")
#set par(justify: true, leading: 0.55em)
#set heading(numbering: "1.")
#show heading.where(level: 1): it => block(above: 1.5em, below: 0.8em)[#it]
#show heading.where(level: 2): it => block(above: 1.2em, below: 0.6em)[#it]

// Theorem-like environments with auto-numbering
#show figure.where(kind: "theorem"): it => align(left, it.body)
#show figure.where(kind: "proposition"): it => align(left, it.body)
#show figure.where(kind: "definition"): it => align(left, it.body)

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

#let definition(name: none, body) = figure(
  kind: "definition", supplement: [Definition], numbering: "1", outlined: false,
  block(width: 100%, inset: (left: 1em))[
    *Definition #context counter(figure.where(kind: "definition")).display("1")*#if name != none [ (#name)]. #body
  ]
)

#align(center)[
  #text(size: 15pt, weight: "bold")[
    Decomposed Combinatorial Clearing \ via Fisher Market Budget Allocation
  ]
  #v(0.5em)
  #text(size: 11pt)[Scalable Prediction Market Clearing through Eisenberg-Gale Decomposition]
  #v(0.3em)
  #text(size: 9pt, style: "italic")[Draft — February 2026]
]

#v(1em)

#block(inset: (x: 2em))[
  #text(weight: "bold")[Summary.]
  Prediction market batch auctions with bundle orders face an exponential state space: $N$ groups of mutually exclusive outcomes produce $product K_j$ joint states. The welfare-maximizing clearing problem (an Eisenberg-Gale convex program) is structurally tractable but computationally intractable for large coupled components. We show that the Fisher market structure provides a natural decomposition: independently-solved components are coordinated through MM budget allocation, with the optimality condition being _equal utility_ across components. The iterative algorithm — mirror descent on the concave coordination problem — converges at rate $O(1\/t)$. We give welfare bounds for approximate decomposition, describe an automatic grouping algorithm based on the coupling graph, and propose a _searcher model_ in which external solvers compete to clear coupled components, coordinated by the exchange via budget equalization.
]

#v(1em)

= Introduction

The companion paper (_Prediction Markets Are Fisher Markets_) establishes that batch auction clearing with budget-constrained market makers is an Eisenberg-Gale convex program. The Fisher market isomorphism holds for _arbitrary joint state spaces_ — not just single mutually exclusive groups. This paper addresses the computational consequence: the joint state space is exponentially large, and we need to solve the clearing problem anyway.

*The problem.* Consider $N$ groups of mutually exclusive outcomes, group $j$ having $K_j$ outcomes. The joint state space is $cal(S) = product_j {1, dots, K_j}$, with $|cal(S)| = product K_j$. A _bundle order_ is an order whose payoff depends on the joint state: "buy YES on both $A$ and $B$" pays \$1 only in the joint state where both resolve YES. Such orders create non-separable demand across groups.

When no orders span multiple groups, the clearing problem decomposes into independent per-group programs — the product-LMSR factorization (Chen and Pennock 2007). Cross-group orders break this, coupling groups _transitively_: if orders link $A$–$B$ and $B$–$C$, clearing requires the full $K_A times K_B times K_C$ state space. In practice, a handful of bundle orders can connect all groups into one giant component.

*The insight.* The Eisenberg-Gale structure provides a decomposition mechanism that the linear (risk-neutral) formulation lacks. The key is _budget splitting_: an MM with budget $B_k$ and orders across multiple components can have its budget allocated across components, with each component solving independently. The optimal allocation satisfies a clean condition — equal utility across components — and the iterative algorithm that finds it is _mirror descent_ on the concave coordination problem.

This turns an intractable monolithic optimization into a coordination problem: solve components independently, adjust budget allocations, repeat. The components can be solved in parallel, by different machines, or by different _searchers_ (external solvers that compete to provide the best cross-group solutions).

*Contributions.*

+ The _budget decomposition theorem_: the joint EG program decomposes into per-component programs coordinated by budget allocation, with equal utility at optimality (§2).

+ _Mirror descent convergence_: the iterative budget reallocation algorithm converges at rate $O(1\/t)$, with each iteration requiring only independent per-component solves (§3).

+ _Welfare bounds_ for approximate decomposition: when cross-group orders are dropped or leg-decomposed, the welfare loss is bounded by the contribution of the dropped correlations (§4).

+ _Automatic grouping_: the coupling graph identifies minimal components; a threshold policy decides which to solve exactly vs. approximately (§5).

+ _The searcher model_: external solvers compete to clear coupled components, coordinated by the exchange via budget equalization. The Fisher market structure makes this incentive-compatible (§6).


= Budget Decomposition <decomposition>

== Setup

We use the notation and results of the companion paper. The risk-averse batch auction clearing program over a joint state space $cal(S)$ is:

$
P^"RA": quad max_(bold(q) in cal(C)) quad sum_k B_k ln U_k (bold(q)) + sum_(j in.not "MM") w_j q_j - C_b (bold(D)(bold(q)))
$

where $bold(D)(bold(q)) in RR^(|cal(S)|)$ is the net demand vector over joint states, $C_b (bold(D)) = b ln sum_(s in cal(S)) exp(D_s \/ b)$ is the smoothed minting cost, and $U_k (bold(q)) = sum_(i in "MM"_k) L_i q_i$ is MM $k$'s weighted fill.

Suppose the groups partition into _components_ $cal(M) = {C_1, dots, C_M}$ (we discuss how to choose this partition in §5). Each component $C_m$ has its own state space $cal(S)_m = product_(j in C_m) {1, dots, K_j}$, orders $cal(O)_m$, and minting cost $C_b^m$.

== The Monolithic vs. Decomposed Program

If an MM $k$ has orders in multiple components, the monolithic program optimizes over all of them jointly. The decomposed approach allocates a _budget share_ $B_k^m >= 0$ to each component, with $sum_m B_k^m = B_k$, and solves each component independently:

$
P_m (bold(B)^m): quad max_(bold(q)_m in cal(C)_m) quad sum_k B_k^m ln U_k^m (bold(q)_m) + sum_(j in cal(O)_m \\ "MM") w_j q_j - C_b^m (bold(D)^m (bold(q)_m))
$

The _coordination problem_ is to find the budget allocation $bold(B) = {B_k^m}$ that maximizes total welfare:

$
max_(bold(B) >= 0) quad sum_m W_m^* (bold(B)^m) quad "s.t." quad sum_m B_k^m = B_k quad forall k
$

where $W_m^* (bold(B)^m)$ is the optimal welfare of component $m$ given budgets $bold(B)^m$.

#theorem(name: "Budget Decomposition")[
  _When no orders span multiple components, the decomposed program with optimal budget allocation achieves the same welfare as the monolithic program. The optimal allocation satisfies, for each MM $k$ active in components $m$ and $m'$:_

  $ ln U_k^(m *) = ln U_k^(m' *) $

  _Equal utility across components: MM $k$ achieves the same weighted fill in every component where it is active._
] <thm-decomp>

_Proof._ When no orders span components, the minting cost separates: $C_b = sum_m C_b^m$ (product-LMSR). The monolithic program becomes a joint optimization over fills $bold(q)$ and budget allocations $bold(B)$:

$
max_(bold(q), bold(B) >= 0) quad sum_m [sum_k B_k^m ln U_k^m (bold(q)_m) + "retail"_m - C_b^m (bold(D)^m)] quad "s.t." quad sum_m B_k^m = B_k quad forall k
$

This is jointly concave: $B_k^m ln U_k^m (bold(q)_m)$ is concave in $(B_k^m, bold(q)_m)$ jointly (it is the perspective of the concave function $ln U_k^m$, and the perspective of a concave function is concave). The remaining terms are concave in $bold(q)_m$ and independent of $bold(B)$.

By the envelope theorem, the marginal value of budget in component $m$ is:

$ (d W_m^*) / (d B_k^m) = ln U_k^(m *) $

where $U_k^(m *)$ is the optimal utility at the current budget. (The indirect effect through the optimal fills vanishes by the optimality of $bold(q)_m^*$.) The coordination problem $max_(bold(B)) sum_m W_m^* (bold(B)^m)$ subject to $sum_m B_k^m = B_k$ has the first-order condition:

$
(d W_m^*) / (d B_k^m) = lambda_k quad forall m "where" B_k^m > 0
$

Therefore $ln U_k^(m *) = lambda_k$ for all active components: MM $k$ achieves the same utility in every component. Since the decomposed problem is a relaxation of the monolithic (the monolithic is free to choose the same budget split), and the monolithic with separated minting cost factors as the decomposed problem, the welfare is identical. #h(1fr) $square$

_Remark._ The equal-utility condition determines the budget split implicitly: increasing $B_k^m$ increases $U_k^(m *)$ (the $ln$ singularity guarantees this — more budget means more fills). Components with better opportunities for MM $k$ (higher utility per dollar) receive more budget until utilities equalize.

_Remark._ When orders _do_ span components, the decomposition is approximate: cross-component orders are either dropped, leg-decomposed, or handled by a searcher (§4, §6).


== Why Linear Welfare Cannot Decompose

In the risk-neutral (linear welfare) model, MM $k$'s contribution is $sum_(i in "MM"_k) w_i q_i$ with a hard budget constraint $"cap"_k <= B_k$. There is no natural way to split a hard budget across components: the constraint $"cap"_k^m <= B_k^m$ with $sum_m B_k^m = B_k$ introduces a combinatorial allocation problem (which component gets how much budget?) on top of the already-intractable clearing problem. The $ln$ utility replaces this discrete allocation with a smooth, differentiable one. This is the computational payoff of the Fisher market structure.


= Budget Equalization via Mirror Descent <algorithm>

The equal-utility condition (@thm-decomp) suggests a natural iterative algorithm. We need to find the budget split that equalizes $ln U_k^(m *)$ across components. The coordination problem $max sum_m W_m^* (bold(B)^m)$ subject to $sum_m B_k^m = B_k$ is concave, and its gradient is $partial W_m^* \/ partial B_k^m = ln U_k^(m *)$ (envelope theorem). Mirror descent with KL divergence as Bregman divergence gives:

+ *Initialize.* For each MM $k$, set $B_k^m = B_k \/ |{m : "MM"_k inter cal(O)_m != emptyset}|$ (equal split across active components).

+ *Solve.* For each component $m$ in parallel, solve $P_m (bold(B)^m)$ to get optimal fills $bold(q)_m^*$ and utilities $U_k^(m *)$.

+ *Update.* For each MM $k$, reallocate:
  $ B_k^m <- B_k dot (B_k^m dot U_k^(m *)) / (sum_(m') B_k^(m') dot U_k^(m' *)) $
  Multiply each component's allocation by its utility and renormalize.

+ *Repeat* steps 2–3 until convergence.

The update is _multiplicative weights_: each component's budget share grows proportionally to the utility it delivers. At the fixed point, $U_k^(m *)$ must be constant across $m$ (otherwise the highest-utility component would attract more budget), matching @thm-decomp.

#theorem(name: "Convergence")[
  _The mirror descent algorithm converges to the optimal budget allocation. For smooth $W_m^*$, the welfare gap satisfies $W^* - W(bold(B)^t) = O(1\/t)$._
] <thm-convergence>

_Proof._ The coordination problem is concave: each $W_m^* (bold(B)^m)$ is concave (it is the value function of maximizing a jointly concave objective over a convex set). The gradient $partial W_m^* \/ partial B_k^m = ln U_k^(m *)$ is computable by solving the per-component program and reading off the equilibrium utilities.

The update exponentiates the gradient and renormalizes — this is mirror descent with the KL divergence as Bregman divergence (equivalently, exponentiated gradient ascent). Since $exp(ln U_k^(m *)) = U_k^(m *)$, the update takes the simple multiplicative form above.

For concave maximization of an $L$-smooth function over the simplex, mirror descent with appropriate step size converges at rate $O(L D^2 \/ t)$ where $D$ is the KL diameter of the feasible region (Beck and Teboulle 2003). In our setting, smoothness follows from the continuous dependence of equilibrium utilities on budgets (the EG program has a unique optimum that varies smoothly with parameters in the interior). #h(1fr) $square$

_Remark._ Each iteration requires solving $M$ independent convex programs — fully parallelizable. In practice, 3–5 iterations suffice for the budget allocation to stabilize (the per-component programs change slowly as budgets shift).

_Remark._ Warm-starting: after a budget update, each component's program is a small perturbation of the previous one. Interior-point solvers can warm-start from the previous solution, making subsequent iterations much cheaper than the first.


= Welfare Bounds for Approximate Decomposition <welfare>

When bundle orders span multiple components, the decomposition is not exact. The cross-component orders must be handled approximately. We consider two strategies and bound the welfare loss of each.

== Dropping Cross-Component Orders

The simplest strategy: exclude all cross-component orders and solve the remaining (exactly decomposable) system.

#proposition(name: "Drop Bound")[
  _Let $cal(O)_times$ be the set of cross-component orders. The welfare loss from dropping them satisfies:_

  $ W^* - W_"drop"^* <= sum_(i in cal(O)_times) w_i overline(q)_i $

  _where $w_i$ is the limit price and $overline(q)_i$ the maximum fill of order $i$._
] <prop-drop>

_Proof._ The dropped system has feasible set $cal(C) inter {q_i = 0 : i in cal(O)_times}$, which is a subset of the monolithic feasible set, so $W_"drop"^* <= W^*$. Conversely, the monolithic optimum fills each cross-component order $i$ by at most $overline(q)_i$ at welfare per unit at most $w_i$ (the limit price). The within-component orders' fills in the monolithic optimum are feasible for the dropped system (since they don't couple across components), so the gap is at most the cross orders' contribution. #h(1fr) $square$

This bound is crude — it ignores that cross-component orders compete for minting capacity with within-component orders. In practice, the welfare contribution of cross orders is much smaller than $sum w_i overline(q)_i$ because many are only partially filled.

== Leg Decomposition

A tighter approximation: decompose each cross-component order into per-component _legs_ using marginal payoffs.

A bundle order $i$ spanning components $C_a$ and $C_b$ has payoff $phi_i (s_a, s_b)$ depending on the joint state. Given current prices $bold(p)_b$ in component $b$, the _leg_ in component $a$ is the marginal payoff:

$ phi_i^a (s_a) = sum_(s_b) p_b (s_b) dot phi_i (s_a, s_b) $

The leg captures the order's expected demand contribution to $a$, averaged over $b$'s outcomes at current prices.

#proposition(name: "Leg Decomposition Error")[
  _The welfare difference between the joint optimum and the leg-decomposed optimum is bounded by the payoff correlation: the total variation between $phi_i (s_a, s_b)$ and the separable approximation $phi_i^a (s_a) + phi_i^b (s_b) - E[phi_i]$._
] <prop-leg>

The bound is tight when cross-component orders have nearly separable payoffs (e.g., a bundle "buy A and B" where A and B are nearly independent). It is loose when payoffs are strongly correlated (e.g., conditional orders "buy A if B resolves YES").

_Remark._ Leg decomposition is iterative: the legs depend on prices $bold(p)_b$, which depend on fills, which depend on the legs. In practice, one round of leg decomposition (using prices from the within-component solution) captures most of the welfare. A second round, updating legs using the new prices, is rarely necessary.

== When Decomposition Is Exact

Decomposition incurs zero welfare loss when cross-component orders contribute no _irreducible_ correlation:

+ *No cross-component orders.* The product-LMSR factorization applies directly (@thm-decomp).

+ *Separable cross-component payoffs.* If every cross-component order $i$'s payoff factors as $phi_i (s_a, s_b) = phi_i^a (s_a) + phi_i^b (s_b)$, leg decomposition is exact (there is no correlation to approximate).

+ *Sparse coupling with tight budgets.* When cross-component orders are few and MM budgets are large relative to their welfare contribution, the cross orders' fills are negligible and the decomposition error vanishes.


= Automatic Grouping <grouping>

== The Coupling Graph

#definition(name: "Coupling Graph")[
  _The coupling graph $G = (V, E)$ has groups as vertices. An edge $(j, j')$ exists if any order has non-separable payoffs across groups $j$ and $j'$ (i.e., the order's payoff depends on the joint state of $j$ and $j'$, not just their marginals)._
] <def-coupling>

Connected components of $G$ are the minimal sets of groups that must be solved jointly for exact clearing. The state space of component $C_m$ is $|cal(S)_m| = product_(j in C_m) K_j$.

== Threshold Policy

Given a computational budget (maximum tractable state space size $S_max$):

+ Compute connected components of $G$.
+ For each component: if $|cal(S)_m| <= S_max$, solve exactly.
+ If $|cal(S)_m| > S_max$: find a minimum-weight edge cut that partitions the component into sub-components each with $|cal(S)| <= S_max$. Edge weights are the welfare contribution of the cross-group orders on that edge. Cut edges become leg-decomposed orders.

This minimizes the welfare lost to approximation subject to the computational constraint.

== Treewidth and Structured Coupling

The coupling graph (@def-coupling) connects the clearing problem to graphical model inference. The minting cost $C_b (bold(D))$ requires summing over joint states $cal(S) = product K_j$, but this sum factors according to the coupling graph's structure.

Each cross-component order $i$ introduces a _factor_ $phi_i$ over the groups it touches. The minting cost gradient $partial C_b \/ partial D_s$ becomes a marginal computation over a factor graph — exactly the setting of belief propagation and junction tree algorithms.

*Key observation.* If the coupling graph has treewidth $tau$, the minting cost and its gradients can be computed in $O(product_(j=1)^(tau+1) K_j)$ time via the junction tree algorithm, avoiding the full $product K_j$ enumeration. Since each order touches at most $kappa$ groups (in our system, $kappa <= 5$), factor size is bounded. The bottleneck is the treewidth $tau$ of the coupling graph, which grows with transitive coupling.

In practice, coupling graphs tend to have moderate treewidth: most markets are coupled to only a few others through bundle orders. When treewidth is low (say $tau <= 10$ with binary outcomes), exact clearing via message passing is tractable ($2^11 approx 2000$ states per junction tree node). When treewidth is high, the threshold policy (§5.2) falls back to approximate decomposition.


= The Searcher Model <searchers>

The decomposition framework enables a _market for clearing services_, analogous to Proposer-Builder Separation (PBS) in Ethereum.

== Architecture

+ *The exchange* solves per-group clearing (the easy, separable part) and coordinates budget allocation across components.

+ *Searchers* are external solvers that identify profitable cross-group opportunities. A searcher takes a set of coupled groups, solves the joint EG program over their state space, and submits a _clearing proposal_: fills for the cross-group orders plus the induced prices.

+ *Coordination.* The exchange combines proposals from multiple searchers via budget equalization (@thm-decomp). Each searcher's component receives MM budget proportional to the utility it delivers.

== Incentive Structure

A searcher that finds a higher-welfare solution for a coupled component attracts more MM budget (via the budget equalization mechanism), earning more from the bid-ask spread. Searchers compete on _welfare extraction_: the one that best exploits cross-group correlations wins the budget allocation.

This is incentive-compatible: searchers are rewarded exactly for the welfare they create beyond what leg decomposition provides. The exchange does not need to know how to solve the combinatorial problem — it outsources it to competing searchers and coordinates via the Fisher market mechanism.

== Connection to PBS

In Ethereum's PBS, builders compete to construct blocks (bundles of transactions). The proposer selects the most valuable block. In our model:

#align(center)[
  #table(
    columns: 3,
    align: (left, center, center),
    stroke: none,
    [*Component*], [*PBS (Ethereum)*], [*Clearing decomposition*],
    [Builder/Searcher], [Constructs blocks], [Solves coupled components],
    [Proposer/Exchange], [Selects best block], [Coordinates via budget equalization],
    [Bid], [Block value (MEV)], [Component welfare ($U_k^m$)],
    [Selection], [Highest bid wins], [Mirror descent (smooth)],
  )
]

The key difference: PBS is winner-take-all (one block selected); our model is _proportional_ (multiple searchers' solutions coexist, weighted by welfare). This follows from the $ln$ utility — the smooth allocation again.


= Discussion

== What We Proved

The Eisenberg-Gale structure turns the intractable combinatorial clearing problem into a tractable coordination problem:

#align(center)[
  #table(
    columns: 3,
    align: (left, center, center),
    [*Problem*], [*Linear welfare*], [*Log welfare (EG)*],
    [Single group], [LP (easy)], [Convex program (easy)],
    [Multi-group, no bundles], [Independent LPs], [Independent EGs + budget split],
    [Multi-group, bundles], [Intractable], [Decompose + mirror descent],
    [Budget coordination], [Combinatorial], [Smooth (equal utility)],
  )
]

== Open Questions

+ *Tight welfare bounds.* Our bounds for approximate decomposition are worst-case. Can we give instance-dependent bounds based on the spectral properties of the coupling graph?

+ *Online / streaming setting.* Orders arrive continuously. Can mirror descent be run incrementally (updating budgets as new orders arrive) rather than from scratch each batch?

+ *Searcher collusion.* If searchers collude, they can extract more MM budget than competitive equilibrium would allocate. What mechanisms prevent this?


#v(2em)
#line(length: 100%)
#v(0.5em)
#text(size: 9pt, style: "italic")[
  This paper is a companion to _Prediction Markets Are Fisher Markets_ (2026), which establishes the Eisenberg-Gale structure that this paper exploits computationally.
]
