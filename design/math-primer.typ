#set document(title: "Mathematical Primer for 'Prediction Markets Are Fisher Markets'")
#set text(font: "New Computer Modern", size: 10pt)
#set page(margin: (x: 1.5in, y: 1.2in), numbering: "1")
#set par(justify: true, leading: 0.55em)
#set heading(numbering: "1.")
#show heading.where(level: 1): it => block(above: 1.5em, below: 0.8em)[#it]
#show heading.where(level: 2): it => block(above: 1.2em, below: 0.6em)[#it]

#align(center)[
  #text(size: 15pt, weight: "bold")[Mathematical Primer]
  #v(0.3em)
  #text(size: 11pt)[For "Prediction Markets Are Fisher Markets"]
  #v(0.3em)
  #text(size: 9pt, style: "italic")[Short on background, long on the reduced-form core]
]

#v(1em)

#block(inset: (x: 2em, y: 0.7em), fill: luma(245), radius: 4pt)[
  *Purpose.* This primer strips the paper down to the minimum math needed to read it confidently. Sections 1--4 compress the background that used to take many pages. Sections 5--6 spend the time on the paper's real center of gravity: reduced-form MM utility, deployed value, the price dual, and the computation story.

  #v(0.4em)
  #align(center)[
    #table(
      columns: 2,
      align: (left, left),
      stroke: 0.5pt,
      inset: 6pt,
      [*Section*], [*Main takeaway*],
      [1--2], [LP clearing, simplex prices, LMSR smoothing, and the KKT logic behind uniform clearing],
      [3--4], [Why hard MM budgets break convexity, and why diminishing returns is the right fix],
      [5], [The key object is $psi_B(U)$: affine below budget, logarithmic above it],
      [6], [The deployed-value lift gives a clean conic solver form and a decomposition story],
    )
  ]
]

= Batch Clearing in One Page

A batch auction chooses fills $bold(q)$ for all orders at once. The objective is simple: create as much surplus as possible, but pay for any new shares that must be minted.

== Minting Cost

With $K$ mutually exclusive outcomes, minting one complete set of shares costs $\$1$. If the fills imply net demand $D_k(bold(q))$ for outcome $k$, then the exchange must mint at least enough complete sets to cover the largest shortfall. The clearing LP is therefore

$ max_(bold(q) in cal(C)) quad sum_i w_i q_i - max_k D_k(bold(q)) $

where $cal(C)$ is the usual box-plus-balance polytope. The first term is welfare from fills; the second is the minting bill.

The important point is conceptual: $max_k D_k$ is not an arbitrary penalty. It is the exact cost of endogenous supply under mutual exclusivity.

== Why Prices Live on the Simplex

The convex conjugate of $max_k D_k$ is the indicator of the simplex:

$ (max_k D_k)^*(bold(p)) = delta_Delta(bold(p)) $

So the only price vectors that avoid minting arbitrage are probability vectors. In this framework, “prices sum to one” is a consequence of the minting technology, not a separate assumption.

== LMSR Is the Smoothed Version

Replace $max_k D_k$ by the log-sum-exp smoothing

$ C_b(bold(D)) = b ln sum_k exp(D_k / b) $

Then the conjugate becomes negative entropy:

$ C_b^*(bold(p)) = b sum_k p_k ln p_k $

and the gradient is the familiar softmax rule

$ p_k = (partial C_b) / (partial D_k) = exp(D_k / b) / sum_j exp(D_j / b) $

So LMSR is not a different mechanism layered on top of batch clearing. It is the entropically smoothed version of the same clearing problem.

== What the Temperature $b$ Does

The smoothing error is explicit:

$ max_k D_k <= C_b(bold(D)) <= max_k D_k + b ln K $

So $b ln K$ is the worst-case subsidy paid for price uniqueness and smoothness. Small $b$ makes the market close to the LP. Positive $b$ also makes the price dual strictly convex, which is why smoothed prices are unique.

= KKT and Clearing Logic

Uniform clearing is just KKT conditions in disguise.

If outcome $m$ clears at price $p_m$, then a buy order with limit price $L_i$ behaves as follows:

- if $L_i > p_m$, it wants to fill up to its cap;
- if $L_i < p_m$, it should not fill;
- if $0 < q_i < overline(Q)_i$, then necessarily $L_i = p_m$.

That is exactly the usual call-auction logic. The only twist later in the paper is that a budget-constrained MM does not compare $L_i$ directly to price. It compares $alpha_k L_i$ to price, where $alpha_k <= 1$ is a scarcity factor coming from the MM's reduced-form utility.

= Why Hard Budgets Break Convexity

The naive risk-neutral model keeps linear MM welfare but adds a budget cap. The problem is that spending depends on both fills and prices:

$ sum_(i in "MM"_k) p_(m(i)) q_i <= B_k $

Since prices themselves come from the clearing program, this is bilinear in the primal variables and the dual variables. In general the feasible set is nonconvex.

That is the real obstruction in the paper. Prediction-market clearing is not intrinsically nonconvex. The pathology comes from combining:

- linear MM welfare, and
- hard budget caps written directly in price times quantity form.

Once you see that, the natural question is not “how do we optimize this nonconvex program anyway?” but “what is the right MM objective if capital is genuinely scarce?”

= Why Diminishing Returns Is the Right Fix

The paper does not need a grand behavioral theorem saying every real market maker literally solves Kelly. It only needs a defensible reason to move away from linear welfare when capital is scarce.

Three points matter:

- A hard budget already means the MM is not truly risk neutral. If one extra dollar of fill were always worth one extra dollar of welfare, there would be no internal reason to stop at $B_k$.
- Repeated-betting models make log utility the canonical growth-optimal benchmark.
- Real systems use ladders, position limits, and other sizing rules that encode diminishing returns in practice.

That motivates the resolution: model a budgeted MM as having a concave reduced-form utility. Section 5 derives the exact one induced by retained cash.

= Reduced-Form Utility and Fisher-Market Clearing

This section is the heart of the paper. The central object is not the deployed-value variable $V_k$ itself. The central object is the reduced-form MM utility $psi_B(U)$ that $V_k$ reveals.

== The Four Quantities To Track

For each market maker $k$:

- $U_k(bold(q)) = sum_(i in "MM"_k^+) L_i q_i$ is the MM's weighted fill value.
- $s_k >= 0$ is retained cash.
- $V_k = U_k + s_k$ is total deployed value: fills plus retained cash.
- $alpha_k = psi_(B_k)'(U_k)$ is the capital-scarcity factor.

These play different roles.

#align(center)[
  #table(
    columns: 3,
    align: (left, left, left),
    stroke: 0.5pt,
    inset: 6pt,
    [*Object*], [*Meaning*], [*Why it matters*],
    [$U_k$], [Fill value], [How much outcome exposure the MM is trying to buy],
    [$V_k$], [Deployed value], [The clean lift for proofs, KKT, and conic solvers],
    [$psi_B(U)$], [Reduced-form utility], [The conceptual payload: affine when slack, log when binding],
    [$alpha_k$], [Marginal scarcity], [Rescales MM limit prices in the KKT system],
  )
]

== Deriving the Reduced Form

Fix one MM with budget $B$. If its fill value is $U$, the lifted objective optimizes over deployed value:

$ psi_B(U) = max_(V >= U) [B ln V - V + U] $

This is a one-variable concave problem. The derivative is $B / V - 1$, so the unconstrained maximizer is $V = B$. The constraint $V >= U$ clips that at

$ V^*(U) = max(U, B) $

Substituting back gives the exact reduced form:

$ psi_B(U) = cases(
  U + B ln B - B & "if" U <= B, 
  B ln U & "if" U >= B
) $

This is the key structural fact in the paper.

- *Slack budget:* $U <= B$. The utility is affine, with slope $1$.
- *Binding budget:* $U >= B$. The utility becomes logarithmic, with slope $B / U$.

So a budgeted MM behaves like the LP while it has spare capital, and like a log-utility agent once capital binds.

#block(inset: (x: 1.6em, y: 0.6em), fill: luma(245), radius: 4pt)[
  *Tiny numerical example.* Take one MM with budget $B = 10$.

  #v(0.4em)
  #align(center)[
    #table(
      columns: 6,
      align: (left, center, center, center, center, center),
      stroke: 0.5pt,
      inset: 6pt,
      [*Case*], [*$U$*], [*$V^* = max(U, B)$*], [*$psi_B(U)$*], [*$alpha = psi_B'(U)$*], [*$g_B(U)$*],
      [Slack], [$6$], [$10$], [$6 + 10 ln 10 - 10 approx 19.03$], [$1$], [$0$],
      [Binding], [$16$], [$16$], [$10 ln 16 approx 27.73$], [$10 / 16 = 0.625$], [$16 - 10 - 10 ln(16 / 10) approx 1.30$],
    )
  ]

  #v(0.4em)
  In the slack case, the MM leaves $4$ units as cash, behaves exactly like the LP at the margin ($alpha = 1$), and pays no envelope penalty ($g_B = 0$). In the binding case, there is no retained cash, the MM discounts its marginal willingness to pay to $0.625$, and the gap $g_B$ records how far the reduced-form utility falls below the affine LP benchmark.
]

== Why $psi_B$ Is the Right Abstraction

Two properties make $psi_B$ especially useful.

First, it is concave and $C^1$. The derivative is

$ psi_B'(U) = min(1, B / U) $

This derivative is the scarcity factor $alpha_k$. It is the MM's marginal willingness to turn one more unit of capital into one more unit of weighted fill.

Second, $psi_B$ sits under its affine envelope:

$ psi_B(U) <= U + B ln B - B $

with equality exactly on the slack-budget region $U <= B$. That single inequality explains LP recovery and the welfare-gap bound later.

== The Reduced-Form Clearing Program

After substituting $psi_(B_k)$ for each MM, the whole batch auction becomes

$ max_(bold(q) in cal(C)) quad sum_k psi_(B_k)(U_k(bold(q))) + sum_(j in.not "MM") w_j q_j - C_b(bold(D)(bold(q))) $

This is the paper's main primal program.

Read it as:

- MMs contribute concave welfare through $psi_B$;
- retail orders stay linear;
- minting cost stays exactly the same.

The geometry is now clean:

- the feasible set $cal(C)$ is convex;
- each $psi_(B_k)(U_k(bold(q)))$ is concave in $bold(q)$ because $U_k$ is linear and $psi_B$ is concave;
- $-C_b(bold(D)(bold(q)))$ is concave because $C_b$ is convex.

So the whole program is a concave maximization over a convex feasible set.

== Why the Deployed-Value Lift Still Matters

Even though $psi_B$ is the main object, the lifted variable $V_k$ is still the best proof device:

$ max_(bold(q) in cal(C), bold(V) >= bold(U)(bold(q))) quad sum_k [B_k ln V_k - V_k + U_k(bold(q))] + sum_(j in.not "MM") w_j q_j - C_b(bold(D)(bold(q))) $

This lift is equivalent MM-by-MM to the reduced form because each bracketed term is exactly the definition of $psi_(B_k)(U_k)$ after optimizing over $V_k$.

Why keep the lift around?

- It exposes the KKT system cleanly.
- It makes budget absorption transparent.
- It is the form that conic solvers actually want.

So the right split is:

- $psi_B$ for exposition and theorem statements,
- $V_k$ for derivation and implementation.

== The KKT Story: Exact Limit Orders with Scarcity

Let $p_m = (partial C_b)/(partial D_m)$ be the clearing price of outcome $m$. For an MM order $i$ belonging to MM $k$, the KKT condition becomes

$ alpha_k L_i <= p_(m(i)) + lambda_i^- - lambda_i^+ $

where $lambda_i^-$ and $lambda_i^+$ are the lower- and upper-bound multipliers for $q_i in [0, overline(Q)_i]$.

The interpretation is the same as in an ordinary call auction, except that the order's effective value is $alpha_k L_i$ instead of $L_i$.

- If $q_i > 0$, then $alpha_k L_i >= p_(m(i))$.
- If $0 < q_i < overline(Q)_i$, then $alpha_k L_i = p_(m(i))$.
- If $alpha_k L_i < p_(m(i))$, the order should not fill.

So limit orders remain exact. Budget scarcity does not blur the price rule; it only rescales the MM's willingness to pay.

== Two Regimes for a Market Maker

The scarcity factor

$ alpha_k = psi_(B_k)'(U_k) = min(1, B_k / U_k) $

divides the MM into two regimes.

#block(inset: (x: 1.6em, y: 0.6em), fill: luma(245), radius: 4pt)[
  *Slack regime: $U_k < B_k$.* Then $alpha_k = 1$. The MM behaves exactly like the risk-neutral LP. Every profitable order is compared to price without discounting.

  *Binding regime: $U_k > B_k$.* Then $alpha_k = B_k / U_k < 1$. The MM's lower-value orders are throttled first, because only orders with sufficiently high $L_i$ remain worth filling after the $alpha_k$ discount.
]

This is the economic content of the model. The MM is LP-like until capital becomes scarce; after that, capital is allocated to the highest-return fills.

== Budget Absorption Without an Explicit Budget Constraint

A key feature of the paper is that the reduced-form program has no explicit spending constraint, yet the optimum still respects the budget.

The reason is that the KKT inequality implies

$ sum_(i in "MM"_k^+) p_(m(i)) q_i <= alpha_k U_k $

Now split by regime.

- If $U_k <= B_k$, then $alpha_k = 1$ and $V_k = B_k$, so retained cash is $V_k - U_k = B_k - U_k$. Total capital used is

  $ sum_i p_(m(i)) q_i + (V_k - U_k) <= U_k + (B_k - U_k) = B_k $

- If $U_k >= B_k$, then $alpha_k = B_k / U_k$ and $V_k = U_k$, so retained cash is zero. Total capital used is

  $ sum_i p_(m(i)) q_i <= alpha_k U_k = B_k $

So the budget is absorbed into the objective. That is why the paper can delete the explicit hard cap without losing economic discipline.

== Price Dual and the Role of Temperature

To study prices, project the primal into demand space. Define $W^"RA"(bold(D))$ as the best risk-averse welfare achievable with net demand $bold(D)$. Because it is the value function of a concave program with linear coupling constraints, $W^"RA"$ is concave.

Then the clearing prices solve the dual problem

$ min_(bold(p) in Delta) quad (W^"RA")^*(bold(p)) + b sum_k p_k ln p_k $

This is the exact analogue of the LP/LMSR dual from Section 1, but with the reduced-form MM welfare folded into the conjugate.

The temperature split is now clean.

#align(center)[
  #table(
    columns: 3,
    align: (left, left, left),
    stroke: 0.5pt,
    inset: 6pt,
    [*Quantity*], [*$b > 0$*], [*$b = 0$*],
    [Minting cost], [$C_b = b ln sum exp(D_k / b)$], [$max_k D_k$],
    [Prices], [Softmax gradient], [LP dual variables],
    [Price uniqueness], [Yes: entropy makes the dual strictly convex], [Not guaranteed],
    [Existence / KKT / budget absorption], [Yes], [Yes],
  )
]

So positive temperature buys a unique price vector. Zero temperature keeps the same primal structure but loses the entropic tie-break.

== Affine Envelope: LP Recovery and Welfare Loss

Define the envelope gap

$ g_B(U) = U + B ln B - B - psi_B(U) = cases(
  0 & "if" U <= B,
  U - B - B ln(U / B) & "if" U >= B
) $

This measures how far the MM is from the affine LP regime.

Two consequences become immediate.

- *LP recovery.* If an LP optimum has $U_k <= B_k$ for every MM, then $g_(B_k)(U_k) = 0$ for every MM, so the LP optimum is also optimal for the reduced-form program.
- *Welfare gap.* If the LP would overuse some MM's capital, the reduced-form welfare differs from LP welfare only through the sum of these gap terms. The exact loss for MM $k$ with shortfall $Delta_k = max(0, U_k^"LP" - B_k)$ is

  $ Delta_k - B_k ln(1 + Delta_k / B_k) <= Delta_k^2 / (2 B_k) $

This is why the risk-averse solution usually stays close to the LP in practice: only the capital-binding region is penalized, and the penalty is second-order for small shortfalls.

== Why the Title Mentions Fisher Markets

A quasi-linear Fisher market has:

- agents with budgets,
- divisible goods,
- concave utility over goods,
- optional retained cash,
- prices as dual variables.

The reduced-form clearing program has the same structure.

#align(center)[
  #table(
    columns: 3,
    align: (left, center, center),
    stroke: 0.5pt,
    inset: 6pt,
    [*Component*], [*Quasi-linear Fisher market*], [*Batch auction here*],
    [Agents], [Consumers], [Budgeted market makers],
    [Goods], [Divisible commodities], [Outcome shares],
    [Budgets], [$B_k$], [$B_k$],
    [Cash], [Unspent budget], [Retained cash $s_k$],
    [Utility], [Concave in allocations], [$psi_(B_k)(U_k)$],
    [Prices], [Dual variables], [Clearing prices],
  )
]

The two extensions specific to prediction markets are:

- supply is endogenous through minting cost instead of fixed endowment;
- retail orders contribute linear welfare alongside MM utility.

That is why the title is right, but also why the reduced form matters more than the slogan: the Fisher interpretation is a consequence of the utility structure.

== Buy/Sell Reduction and Multiple Groups

Two implementation notes matter.

- *Buy/sell reduction.* MM sells are netted against buys. A net short in one outcome is equivalent to a net buy in the complement, so the theory can be written using net MM buy exposure only.
- *Multiple groups and bundles.* Nothing in the reduced-form argument depends on having one mutually exclusive group. If the state space is joint, simply replace $k$ by joint state $s$ in the demand vector and minting cost. The structure survives; the problem becomes computational rather than conceptual.

= Computation and Decomposition

Section 6 of the paper is about turning the theory into solvers.

== Why Solvers Want the Lifted Form

The reduced form $psi_B(U)$ is excellent for theory, but the lifted form is better for optimization. At zero temperature, the program is

$ max quad sum_k [B_k ln V_k - V_k + U_k] + "linear terms" - M $

subject to linear constraints plus $V_k >= U_k$ and the minting epigraph constraints $M >= D_k$.

After the lift, the only nonlinear pieces are the $ln V_k$ terms. That is exactly what lets the problem fit standard conic machinery.

== Conic Formulation in Words

Introduce variables $t_k$ with

$ t_k <= ln V_k $

Then each MM contribution becomes linear in $(t_k, V_k, U_k)$:

$ B_k t_k - V_k + U_k $

The constraint $t_k <= ln V_k$ is a standard exponential-cone constraint. Everything else is linear. So the whole $b = 0$ program becomes an exponential-cone program.

This matters for two reasons.

- It gives a direct polynomial-time solvability statement using standard conic optimization.
- It shows that the “hard part” of the model is very structured: the only nonlinearity is one log term per MM.

== Augmented-LP Intuition

The conic view is the formal complexity result. The engineering intuition is even simpler: compared with the clearing LP, each MM contributes just one extra scalar nonlinear variable $V_k$ and one log barrier term.

So in sparse order books with few budgeted MMs, the lifted program should behave much more like an LP with a low-rank correction than like a fundamentally different optimization problem. That is why the paper's practical solver story is plausible.

== When Decomposition Helps

Suppose the market has many groups. If no order spans multiple groups, then the smoothed minting cost factorizes across groups. The monolithic program splits into independent subproblems, except that each MM's total budget must still be allocated across the components.

With reduced-form utility, this allocation is smooth. If MM $k$ splits its budget across components $m$, the coordination gradient is

$ partial W_m^* / partial B_k^m = ln V_k^(m *) $

So a first-order method such as mirror descent on budget shares is natural: solve each component independently, read off $ln V_k^(m *)$, and update the budget split.

This is a structural payoff of the reduced form. Under hard budget constraints with linear welfare, there is no comparable smooth coordination layer.

== When Decomposition Does Not Help

Current small benchmarks are mostly independent binary markets. In that regime, monolithic LP clearing is already cheap, so decomposition pays coordination overhead without getting much in return.

That is why the paper reports the following qualitative pattern:

- decomposed LP is close in welfare to monolithic LP;
- decomposed conic currently suffers from numerical issues when component budgets become tiny;
- monolithic methods remain faster on small independent instances.

This is not a contradiction. It just means the benchmark regime is not the regime decomposition is for.

== The Regime Decomposition Is For

Decomposition becomes valuable when bundle orders create an exponential joint state space but the coupling graph still splits into modest connected components.

A simple picture:

- monolithic clearing over $20$ binary groups sees $2^20 approx 10^6$ joint states;
- if the coupling graph splits into four components of five groups each, componentwise clearing sees only $4 dot 2^5 = 128$ states, plus a smooth coordination layer over MM budgets.

That is the asymptotic regime where decomposition should win. The current theory already points there; the empirical program still needs benchmarks that live there.

= Mental Model

If you remember only four lines from the paper, remember these.

- *Baseline:* batch clearing is LP welfare minus minting cost.
- *Obstacle:* linear MM welfare plus hard budgets makes the problem nonconvex.
- *Fix:* with retained cash, a budgeted MM has reduced-form utility $psi_B(U)$, affine when slack and logarithmic when binding.
- *Consequence:* the auction becomes a concave Fisher-style program with exact KKT logic, unique prices for $b > 0$, and a clean conic solver form.
