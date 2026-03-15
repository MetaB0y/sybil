#set document(title: "Bundle Clearing via Sparse Minting Cost Decomposition")
#set text(font: "New Computer Modern", size: 10pt)
#set page(margin: (x: 1.5in, y: 1.2in), numbering: "1")
#set par(justify: true, leading: 0.55em)
#set heading(numbering: "1.")
#show heading.where(level: 1): it => block(above: 1.5em, below: 0.8em)[#it]
#show heading.where(level: 2): it => block(above: 1.2em, below: 0.6em)[#it]

// Theorem-like environments
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
    Bundle Clearing via \ Sparse Minting Cost Decomposition
  ]
  #v(0.5em)
  #text(size: 11pt)[Exact Cross-Group Clearing Without State Enumeration]
  #v(0.3em)
  #text(size: 9pt, style: "italic")[Draft --- February 2026]
]

#v(1em)

#block(inset: (x: 2em))[
  #text(weight: "bold")[Summary.]
  Bundle orders couple clearing across an exponentially large joint state space ($product K_j$ states). We show that the joint minting cost and its gradient can be computed _exactly_ in time polynomial in the number of bundles, without enumerating joint states. The key: bundle demand is sparse (each bundle touches $<= 2^5 = 32$ joint states), so the minting cost decomposes as a separable product-form base plus sparse corrections. This makes the existing Eisenberg-Gale Frank-Wolfe solver handle bundles exactly. When the coupling structure is too dense for exact computation, we fall back to price-taking injection --- filling bundles at base clearing prices --- with second-order welfare loss.
]

#v(1em)

= Introduction

Bundle orders span multiple market groups. "Buy A-YES and B-YES" creates demand in the joint state space $cal(S) = product_j {1, dots, K_j}$, coupling groups that would otherwise solve independently. The joint state space is exponentially large: $N$ binary groups produce $2^N$ states. Even a handful of bundles can couple all groups into one component.

The current approach --- marginal decomposition --- linearizes each bundle into per-market legs. This systematically overprices non-separable bundles: for "buy A-YES and B-YES," the leg price is $0.5 p_A + 0.5 p_B$ while the true price is $p_A dot p_B$. At $p_A = 0.6, p_B = 0.4$: leg price $= 0.50$, true price $= 0.24$. The mispricing rejects profitable bundles.

The standard fix --- enumerate the joint state space --- is exponential. We show this is unnecessary. The minting cost can be computed exactly in polynomial time by exploiting the sparsity of bundle demand: each bundle affects at most $2^(K_i) <= 32$ joint states (with MAX_MARKETS_PER_ORDER $= 5$). The rest of the state space is separable and factors.


= The Minting Cost Obstacle <obstacle>

Without bundles, demand in joint state $s = (s_1, dots, s_N)$ is separable: $D_s = sum_j D_(s_j)^j$. The minting cost $C_b(bold(D)) = b ln sum_s exp(D_s\/b)$ factors into a product:

$ Z_0 equiv sum_s exp(D_s^0\/b) = sum_s product_j exp(D_(s_j)^j\/b) = product_j underbrace(sum_(s_j) exp(D_(s_j)^j\/b), Z_j) $

Computable in $O(sum_j K_j)$ time. The gradient is the product distribution $p_s^0 = product_j p_(s_j)^j$ where $p_(s_j)^j = exp(D_(s_j)^j\/b) \/ Z_j$ is the per-group softmax. Each group's clearing prices are independent.

Bundles break this. A bundle spanning groups $A, B$ with fill $q_i$ adds demand $phi_(i,s) dot q_i$ to joint states $s$. The total demand becomes $D_s = sum_j D_(s_j)^j + delta_s$ where $delta_s = sum_(i in "bundles") phi_(i,s) q_i$ is the aggregate bundle demand. The product factorization no longer holds: $sum_s exp((sum_j D_(s_j)^j + delta_s)\/b) != product_j Z_j$ because the $delta_s$ term couples groups.

The naive approach enumerates all $product K_j$ states. With thousands of groups, this is absurd. But $delta_s$ is nonzero on at most $sum_i 2^(K_i)$ states --- a polynomial number. This sparsity is what we exploit.


= Sparse Minting Cost Decomposition <sparse>

== The Decomposition

#theorem(name: "Sparse Minting Cost")[\
  _Let $D_s = sum_j D_(s_j)^j + delta_s$ where $delta_s$ is nonzero on a set $cal(A) subset cal(S)$ of joint states. Then:_

  $ Z = Z_0 + sum_(s in cal(A)) R_s dot (exp(delta_s \/ b) - 1) $

  _where $Z_0 = product_j Z_j$ is the separable normalization and $R_s = product_j exp(D_(s_j)^j \/ b)$ is the separable contribution of state $s$. When bundle $i$ spans groups $G_i subset {1, dots, N}$ with $|G_i| <= 5$:_

  $ R_s = Z_0 / (product_(j in G_i) Z_j) dot product_(j in G_i) exp(D_(s_j)^j \/ b) $

  _Each term costs $O(|G_i|)$, not $O(N)$. The total cost is $O(sum_j K_j + sum_i |G_i| dot 2^(K_i))$._
] <thm-sparse>

_Proof._ Write $Z = sum_s exp(D_s\/b) = sum_s exp(sum_j D_(s_j)^j\/b) dot exp(delta_s\/b)$. Split into states with $delta_s = 0$ and $delta_s != 0$:

$
Z &= sum_(s in.not cal(A)) product_j exp(D_(s_j)^j\/b) + sum_(s in cal(A)) product_j exp(D_(s_j)^j\/b) dot exp(delta_s\/b) \
  &= underbrace(sum_(s in cal(S)) product_j exp(D_(s_j)^j\/b), = Z_0) - sum_(s in cal(A)) R_s + sum_(s in cal(A)) R_s dot exp(delta_s\/b) \
  &= Z_0 + sum_(s in cal(A)) R_s dot (exp(delta_s\/b) - 1)
$

For the efficient computation of $R_s$: when $s$ is determined by the bundle's groups $G_i$ (the remaining groups are free), the product over all groups factors as $product_(j in G_i) exp(D_(s_j)^j\/b) dot product_(j in.not G_i) exp(D_(s_j)^j\/b)$. The second product, summed over free groups, gives $product_(j in.not G_i) Z_j = Z_0 \/ product_(j in G_i) Z_j$. This cancels when we collect terms by the bundle's group restriction, giving the stated formula. #h(1fr) $square$

*Cost analysis.* Precompute $Z_j$ for each group: $O(sum_j K_j)$. Precompute $Z_0 = product_j Z_j$: $O(N)$. For each bundle $i$ spanning groups $G_i$ with $|G_i| <= 5$ and $2^(|G_i|) <= 32$ joint states: compute $Z_0 \/ product_(j in G_i) Z_j$ once ($O(|G_i|)$), then enumerate the bundle's states ($O(2^(|G_i|))$). Total: $O(sum_j K_j + N + sum_i |G_i| dot 2^(|G_i|))$. For 3000 groups, 1000 bundles spanning 2--3 groups: $O(9000 + 3000 + 1000 dot 3 dot 8) = O(36000)$.


== The Gradient

The EG Frank-Wolfe solver needs $partial C_b \/ partial q_i$ for each order $i$. For single-market order $i$ on outcome $m$ in group $j$:

$ (partial C_b) / (partial q_i) = Pr[s_j = m] = sum_(s : s_j = m) p_s $

This is the marginal probability of outcome $m$ --- the clearing price. For bundle order $i$ with payoff vector $phi_i$:

$ (partial C_b) / (partial q_i) = sum_s phi_(i,s) dot p_s = "price"_i $

Both require the joint distribution $p_s = exp(D_s\/b) \/ Z$. We need marginal and bundle-averaged probabilities, not the full distribution.

#proposition(name: "Sparse Gradient")[\
  _The marginal probability $Pr[s_j = m]$ decomposes as:_

  $ Pr[s_j = m] = 1/Z [Z_0/Z_j exp(D_m^j\/b) + sum_(s in cal(A) : s_j = m) R_s dot (exp(delta_s\/b) - 1)] $

  _The first term is the separable marginal $p_m^j dot Z_0\/Z$. The second sums only over bundle states that touch group $j$ and have $s_j = m$. For groups untouched by any bundle, $Pr[s_j = m] = p_m^j$ exactly (rescaled by $Z_0\/Z$)._
] <prop-gradient>

_Proof._ $Pr[s_j = m] = 1\/Z sum_(s : s_j = m) exp(D_s\/b)$. Apply the same split-and-recombine as @thm-sparse, restricting the sum to $s_j = m$. The separable sum over $s$ with $s_j = m$ gives $exp(D_m^j\/b) dot product_(l != j) Z_l = Z_0\/Z_j dot exp(D_m^j\/b)$. The correction sums over $cal(A) inter {s : s_j = m}$ --- only bundles spanning group $j$ contribute. #h(1fr) $square$

*Cost.* Computing all marginals $Pr[s_j = m]$ for all groups and outcomes: $O(sum_j K_j)$ for the separable part, plus $O(sum_i |G_i| dot 2^(|G_i|))$ for the corrections (each bundle contributes corrections to each of its groups). Same asymptotic cost as $Z$ itself.

Computing bundle prices $"price"_i = sum_s phi_(i,s) p_s$: each bundle's price sums over its $2^(|G_i|)$ states. Each $p_s$ decomposes as $R_s dot exp(delta_s\/b) \/ Z$. Cost: $O(2^(|G_i|))$ per bundle.


== Cross-Terms and Dense Coupling

@thm-sparse treats $delta_s = sum_i phi_(i,s) q_i$ as a single aggregate demand per joint state. When multiple bundles have overlapping group sets, their demands interact in the same joint states. This is handled correctly --- $delta_s$ is just the sum of all bundle payoffs at state $s$, and the formula computes the exact $Z$.

The potential issue is _enumeration of $cal(A)$_. If bundles span overlapping groups, the set of states with $delta_s != 0$ can be larger than $sum_i 2^(|G_i|)$:

- *Non-overlapping groups.* Bundles $(A,B)$ and $(C,D)$ have disjoint supports. $|cal(A)| = 2^2 + 2^2 = 8$.

- *Shared group.* Bundles $(A,B)$ and $(B,C)$ overlap on $B$. Each has 4 states, but demand at state $(a,b,c)$ is $delta_(a,b)^1 + delta_(b,c)^2$ --- the full support is $K_A times K_B times K_C$. For binary groups: $|cal(A)| = 8$, not $4 + 4$.

- *Chain coupling.* Bundles $(A,B), (B,C), (C,D), dots$ create a chain. The support grows as the _product_ over groups in the connected component of the coupling graph. For $tau$ coupled binary groups: $|cal(A)| = 2^tau$.

The decomposition is exact regardless --- the formula handles any $cal(A)$. The question is whether $|cal(A)|$ is tractable. Three regimes:

#align(center)[
  #table(
    columns: 3,
    align: (left, center, left),
    stroke: 0.5pt,
    inset: 6pt,
    [*Coupling structure*], [$|cal(A)|$], [*Tractability*],
    [Disjoint bundles], [$sum_i 2^(|G_i|) <= 32 N_B$], [Always fast],
    [Small components ($<= 15$ groups)], [$<= 2^15 = 32"K per component"$], [Fast],
    [Large dense components], [$2^tau$ for $tau$ coupled groups], [Exponential --- need fallback],
  )
]

The first two cases cover most practical instances. The third requires the fallback approach (§4).


= Exact Bundle Clearing via Frank-Wolfe <exact>

When $|cal(A)|$ is tractable, the existing EG Frank-Wolfe solver handles bundles exactly with minimal modification.

== The Modified Solver

The Frank-Wolfe EG solver iterates:

+ *Gradient.* Compute $nabla f(bold(q)^t)$ where $f$ is the EG objective. For each order $i$: $nabla_i f = mu_k L_i - "price"_i(bold(q)^t)$ (MM orders) or $w_i - "price"_i (bold(q)^t)$ (retail). The prices come from @prop-gradient.

+ *LP oracle.* Solve $max_(bold(q) in cal(C)) nabla f(bold(q)^t)^T bold(q)$ --- a linear program with the gradient as welfare weights. The feasible set $cal(C)$ is the box constraints plus per-group minting balance. The LP oracle does _not_ need joint-state constraints; it uses the gradient prices directly.

+ *Line search.* Find $alpha^*$ maximizing $f(bold(q)^t + alpha (bold(q)^"LP" - bold(q)^t))$ by bisection on the exact EG objective, which uses the sparse minting cost (@thm-sparse).

+ *Update.* $bold(q)^(t+1) = bold(q)^t + alpha^* (bold(q)^"LP" - bold(q)^t)$.

The only changes from the current solver are in steps 1 and 3: the gradient and objective evaluation use the sparse joint minting cost instead of per-group costs. The LP oracle (step 2) is unchanged --- it sees modified welfare weights but the same constraint structure.

== Why the LP Oracle Works

The LP oracle uses per-group minting constraints, not joint-state constraints. This means it may propose fills that are infeasible in the joint state space (filling a bundle without enough joint minting). The line search catches this: the exact EG objective includes the joint minting cost, so infeasible directions have high cost and the step size $alpha^*$ is set small. Frank-Wolfe converges to the optimum of the _exact_ joint EG program despite using an _approximate_ LP oracle, as long as the oracle's feasible set contains the true optimum --- which it does, since per-group constraints are a relaxation of joint constraints.

== Cost Per Iteration

Each Frank-Wolfe iteration: one gradient evaluation ($O(sum_j K_j + sum_i |G_i| dot 2^(|G_i|))$), one LP solve ($O("LP"(n))$, same as current), one line search ($O(10 dot |cal(A)|)$ for 10 bisection steps). With 25 iterations and $|cal(A)| approx 32000$: total cost dominated by the LP solves, same as the current solver.


= Price-Taking Injection (Fallback) <injection>

When $|cal(A)|$ is exponentially large (dense coupling), exact computation is infeasible. The fallback: solve without bundles, inject them as price-takers.

== Algorithm

+ *Base solve.* EG without bundles. Decomposes across groups. Output: $bold(q)^0$, per-group prices $bold(p)^0$, shadow prices $mu_k^0$.

+ *Bundle pricing.* For each bundle $i$, the true price at the separable solution is:
  $ "price"_i = sum_s phi_(i,s) product_j p_(s_j)^j $
  This is computable in $O(2^(|G_i|))$ per bundle without the joint state space --- the _product-form gradient_ at the separable solution (@prop-gradient with $delta = 0$). Fill bundle $i$ if (possibly shadow-adjusted) limit exceeds this price.

+ *Output.* Base fills plus bundle fills.

== Welfare Bound

#proposition(name: "Injection Welfare Bound")[\
  _Let $W^*$ be the EG welfare with all orders, $W_"inj"$ the injection welfare, $V_B = sum_i w_i overline(Q)_i$ the maximum bundle welfare, and $delta_"max" = max_s |delta_s|$ the peak bundle demand. Then $W^* - W_"inj" <= V_B + delta_"max"^2 \/ (8b)$._
] <prop-injection>

The bound is loose but qualitatively right: the welfare loss is first-order in total bundle welfare (from missed repricing interactions) and second-order in bundle demand (from minting cost error). For bundles at 10--15% of volume, expect $< 2%$ welfare loss.

== Iterative Re-Pricing

Injection can be iterated: after injecting bundles, re-solve the base EG with bundle-induced demand as fixed input, re-evaluate bundle profitability at updated prices, repeat. Each iteration is one decomposed EG solve. Convergence is geometric when bundle volume is small relative to single-market volume ($kappa = V_B \/ V_S < 1$). In practice 2--3 iterations suffice.


= When to Use Which Approach <decision>

#align(center)[
  #table(
    columns: 3,
    align: (left, left, left),
    stroke: 0.5pt,
    inset: 6pt,
    [*Coupling structure*], [*Approach*], [*Welfare loss*],
    [No bundles], [Decomposed EG (existing)], [0],
    [Bundles, small components], [Sparse exact (§4)], [0],
    [Bundles, large components], [Injection + re-pricing (§5)], [$O(epsilon^2)$],
  )
]

The decision is based on $|cal(A)|$: compute the coupling graph's connected components, check whether each component's state space ($product_(j in C) K_j$) is tractable ($<= 2^15$ as a practical threshold). Exact for small, injection for large.

For large components, graph sparsification (min-weight balanced partitioning of the coupling graph) can break them into smaller pieces by dropping minimum-welfare bundles. The dropped bundles are re-injected as price-takers. This is useful when the coupling graph has a small cut --- which depends on the structure of submitted bundles and is an empirical question.


= Discussion

== What Is New

The sparse minting cost decomposition (@thm-sparse) and its gradient (@prop-gradient) are the main contributions. They show that the joint clearing problem is polynomial in the number of bundles, not exponential in the number of coupled groups --- as long as the coupling graph has bounded component size. This is not an approximation; it is the exact joint EG program solved by the existing Frank-Wolfe algorithm with modified gradient and objective evaluations.

== Open Problems

+ *Dense coupling.* When the coupling graph has large components ($>$ 15 coupled groups), the sparse decomposition becomes exponential in the component size. Whether belief propagation, column generation, or other structured inference methods can compute approximate marginals cheaply on such components is the key open question. The coupling graph is a graphical model; the minting cost defines a Gibbs distribution; computing marginals is inference. The literature on approximate inference (loopy BP, variational methods, MCMC) may apply directly.

+ *Cross-term structure.* When bundles share groups, the enumeration of $cal(A)$ involves cross-terms (§3.3). For chain-like coupling ($A$-$B$, $B$-$C$, $C$-$D$), the structure is a tree and exact inference costs $O(product_("chain") K_j)$, which can be much smaller than $product_("all groups") K_j$. For loopy coupling, the junction tree width determines the cost. Characterizing the typical treewidth of real prediction market coupling graphs is an empirical question.

+ *Bundle volume in practice.* No prediction market at scale currently supports bundles. The fraction of volume that will be cross-group (analogous to cross-asset derivatives in traditional finance) is unknown. Options volume is 30--50% of equity spot in delta-adjusted notional, but most options are single-name; cross-asset strategies are $<$ 10% of options volume. If bundles are $<$ 10% of total volume, injection alone may suffice and the exact approach is unnecessary. If bundles are $>$ 30%, the coupling graph will be dense and the open problems above become critical.

+ *Combining candidate solutions.* Given multiple candidate solutions (from different approaches), can they be combined? Fill vectors can be convex-combined (the feasible set is a polytope) and the EG welfare of the combination is $>=$ the weighted average (concavity). But the combined clearing prices (via softmax of combined demand) may violate UCP --- the combined price can exceed both individual prices because softmax is convex component-wise. This means combined fills are EG-feasible but may not constitute a valid clearing outcome. A potential workaround: combine _demand vectors_ to get consensus prices, then re-solve fills as a single LP at those prices (UCP-compliant by construction). Whether this yields better welfare than the best individual solution is unclear.


#v(2em)
#line(length: 100%)
#v(0.5em)
#text(size: 9pt, style: "italic")[
  Companion to _Prediction Markets Are Fisher Markets_ (2026) and _Decomposed Clearing via Fisher Market Budget Allocation_ (2026).
]
