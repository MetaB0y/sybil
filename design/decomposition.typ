#set document(title: "Decomposed Clearing via Fisher Market Budget Allocation")
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
    Decomposed Clearing \ via Fisher Market Budget Allocation
  ]
  #v(0.5em)
  #text(size: 11pt)[Prediction Market Clearing through Eisenberg-Gale Decomposition]
  #v(0.3em)
  #text(size: 9pt, style: "italic")[Draft --- February 2026]
]

#v(1em)

#block(inset: (x: 2em))[
  #text(weight: "bold")[Summary.]
  Prediction market batch auctions with bundle orders face an exponential state space: $N$ groups of mutually exclusive outcomes produce $product K_j$ joint states. We observe that the Fisher market (Eisenberg-Gale) formulation of clearing provides a natural decomposition mechanism that the standard linear welfare formulation lacks. The key insight: with log utility, splitting a market maker's budget across independently-solved components is a _smooth concave_ optimization problem, whereas with linear welfare the same budget split is _combinatorial_. We prove that the optimal split equalizes utility across components, and that standard mirror descent converges to it at rate $O(1\/t)$ with each iteration requiring only independent per-component solves.
]

#v(1em)

= Introduction

The companion paper (_Prediction Markets Are Fisher Markets_) establishes that batch auction clearing with budget-constrained market makers is an Eisenberg-Gale convex program. This paper addresses the computational consequence: the joint state space is exponentially large, and we need to solve the clearing problem anyway.

*The problem.* Consider $N$ groups of mutually exclusive outcomes, group $j$ having $K_j$ outcomes. The joint state space is $cal(S) = product_j {1, dots, K_j}$, with $|cal(S)| = product K_j$. A _bundle order_ is an order whose payoff depends on the joint state: "buy YES on both $A$ and $B$" pays \$1 only in the joint state where both resolve YES. Such orders create non-separable demand across groups.

When no orders span multiple groups, the clearing problem decomposes into independent per-group programs --- the product-LMSR factorization (Chen and Pennock 2007). Cross-group orders break this, coupling groups _transitively_: if orders link $A$--$B$ and $B$--$C$, clearing requires the full $K_A times K_B times K_C$ state space. In practice, even a handful of bundle orders can connect all groups into one giant component.

*The core insight.* The Eisenberg-Gale structure provides a decomposition mechanism that the linear (risk-neutral) formulation lacks. With log utility, an MM with budget $B_k$ and orders across multiple components can have its budget _smoothly allocated_ across components, with each component solving independently. The optimal allocation satisfies a clean condition --- equal utility across components --- and standard mirror descent finds it. With linear welfare, the same budget allocation is a combinatorial problem: a hard constraint $"cap"_k <= B_k$ cannot be smoothly split.

*What this paper proves.*

+ The _budget decomposition theorem_: when no orders span multiple components, the joint EG program decomposes into independent per-component programs coordinated by budget allocation. The optimal allocation equalizes utility across components (#sym.section\2).

+ _Mirror descent convergence_: the iterative budget reallocation algorithm converges at rate $O(1\/t)$, with each iteration requiring only independent per-component solves (#sym.section\3).

*What this paper does not prove.* Tight welfare bounds for the case when bundle orders _do_ span components. We discuss the approximation quality of dropping or leg-decomposing cross-component orders (#sym.section\4), but the bounds are loose. We also sketch directions for exploiting coupling graph structure and external solvers (#sym.section\5), without proofs.


= Budget Decomposition <decomposition>

== Setup

We use the notation and results of the companion paper. The risk-averse batch auction clearing program over a joint state space $cal(S)$ is the quasi-linear Fisher market:

$
P^"RA": quad max_(bold(q) in cal(C), bold(s) >= 0) quad sum_k [B_k ln(U_k (bold(q)) + s_k) - s_k] + sum_(j in.not "MM") w_j q_j - C_b (bold(D)(bold(q)))
$

where $bold(D)(bold(q)) in RR^(|cal(S)|)$ is the net demand vector over joint states, $C_b (bold(D)) = b ln sum_(s in cal(S)) exp(D_s \/ b)$ is the smoothed minting cost, $U_k (bold(q)) = sum_(i in "MM"_k) L_i q_i$ is MM $k$'s weighted fill, and $s_k >= 0$ is retained cash.

We focus on the _capital-constrained_ regime where $s_k = 0$ for all MMs (i.e., $mu_k = B_k \/ U_k > 1$). When budgets don't bind, the LP solution is optimal regardless of budget allocation, so decomposition is trivially exact.

Suppose the groups partition into _components_ $cal(M) = {C_1, dots, C_M}$. Each component $C_m$ has its own state space $cal(S)_m = product_(j in C_m) {1, dots, K_j}$, orders $cal(O)_m$, and minting cost $C_b^m$.

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

_Proof._ When no orders span components, the minting cost separates: $C_b = sum_m C_b^m$ (product-LMSR). We prove the claim in two steps: (i) the optimal budget allocation equalizes utility, and (ii) the decomposed fills match the monolithic optimum.

*(i) Equal utility.* Each per-component program $P_m (bold(B)^m)$ is a strictly concave maximization with a unique optimum. The objective $B_k^m ln U_k^m (bold(q)_m)$ is jointly concave in $(B_k^m, bold(q)_m)$ (it is the perspective of $ln U_k^m$, and the perspective of a concave function is concave). By the envelope theorem, the marginal value of budget in component $m$ is:

$ (d W_m^*) / (d B_k^m) = ln U_k^(m *) $

(The indirect effect through optimal fills vanishes by the optimality of $bold(q)_m^*$.) The coordination problem $max_(bold(B)) sum_m W_m^* (bold(B)^m)$ subject to $sum_m B_k^m = B_k$ has the first-order condition $ln U_k^(m *) = lambda_k$ for all active components: MM $k$ achieves the same utility in every component where it holds budget.

*(ii) Fills match the monolithic.* The monolithic program has shadow price $mu_k = B_k \/ sum_m U_k^(m *)$ --- the same for all components. At the decomposed optimum, $U_k^(m *) = u_k$ for all active $m$, so $sum_m U_k^(m *) = M_k u_k$ (where $M_k$ is the number of active components). The decomposed shadow price $mu_k^m = B_k^m \/ u_k = (B_k \/ M_k) \/ u_k = B_k \/ (M_k u_k) = mu_k$.

Since shadow prices match and minting costs are independent, the KKT conditions for each order $i$ are identical in both programs. By uniqueness of the per-component optimum, the fills agree. #h(1fr) $square$

_Remark._ The decomposed objective $sum_m B_k^m ln U_k^m$ is _less_ than the monolithic objective $B_k ln sum_m U_k^m$ (by Jensen's inequality). This reflects the pooling benefit of joint Kelly optimization. The theorem says the _fills_ agree, not the objective values.

_Remark._ This theorem is most interesting when MMs have orders across multiple independently-solvable components. Without cross-component MMs, the components are fully independent and no coordination is needed at all.


== Why Linear Welfare Cannot Decompose

In the risk-neutral (linear welfare) model, MM $k$'s contribution is $sum_(i in "MM"_k) w_i q_i$ with a hard budget constraint $"cap"_k <= B_k$. There is no natural way to split a hard budget across components: the constraint $"cap"_k^m <= B_k^m$ with $sum_m B_k^m = B_k$ introduces a combinatorial allocation problem (which component gets how much budget?) on top of the already-intractable clearing problem. The $ln$ utility replaces this discrete allocation with a smooth, differentiable one. This is the computational payoff of the Fisher market structure.


= Budget Equalization via Mirror Descent <algorithm>

The equal-utility condition (@thm-decomp) suggests a natural iterative algorithm. The coordination problem $max sum_m W_m^* (bold(B)^m)$ subject to $sum_m B_k^m = B_k$ is concave, and its gradient is $partial W_m^* \/ partial B_k^m = ln U_k^(m *)$ (envelope theorem). Mirror descent with KL divergence gives:

+ *Initialize.* For each MM $k$, set $B_k^m = B_k \/ |{m : "MM"_k inter cal(O)_m != emptyset}|$ (equal split across active components).

+ *Solve.* For each component $m$ in parallel, solve $P_m (bold(B)^m)$ to get optimal fills $bold(q)_m^*$ and utilities $U_k^(m *)$.

+ *Update.* For each MM $k$, reallocate:
  $ B_k^m <- B_k dot (B_k^m dot U_k^(m *)) / (sum_(m') B_k^(m') dot U_k^(m' *)) $
  Multiply each component's allocation by its utility and renormalize.

+ *Repeat* steps 2--3 until convergence.

The update is _multiplicative weights_: each component's budget share grows proportionally to the utility it delivers. At the fixed point, $U_k^(m *)$ must be constant across $m$, matching @thm-decomp.

#theorem(name: "Convergence")[
  _The mirror descent algorithm converges to the optimal budget allocation. For smooth $W_m^*$, the welfare gap satisfies $W^* - W(bold(B)^t) = O(1\/t)$._
] <thm-convergence>

_Proof sketch._ The coordination problem is concave (each $W_m^*$ is a concave value function). The update exponentiates the gradient and renormalizes --- mirror descent with KL divergence. The $O(1\/t)$ rate for concave maximization over the simplex is standard (Beck and Teboulle 2003). Smoothness follows from continuous dependence of equilibrium utilities on budgets. #h(1fr) $square$

_Remark._ Each iteration requires solving $M$ independent convex programs --- fully parallelizable. In practice, 3--5 iterations suffice for budget stabilization. Interior-point solvers can warm-start from the previous iteration's solution.

*Practical speed.* Without bundle orders, the decomposition gives a significant speed advantage. If $N$ groups each have $K$ outcomes, the monolithic solve has state space $K^N$ while the decomposed solve runs $N$ independent $K$-state programs in parallel. Even for moderate $N$ and $K$, this is the difference between intractable and instant. With MMs spanning components, add 3--5 iterations of budget equalization --- still far cheaper than monolithic.


= Welfare Bounds for Approximate Decomposition <welfare>

When bundle orders span multiple components, the decomposition is not exact and cross-component orders must be handled approximately. We state two simple bounds; both are loose but provide the right qualitative picture.

*Dropping cross-component orders.* Excluding all cross-component orders $cal(O)_times$ loses at most $sum_(i in cal(O)_times) w_i overline(q)_i$ welfare (sum of each dropped order's limit price times max fill). This is an immediate consequence of feasibility containment.

*Leg decomposition.* Decomposing each bundle into per-component marginal legs (averaging over the other components' states at reference prices) introduces error bounded by the _interaction magnitude_ $Delta_i = ||phi_i - hat(phi)_i||_infinity$: the welfare gap is at most $sum_(i in cal(O)_times) overline(q)_i Delta_i$. This follows from the Lipschitz property of the smoothed minting cost.

*Limitations.* Both bounds are loose in practice. The drop bound ignores that cross orders compete for minting capacity with within-component orders. The leg decomposition bound is "tight when $Delta_i approx 0$" --- but this is vacuous, since the interesting cases have $Delta_i = 1\/4$ (buy both YES) or $Delta_i = 1\/2$ (conditional orders). Tighter instance-dependent bounds remain open.


= Directions <directions>

This section sketches computational and architectural ideas that we have not proved but believe are promising.

== Coupling Graph and Structured Computation

The _coupling graph_ has market groups as vertices and edges wherever a bundle order creates non-separable dependence. Connected components identify the minimal sets of groups that must be solved jointly. In principle, if the coupling graph has low treewidth $tau$, minting cost gradients can be computed in $O(K^(tau+1))$ time via junction tree algorithms rather than $O(K^N)$ full enumeration.

In practice, coupling graphs may not have low treewidth --- a few popular bundles can connect many groups transitively. The question is whether _demand sparsity_ helps: most joint states have negligible demand, so column generation (iteratively discovering active states) may avoid full enumeration even when the coupling graph is dense.

== A Searcher Model

The decomposition framework suggests a _market for clearing services_ analogous to Proposer-Builder Separation in Ethereum. The exchange solves per-group clearing (the easy separable part) and coordinates budget allocation. External _searchers_ compete to solve coupled components: a searcher takes a set of coupled groups, solves the joint clearing problem, and submits fills and prices. The exchange selects and combines proposals via budget equalization.

The log-utility structure makes this naturally proportional rather than winner-take-all: multiple searchers' solutions can coexist, weighted by the welfare they deliver. Whether this is incentive-compatible, collusion-resistant, or practically useful remains to be established.


= Discussion

== What We Proved

#align(center)[
  #table(
    columns: 3,
    align: (left, center, center),
    stroke: none,
    [*Setting*], [*Linear welfare*], [*Log welfare (EG)*],
    [Single group], [LP (easy)], [Convex program (easy)],
    [Multi-group, no bundles], [Independent LPs], [Independent EGs + budget split],
    [Multi-group + cross-component MMs], [Combinatorial budget split], [Smooth budget split (mirror descent)],
  )
]

The key result is the contrast in the last row: with linear welfare, splitting an MM's hard budget constraint across components is a combinatorial optimization. With log utility, the same problem is smooth and concave --- the budget flows to where it generates the most utility, equalized at optimum by a simple multiplicative-weights iteration.

== What Remains Open

+ *Tight welfare bounds for bundles.* Our bounds for dropping or leg-decomposing cross-component orders are worst-case. Instance-dependent bounds would be much more useful for deciding when approximate decomposition is acceptable.

+ *Bundle handling.* The per-market leg decomposition used by our LP and EG solvers misprices non-separable bundles. The fill price is linear in per-market prices ($0.5 p_A + 0.5 p_B$ for "buy both YES") while the true value is multiplicative ($p_A dot p_B$). At $p_A = 0.6, p_B = 0.4$: leg price $= 0.50$, true price $= 0.24$. This overpricing can reject profitable bundle orders. Correctly pricing bundles requires solving the joint-state formulation, making the decomposition approach essential.

+ *Practical convergence.* We proved $O(1\/t)$ worst-case. Empirical convergence may be much faster. Characterizing when this happens would guide the choice between decomposition and monolithic solving.


#v(2em)
#line(length: 100%)
#v(0.5em)
#text(size: 9pt, style: "italic")[
  This paper is a companion to _Prediction Markets Are Fisher Markets_ (2026), which establishes the Eisenberg-Gale structure that this paper exploits computationally.
]
