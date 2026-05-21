#set document(title: "Conic Reformulation of Quasi-Linear EG Clearing")
#set text(font: "New Computer Modern", size: 10pt)
#set page(margin: (x: 1.5in, y: 1.2in), numbering: "1")
#set par(justify: true, leading: 0.55em)
#set heading(numbering: "1.")
#show heading.where(level: 1): it => block(above: 1.5em, below: 0.8em)[#it]
#show heading.where(level: 2): it => block(above: 1.2em, below: 0.6em)[#it]

#align(center)[
  #text(size: 15pt, weight: "bold")[
    Conic Reformulation of \ Quasi-Linear EG Clearing
  ]
  #v(0.5em)
  #text(size: 11pt)[Interior Point Solving via Exponential Cones]
  #v(0.3em)
  #text(size: 9pt, style: "italic")[Draft — February 2026]
]

#v(1em)

#block(inset: (x: 2em))[
  #text(weight: "bold")[Summary.]
  The Eisenberg-Gale clearing problem — a linear program augmented with $K$ logarithmic utility terms — admits two efficient interior point formulations. The _conic reformulation_ introduces $K$ exponential cone constraints and hands off to Clarabel.rs, achieving a clean single-solve approach. The _augmented LP_ reformulation reveals a deeper insight: the EG Hessian is a rank-$K$ perturbation of the LP's diagonal barrier, so Newton's method on the EG problem has the same per-iteration cost as LP interior point. Both replace the current Frank-Wolfe approach (25 LP oracle calls) with a single interior-point solve, targeting 10–25$times$ speedup.
]

#v(1em)


= The Quasi-Linear EG Program

The risk-averse batch clearing program (companion paper, Theorem 5):

$
P^"RA": quad max_(bold(q) >= 0, bold(s) >= 0) quad underbrace(sum_k [B_k ln(V_k) - s_k], "MM log utility") + underbrace(sum_(j in.not "MM") w_j q_j, "retail welfare") - underbrace(N dot sum_m "mint"_m + N dot sum_g "gmint"_g, "minting cost")
$

where:
- $V_k = U_k + s_k = sum_(i in "MM"_k) L_i q_i + s_k$ is MM $k$'s total value (fill utility plus retained cash)
- $L_i = "sign"_i dot "limit"_i$ is the welfare weight of order $i$
- $N = 10^9$ (nanos per dollar)
- $s_k >= 0$ is cash retained by MM $k$ (quasi-linear extension)

Subject to:
- *Position balance* (per market $m$): $ sum_i c_(y,i,m) q_i = "mint"_m + "gmint"_(g(m)), quad sum_i c_(n,i,m) q_i = "mint"_m $
- *Box constraints*: $0 <= q_i <= overline(q)_i$, $s_k >= 0$, $"gmint"_g >= 0$, $"mint"_m$ free.

*Observation 1 (structure).* The objective is concave: $B_k ln(V_k)$ is concave in $(bold(q), s_k)$ (perspective of $ln$ preserves concavity). All constraints are linear. This is a _linearly constrained concave maximization_ — the natural domain of interior point methods.

*Observation 2 (LP reduction).* Without the $ln$ terms (i.e., $B_k = 0$ for all $k$, or equivalently no MMs), the program reduces to the LP solved by `lp_solver.rs`. The $K$ log terms are the _only_ source of nonlinearity.

*Observation 3 (cash variable).* The KKT condition for $s_k$ gives $B_k \/ V_k <= 1$, with equality when $s_k = 0$. At optimum:
$ V_k = max(U_k, B_k) $
Over-capitalized MMs park $s_k = B_k - U_k > 0$ as cash, preventing budget distortion of fills. The program automatically recovers LP behavior when budgets don't bind.


= Conic Reformulation (Clarabel) <conic>

Clarabel solves conic programs in standard form:

$ min_(bold(x)) quad 1/2 bold(x)^T P bold(x) + bold(c)^T bold(x) quad "s.t." quad A bold(x) + bold(s) = bold(b), quad bold(s) in cal(K) $

where $cal(K) = K_1 times K_2 times dots$ is a Cartesian product of supported cones.


== Variables

The decision vector $bold(x) in RR^d$ with $d = n + K + K + M + G$:

#align(center)[
  #table(
    columns: 4,
    align: (center, left, center, left),
    stroke: none,
    [*Symbol*], [*Meaning*], [*Count*], [*Bounds*],
    [$q_i$], [Fill quantity, order $i$], [$n$], [$[0, overline(q)_i]$],
    [$s_k$], [Retained cash, MM $k$], [$K$], [$>= 0$],
    [$t_k$], [Log-utility epigraph], [$K$], [free],
    [$"mint"_m$], [Per-market minting], [$M$], [free],
    [$"gmint"_g$], [Group minting], [$G$], [$>= 0$],
  )
]


== Objective

Minimize (negated welfare):

$ bold(c)^T bold(x) = -sum_k B_k t_k + sum_k s_k - sum_(j in.not "MM") w_j q_j + N dot sum_m "mint"_m + N dot sum_g "gmint"_g $

No quadratic term: $P = 0$. The curvature lives entirely in the cone constraints.


== Exponential Cone Constraints

For each MM $k$, the constraint $t_k <= ln(V_k)$ is modeled via the exponential cone:

$ cal(K)_"exp" = {(x, y, z) : y exp(x\/y) <= z, quad y >= 0} $

Setting $(s_1, s_2, s_3) = (t_k, 1, V_k)$ gives $exp(t_k) <= V_k$, i.e., $t_k <= ln V_k$. #sym.checkmark

In Clarabel's slack form $A bold(x) + bold(s) = bold(b)$, each MM $k$ produces three rows:

$
mat(
  -bold(e)_(t_k);
  bold(0);
  -bold(L)_k - bold(e)_(s_k)
) bold(x) + vec(s_1, s_2, s_3) = vec(0, 1, 0)
$

where $bold(e)_(t_k)$ selects the $t_k$ variable and $bold(L)_k$ has $L_i$ at position $i$ for each $i in "MM"_k$.


== Linear Constraints

*Position balance* (zero cone): For each market $m$, two equality rows. $2M$ rows total.

*Box constraints* (nonnegative cone): Upper bounds $overline(q)_i - q_i >= 0$, lower bounds $q_i >= 0$, cash $s_k >= 0$, group minting $"gmint"_g >= 0$. Also $"mint"_m$ is free (no cone constraint).


== Full Cone Specification

$ cal(K) = underbrace(cal(K)_"exp"^K, "log utility") times underbrace(cal(K)_0^(2M), "pos. balance") times underbrace(cal(K)_(>=)^(2n + K + G), "box + nonneg") $

Total slack dimension: $3K + 2M + 2n + K + G$. Number of exponential cones: $K$ (typically 1–10).

The entire problem is specified by two sparse matrices ($A$, $P = 0$), two vectors ($bold(b)$, $bold(c)$), and a cone list. No callbacks, no oracles, no iteration.


== Price Extraction

Clearing prices come from the duals of position balance constraints, identical to the LP:
$ p_("YES", m) = |lambda_("YES", m)|, quad p_("NO", m) = |lambda_("NO", m)| $

where $lambda$ are the dual variables (Clarabel's `solution.z` for the zero-cone block). When minting is active, $p_"YES" + p_"NO" = N$ by stationarity of the mint variable.


= The Augmented-LP Insight <augmented-lp>

The conic reformulation is clean but treats Clarabel as a black box. A deeper perspective reveals that the EG problem is a _minimal perturbation_ of the LP.


== Hessian Structure

The EG objective $f(bold(q), bold(s)) = sum_k [B_k ln(V_k) - s_k] + sum_(j in.not "MM") w_j q_j - ("minting cost")$ has Hessian:

$ nabla^2_(q_i q_j) f = -sum_k B_k / V_k^2 dot L_(k,i) dot L_(k,j) $

where $L_(k,i)$ is the welfare weight of order $i$ for MM $k$ ($L_i$ if $i in "MM"_k$, else 0). In matrix form:

$ H_"EG" = -sum_(k=1)^K B_k / V_k^2 dot bold(ell)_k bold(ell)_k^T $

*This Hessian has rank at most $K$.* Each $bold(ell)_k in RR^n$ has nonzeros only for MM $k$'s orders. For typical batches ($K = 1$–$10$ MMs, $n = 100$–$10000$ orders), the Hessian is trivially cheap to form and apply.

The cross-derivatives with cash variables $s_k$ extend this cleanly. Define $tilde(bold(ell))_k in RR^(n+K)$ as $bold(ell)_k$ extended with a $1$ in position $n + k$. Then the full Hessian over $(bold(q), bold(s))$ is:

$ H = -sum_(k=1)^K B_k / V_k^2 dot tilde(bold(ell))_k tilde(bold(ell))_k^T $

Still rank-$K$, still negative semidefinite.


== Newton's Method = Modified LP Interior Point

A standard LP interior-point solver minimizes the barrier subproblem $-bold(c)^T bold(q) + mu sum_i [-ln(overline(q)_i - q_i) - ln(q_i)]$, solving the KKT system at each step:

$ mat(D_"LP", A^T; A, 0) vec(Delta bold(q), Delta bold(lambda)) = vec(bold(r)_1, bold(r)_2) $

where $D_"LP" = "diag"(1\/q_i^2 + 1\/(overline(q)_i - q_i)^2)$ is the barrier Hessian (diagonal, positive definite).

For EG, the KKT system becomes:

$ mat(D_"LP" + Sigma, A^T; A, 0) vec(Delta bold(q), Delta bold(lambda)) = vec(bold(r)_1, bold(r)_2) $

The only change: $D_"LP"$ gains the rank-$K$ correction

$ Sigma = -H_"EG" = sum_k B_k / V_k^2 dot bold(ell)_k bold(ell)_k^T $

which is positive semidefinite (so the system stays positive definite).


== Efficient Solve via Woodbury

The normal equations (Schur complement) are:

$ A (D_"LP" + Sigma)^(-1) A^T Delta bold(lambda) = bold(r) $

By Sherman–Morrison–Woodbury:

$ (D_"LP" + L L^T)^(-1) = D^(-1) - D^(-1) L (I_K + L^T D^(-1) L)^(-1) L^T D^(-1) $

where $L in RR^(n times K)$ stacks the scaled $sqrt(B_k)\/V_k dot bold(ell)_k$ vectors. The inner matrix $I_K + L^T D^(-1) L$ is $K times K$ — trivially invertible. Each Newton step costs:

- $O(n)$: invert the diagonal $D$ (same as LP)
- $O(n K)$: form $D^(-1) L$ and the $K times K$ inner product
- $O(K^3)$: invert the $K times K$ inner matrix

For $K << n$, this is dominated by $O(n)$ — *the same cost as one LP interior-point iteration.*


== Iteration Count

Self-concordant barrier theory: an interior-point method converges in $O(sqrt(nu))$ iterations where $nu$ is the barrier parameter (number of inequality constraints). The EG problem has the same constraints as the LP plus $K$ log terms — but these $K$ log terms are themselves self-concordant barriers. So $nu_"EG" = nu_"LP" + K$, and the iteration count is $O(sqrt(n + K)) approx O(sqrt(n))$.

*The EG solver has the same asymptotic complexity as the LP solver.*


= Performance Analysis

== Problem Dimensions (Typical)

#align(center)[
  #table(
    columns: 4,
    align: (left, center, center, center),
    stroke: none,
    [*Parameter*], [*Symbol*], [*Small*], [*Large*],
    [Orders], [$n$], [100], [10,000],
    [Markets], [$M$], [20], [200],
    [Groups], [$G$], [5], [50],
    [Market makers], [$K$], [2], [10],
    [Variables], [$d$], [$~$130], [$~$10,300],
    [Constraints], [], [$~$50], [$~$500],
    [Exp. cones], [], [2], [10],
  )
]

== Expected Speedup

#align(center)[
  #table(
    columns: 4,
    align: (left, center, center, center),
    stroke: none,
    [*Method*], [*Solves*], [*Per-solve cost*], [*Total*],
    [Frank-Wolfe (current)], [25 LPs], [$O_"LP"$], [$25 O_"LP"$],
    [Clarabel (conic)], [1], [$~3$–$5 O_"LP"$], [$3$–$5 O_"LP"$],
    [Augmented LP (custom)], [1], [$~1.1 O_"LP"$], [$~1.1 O_"LP"$],
  )
]

- *Frank-Wolfe* (current `eg_solver.rs`): 25 iterations, each solving a full LP via HiGHS. Total: $25 times O_"LP"$.
- *Clarabel*: Generic conic interior-point solver. Handles exponential cones natively but carries overhead vs. LP-specialized solvers (general-purpose KKT factorization, no LP presolve). Estimated $3$–$5 times$ per-solve cost of HiGHS. Net speedup: *5–8$times$*.
- *Augmented LP*: Custom interior-point method that reuses the LP's constraint structure and adds the rank-$K$ Hessian correction. Per-iteration cost identical to LP interior point. Net speedup: *$~$20$times$*, approaching LP parity.

The Clarabel path is straightforward to implement (days of work). The augmented-LP path requires a custom interior-point solver (weeks) but achieves near-LP speed.


= Implementation Plan

== Phase 1: Clarabel (immediate)

+ Add `clarabel` dependency (feature-gated behind `eg-conic`).
+ Build the conic program from the same data structures as `build_and_solve_lp()`. Reuse `precompute_coefficients`, `collect_markets`, etc.
+ Construct sparse $A$ matrix: stack exponential cone rows, position balance rows, box constraint rows. Build $bold(b)$ and $bold(c)$ vectors. Pass cone specification.
+ Extract primal fills ($bold(q)$) and dual prices ($bold(lambda)$) from `solver.solution`.
+ Round fills to integers, normalize prices, run `trim_mm_budget_overflows`; minting/burning is represented by the MINT account during settlement, not by synthetic fills.
+ Validate: compare fills and welfare against Frank-Wolfe EG on all existing test cases.

== Phase 2: Augmented LP (future)

+ Implement a primal-dual interior-point solver for the LP constraint structure (LDL#super[T] or Cholesky on the normal equations $A D^(-1) A^T$).
+ Add the rank-$K$ Hessian correction via Woodbury identity.
+ Implement Mehrotra predictor-corrector for practical iteration efficiency.
+ This gives a single solver for both LP ($K = 0$) and EG ($K > 0$), unifying `lp_solver.rs` and `eg_solver.rs`.


= Extension: Smoothed Minting Cost <smoothed>

The companion paper uses the smoothed minting cost $C_b (bold(D)) = b ln sum_s exp(D_s \/ b)$, which differs from the linear minting cost ($b -> 0$ limit) used in `lp_solver.rs`. The smoothed cost is also expressible via exponential cones.

The epigraph $C_b (bold(D)) <= tau$ decomposes as:

$ exists u_s >= 0: quad sum_s u_s <= 1, quad (D_s \/ b - tau \/ b, quad 1, quad u_s) in cal(K)_"exp" quad forall s in cal(S) $

This adds $|cal(S)|$ exponential cones — one per joint state. For a single binary market ($|cal(S)| = 2$), trivial. For a decomposed component with $|cal(S)| = 2^10 approx 1000$ states, still tractable.

The smoothed formulation is useful for:
- *Theoretical consistency* with the companion paper's analysis
- *Better conditioning*: the log-sum-exp gradient is smooth, avoiding the LP's vertex-hopping and degeneracy
- *Price uniqueness*: smoothed minting gives unique prices even at non-generic demand (no degenerate duals)

The practical solver should support both: linear minting (fast, matches existing solvers) and smoothed minting (robust, matches theory). The conic reformulation handles both — just toggle whether to include the $|cal(S)|$ extra exponential cones.


= Summary

#align(center)[
  #table(
    columns: 4,
    align: (left, center, center, center),
    stroke: none,
    [*Property*], [*Frank-Wolfe*], [*Clarabel*], [*Augmented LP*],
    [Quasi-linear ($s_k$)], [No], [Yes], [Yes],
    [Single solve], [No (25 LPs)], [Yes], [Yes],
    [Price extraction], [Projection LP], [Dual vars], [Dual vars],
    [Implementation], [Exists], [Days], [Weeks],
    [Speedup vs. FW], [1$times$], [$5$–$8 times$], [$~20 times$],
    [Speedup vs. LP], [$0.04 times$], [$0.2$–$0.3 times$], [$~0.9 times$],
  )
]

The key insight: the EG clearing problem is not a "hard nonlinear program" — it is an LP with $K$ logarithmic terms, where $K$ (number of MMs) is tiny. Interior point methods handle this with negligible overhead beyond the LP. The Frank-Wolfe approach pays for 25 LP solves to approximate what one Newton-based solve gives exactly.


#v(2em)
#line(length: 100%)
#v(0.5em)
#text(size: 9pt, style: "italic")[
  Design note for the Sybil matching engine. Companion to _Prediction Markets Are Fisher Markets_ and _Decomposed Combinatorial Clearing via Fisher Market Budget Allocation_ (2026).
]
