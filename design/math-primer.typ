#set document(title: "Mathematical Primer for 'Prediction Markets Are Fisher Markets'")
#set text(font: "New Computer Modern", size: 10pt)
#set page(margin: (x: 1.5in, y: 1.2in), numbering: "1")
#set par(justify: true, leading: 0.55em)
#set heading(numbering: "1.")
#show heading.where(level: 1): it => block(above: 1.5em, below: 0.8em)[#it]
#show heading.where(level: 2): it => block(above: 1.2em, below: 0.6em)[#it]

#align(center)[
  #text(size: 15pt, weight: "bold")[
    Mathematical Primer
  ]
  #v(0.3em)
  #text(size: 11pt)[For "Prediction Markets Are Fisher Markets"]
  #v(0.3em)
  #text(size: 9pt, style: "italic")[Five ideas that make the paper work]
]

#v(1em)

#block(inset: (x: 2em))[
  #text(weight: "bold")[What this document covers.]
  The paper uses five mathematical ideas. Each builds on the previous. If you understand these five things, you understand the entire paper — everything else is either an application of these ideas or context for why they matter.

  #v(0.5em)
  #align(center)[
    #table(
      columns: 3,
      align: (center, left, left),
      stroke: 0.5pt,
      inset: 6pt,
      [*\#*], [*Idea*], [*Unlocks*],
      [1], [Complementary slackness], [Theorems 3, 4, 8],
      [2], [Fenchel conjugates], [§2.2, §2.3, §2.8 (Theorems 1, 2, 7)],
      [3], [KKT conditions], [§2.4, §5.2 (Theorems 3, 8)],
      [4], [The budget absorption trick], [Theorem 8 part (3) — the punchline],
      [5], [Why log makes it convex], [The entire §3 → §5 arc],
    )
  ]
]

#v(1em)

= Minting Cost and the LP: Why $max_k D_k$?

Before the five ideas, one piece of setup from the paper (§2.1) needs unpacking: the clearing LP and its minting cost.

== What Minting Is

A prediction market has $K$ mutually exclusive outcomes. Minting creates one share of _every_ outcome at cost \$1. This is fairly priced: exactly one outcome resolves to \$1 at settlement, so a complete set is always worth \$1.

The key constraint: you cannot create shares of a single outcome. Every mint produces one share of each.

== What $D_k$ Actually Is

$D_k$ looks like "demand for outcome $k$" but it's more precise than that. It is the _net_ creation requirement — how many new shares of outcome $k$ the exchange must create, after netting buys against sells:

$ D_k = sum_(i in "buy"(k)) q_i - sum_(j in "sell"(k)) q_j $

Some shares come from sellers (they already hold shares and are giving them up). $D_k$ is what's left over — the shortfall that sellers can't cover.

#block(inset: (x: 2em, y: 0.5em))[
  *Example.* Outcome A has 100 shares of buy orders filled and 40 shares of sell orders filled. Net: $D_A = 100 - 40 = 60$. The exchange must _create_ 60 new A-shares. The other 40 came from sellers.
]

Crucially, *the optimizer controls $D_k$* through its choice of fills $bold(q)$. It can make $D_k$ small by filling fewer buy orders or filling more sell orders. $D_k$ is not external demand — it's a consequence of the optimizer's decisions.

If $D_k <= 0$ (more selling than buying), no new shares of outcome $k$ are needed — shares are being returned/destroyed.

== Why $M >= D_k$

$M$ is the number of complete sets minted. Each mint produces one share of _every_ outcome. After $M$ mints, you have $M$ new shares of each outcome available.

$D_k$ is how many new shares of outcome $k$ the fills require (as computed above).

The constraint $M >= D_k$ says: *new shares produced $>=$ new shares needed*, for each outcome. You must mint enough complete sets to cover the largest shortfall. You don't need $M >= D_k$ for outcomes where sellers already cover the demand — only where the net is positive.

== The LP Is a Tradeoff

The clearing LP is:

$ max_(bold(q), M) quad underbrace(sum_i w_i q_i, "welfare from fills") - underbrace(M, "minting cost") quad "s.t." quad D_k (bold(q)) <= M space forall k, quad bold(q) in [0, bold(overline(Q))] $

The optimizer chooses _both_ which orders to fill _and_ how much to mint. It's a cost–benefit tradeoff:

- *Filling orders creates welfare* ($w_i q_i > 0$ for profitable orders).
- *Filling orders creates net demand* ($D_k$ increases), which requires minting.
- *Minting costs money* ($-M$ in the objective).
- *The optimizer is free to fill nothing.* Setting $bold(q) = bold(0)$ gives $D_k = 0$, $M = 0$, objective $= 0$. This is always feasible.

The optimizer fills orders only when the welfare gained exceeds the minting cost incurred. The whole LP is the optimizer asking: *which fills create enough welfare to justify the minting they require?*

#block(inset: (x: 2em, y: 0.5em))[
  *Example.* Suppose filling a buy order creates \$0.60 of welfare but requires minting that costs \$0.80 (because no sellers offset it). The optimizer skips this order — it destroys value. But if a sell order offsets half the minting need, the net cost drops to \$0.40, and the fill becomes worthwhile.
]

== Why $M = max_k D_k$ at the Optimum

The constraint _allows_ minting more than needed ($M >= D_k$, not $M = D_k$). But $M$ is subtracted from the objective — it's a cost. The optimizer pushes $M$ down as far as possible, which means $M = max_k D_k$ at the optimum (the tightest the constraints allow). The $>=$ is the LP formulation; the optimizer tightens it to equality.

This is also complementary slackness at work: the dual variable of $D_k <= M$ is the price $p_k$, and $p_k > 0$ only where $D_k = M$ (the binding constraints).

== Why Is the Surplus Free?

When you mint 70 sets but only need 30 B-shares, the remaining 40 B-shares are surplus. Why don't they cost anything? Because complete sets cost \$1 and pay out \$1 — the surplus shares of losing outcomes are worthless at settlement, and the one winning outcome's surplus was already paid for by the mint. The $max_k D_k$ accounting captures this exactly: you pay for the most-demanded outcome, and everything else comes free with the complete sets.

#v(0.5em)

= Complementary Slackness

You have a constraint — say, a fill can't exceed its maximum quantity:

$ q_i <= overline(Q)_i $

The optimizer assigns a _dual variable_ (also called a multiplier) $mu_i^+$ to this constraint. Think of it as the "price" of the constraint: how much better would the objective be if you relaxed this bound by one unit?

Complementary slackness says:

#block(inset: (x: 2em, y: 0.8em), fill: luma(245), radius: 4pt)[
  *Rule.* At the optimum, at least one of these is zero: the multiplier, or the slack.
  $ mu_i^+ dot (overline(Q)_i - q_i) = 0 $
]

Two cases:

#align(center)[
  #table(
    columns: 3,
    align: (left, left, left),
    stroke: 0.5pt,
    inset: 6pt,
    [*Situation*], [*Slack*], [*Multiplier*],
    [Constraint is tight ($q_i = overline(Q)_i$)], [Zero — no room left], [Can be positive — relaxing would help],
    [Constraint is loose ($q_i < overline(Q)_i$)], [Positive — room left], [Must be zero — relaxing is pointless],
  )
]

*Intuition.* "Relaxing" means raising the ceiling — changing $overline(Q)_i$ from 100 to 101, giving the constraint more room. If you're not using the full capacity, a higher ceiling wouldn't help — so the "price" of more capacity is zero. If you _are_ at full capacity, a higher ceiling might actually help, so the price can be positive.

*Where the paper uses this:*
- *Theorem 4* (self-financing): the dual variable of the minting constraint $D_k <= M$ is the price $p_k$. Complementary slackness says $p_k > 0$ only where $D_k = M$ — price is positive only for the highest-demand outcome.
- *§2.4* (clearing rule): gives the three cases (fully filled / unfilled / marginal).
- *Theorem 8* (budget absorption): the $lambda_i^+ q_i$ terms that make spending $<=$ budget.


= Fenchel Conjugates

This is the engine of §2. A Fenchel conjugate transforms a function from "quantity space" to "price space."

== The Definition

Given a function $f(x)$, its conjugate is:

$ f^*(p) = sup_x [p dot x - f(x)] $

You sweep over all $x$, computing "revenue minus cost," and take the best.

== The Meaning

Think of $f(x)$ as a _cost function_ — producing quantity $x$ costs $f(x)$. Now someone offers price $p$ per unit. Your profit from producing $x$ is:

$ "profit" = underbrace(p dot x, "revenue") - underbrace(f(x), "cost") $

The conjugate $f^*(p)$ is your *maximum profit at price $p$* — the best you can do by choosing the optimal production quantity.

#block(inset: (x: 2em, y: 0.8em), fill: luma(245), radius: 4pt)[
  *Key idea.* $f$ lives in quantity space. $f^*$ lives in price space. They encode the same information from dual perspectives. For convex $f$, the conjugate of the conjugate gives back $f$.

  #align(center)[
    #table(
      columns: 3,
      align: center,
      stroke: none,
      [$f(x)$], [$stretch(arrow.l.r, size: #200%)^("conjugate")$], [$f^*(p)$],
      [cost of producing $x$], [], [max profit at price $p$],
      [quantity space], [], [price space],
    )
  ]
]

== Worked Example: Minting Cost (Theorem 1)

Our cost function is $V(bold(D)) = max_k D_k$ — the minting cost. We want the conjugate:

$ V^*(bold(p)) = sup_(bold(D)) [sum_k p_k D_k - max_k D_k] $

You're selling outcome shares at prices $p_k$ and paying minting cost $max_k D_k$. What demand vector $bold(D)$ maximizes profit?

#block(inset: (x: 2em, y: 0.5em))[
  *Case: $bold(p)$ is a probability vector* ($sum p_k = 1$, all $p_k >= 0$).

  Revenue $= sum p_k D_k$ is a weighted average of the $D_k$'s with weights summing to 1. A weighted average can never exceed the maximum:
  $ sum p_k D_k <= max_k D_k $
  So profit $<= 0$ for every $bold(D)$. At $bold(D) = bold(0)$, profit $= 0$. Best profit: $V^*(bold(p)) = 0$.
]

#block(inset: (x: 2em, y: 0.5em))[
  *Case: $sum p_k > 1$.*

  Set all $D_k = t$, send $t -> infinity$. Revenue $= t sum p_k$. Cost $= t$. Profit $= t(sum p_k - 1) -> infinity$.

  _Arbitrage:_ you mint complete sets at \$1 each and sell the shares for $sum p_k > dollar 1$. Scale up for unbounded profit.
]

#block(inset: (x: 2em, y: 0.5em))[
  *Case: $sum p_k < 1$.*

  Set all $D_k = -t$ (buy back shares), send $t -> infinity$. Profit $= t(1 - sum p_k) -> infinity$.

  _Reverse arbitrage:_ buy a complete set of shares for $sum p_k < dollar 1$ and redeem for \$1.
]

#block(inset: (x: 2em, y: 0.5em))[
  *Case: some $p_k < 0$.*

  Drive $D_k -> -infinity$ for that outcome. Someone is paying you to take shares. Infinite profit.
]

Result:

$ V^*(bold(p)) = cases(0 & "if" bold(p) in Delta, +infinity & "otherwise") $

This is the simplex indicator. Its meaning: *the only prices where you can't arbitrage the minting mechanism are probabilities.* The probability axiom is a no-arbitrage condition, not a modeling choice.

== Worked Example: LMSR Cost (Theorem 2)

Replace minting cost with LMSR: $C_b (bold(D)) = b ln sum exp(D_k\/b)$. Same game:

$ C_b^*(bold(p)) = sup_(bold(D)) [sum p_k D_k - b ln sum exp(D_k\/b)] $

For the minting cost (Theorem 1), $V = max_k D_k$ has a kink — it's not differentiable where two $D_k$'s are tied — so we had to reason case-by-case. The LMSR cost $C_b$ is smooth (that's the whole point of entropy smoothing), so we can use calculus: find the maximum by setting the derivative to zero.

The expression inside the $sup$ — "linear in $bold(D)$" minus "convex in $bold(D)$" — is _concave_ in $bold(D)$. For a concave function, any point where the derivative is zero is the global maximum (not just a local one — this is what convexity/concavity buys you). So setting the derivative to zero finds _the_ answer:

$ (partial) / (partial D_k) [sum p_k D_k - C_b(bold(D))] = p_k - exp(D_k\/b) / (sum_j exp(D_j\/b)) = 0 $

$ therefore quad p_k = exp(D_k\/b) / (sum_j exp(D_j\/b)) $

This is the *softmax*. The conjugate asks "what demand vector maximizes profit at prices $bold(p)$?" and the answer is: the one where prices equal the softmax of demand.

Invert ($D_k = b ln p_k + b ln Z$ where $Z = sum exp(D_j\/b)$), substitute back, and after algebra:

$ C_b^*(bold(p)) = b sum_k p_k ln p_k $

Negative Shannon entropy. Finite only on the simplex (softmax always outputs probabilities). Most negative at uniform $p_k = 1\/K$ (maximum entropy = most uncertain prices).

== The Duality Picture

#align(center)[
  #table(
    columns: 4,
    align: (center, center, center, left),
    stroke: 0.5pt,
    inset: 6pt,
    [*Cost (quantity space)*], [], [*Conjugate (price space)*], [*Character*],
    [$V = max_k D_k$], [$stretch(arrow.l.r, size: #150%)$], [$V^* = delta_Delta$], [Hard wall: probabilities or nothing],
    [$C_b = b ln sum exp(D_k\/b)$], [$stretch(arrow.l.r, size: #150%)$], [$C_b^* = b sum p_k ln p_k$], [Soft penalty: non-uniform costs more],
  )

  #v(0.3em)
  _As $b -> 0$: the soft penalty hardens into the wall._
]

*Why this matters for the paper.* In §2.8, the clearing problem is rewritten in price space as:

$ min_(bold(p) in Delta) [W^*(bold(p)) + C_b^*(bold(p))] $

The entropy term $C_b^*$ is _strictly convex_ — it curves. That curvature forces a unique minimizer. This is Theorem 7: without budgets, prices are always unique. The entire §3 obstacle is about budgets breaking this.


= KKT Conditions

You want to maximize $f(x)$ subject to $x in [0, overline(Q)]$. Calculus says "set $f'(x) = 0$" — but what if the max is at the boundary?

KKT handles boundary optima with multipliers:

$ f'(x) = mu^+ - mu^- $

where $mu^+, mu^- >= 0$ with complementary slackness ($mu^+ (x - overline(Q)) = 0$, $mu^- x = 0$).

#align(center)[
  #table(
    columns: 3,
    align: (left, left, left),
    stroke: 0.5pt,
    inset: 6pt,
    [*Case*], [*Condition*], [*Meaning*],
    [Interior ($0 < x < overline(Q)$)], [$f'(x) = 0$], [Just calculus — both multipliers zero],
    [At ceiling ($x = overline(Q)$)], [$f'(x) >= 0$], [Function still rising — you'd want more but you're capped],
    [At floor ($x = 0$)], [$f'(x) <= 0$], [Function falling — you want none],
  )
]

*In the paper (§2.4).* The objective per order is $w_i q_i - C_b (bold(D))$. For buy order $i$ on outcome $k$:

$ (partial) / (partial q_i) [w_i q_i - C_b] = L_i - underbrace(exp(D_k\/b) / (sum_j exp(D_j\/b)), p_k) $

The derivative is just $L_i - p_k$ (limit price minus clearing price). KKT says:

#align(center)[
  #table(
    columns: 3,
    align: (left, left, left),
    stroke: 0.5pt,
    inset: 6pt,
    [*Case*], [*Derivative*], [*In English*],
    [Fully filled ($q_i = overline(Q)_i$)], [$L_i - p_k >= 0$], [Limit above price — fill everything],
    [Unfilled ($q_i = 0$)], [$L_i - p_k <= 0$], [Limit below price — fill nothing],
    [Marginal ($0 < q_i < overline(Q)_i$)], [$L_i - p_k = 0$], [Limit equals price — this order sets the price],
  )
]

This is the Uniform Clearing Price rule: buy if your limit exceeds the price.

*Why this matters.* Theorem 8 uses the exact same KKT structure, but with $B_k \/ U_k$ replacing $w_i$. If you understand the above, you understand the Theorem 8 proof — it's the same three cases with one extra chain-rule step.


= The Budget Absorption Trick

This is the punchline of the paper (Theorem 8, part 3). We replace linear welfare $sum w_i q_i$ with $B_k ln U_k$ where $U_k = sum_(i in "MM"_k) L_i q_i$ is MM $k$'s total weighted fill. An important modeling choice: $"MM"_k$ contains only MM $k$'s _buy_ orders (with $L_i > 0$). Sell orders from MMs are treated as retail (linear welfare). This ensures $U_k > 0$ and aligns with the Fisher market analogy, where agents are consumers (buyers of goods).

== Step 1: KKT for the Log Objective

For MM buy order $i$ belonging to MM $k$, differentiate $B_k ln U_k$ with respect to $q_i$:

$ (partial) / (partial q_i) [B_k ln U_k] = B_k dot 1/U_k dot L_i $

This is the chain rule: derivative of $ln$ is $1\/U_k$, derivative of $U_k$ w.r.t. $q_i$ is $L_i$, times the weight $B_k$.

The full KKT also includes the minting cost derivative. Since this is a buy order, it increases $D_(m(i))$ by $q_i$, costing $p_(m(i))$ per share. With box multipliers:

$ (B_k L_i) / U_k - p_(m(i)) = lambda_i^+ - lambda_i^- $

== Step 2: The Telescoping

Multiply both sides by $q_i$:

$ (B_k L_i q_i) / U_k - p_(m(i)) q_i = lambda_i^+ q_i - lambda_i^- q_i $

Note: $lambda_i^- q_i = 0$ by complementary slackness (if $q_i > 0$ then $lambda_i^- = 0$; if $lambda_i^- > 0$ then $q_i = 0$). So the last term vanishes. Now sum over all $i in "MM"_k$:

$ B_k / U_k dot underbrace(sum_(i in "MM"_k) L_i q_i, "this is " U_k " by definition") - underbrace(sum_(i in "MM"_k) p_(m(i)) q_i, "capital spent on purchases") = underbrace(sum_(i in "MM"_k) lambda_i^+ q_i, >= 0) $

The left side: $U_k$ cancels, leaving $B_k - sum p_(m(i)) q_i$.

#block(inset: (x: 2em, y: 0.8em), fill: luma(245), radius: 4pt)[
  $ B_k - sum_(i in "MM"_k) p_(m(i)) q_i = sum_(i in "MM"_k) lambda_i^+ q_i >= 0 $

  $ therefore quad sum_(i in "MM"_k) p_(m(i)) q_i <= B_k $

  Each MM's capital deployed on purchases is *at most* $B_k$, with equality when no fill hits its upper bound.
]

That's it. Five lines of algebra. The budget constraint was never imposed — it _emerged_ from the first-order conditions of the log objective.

== Why This Works: Intuition

#block(inset: (x: 2em, y: 0.8em), fill: luma(245), radius: 4pt)[
  $ln(U_k)$ has a singularity at $U_k = 0$: the slope goes to $infinity$.

  #v(0.3em)
  #align(center)[
    #table(
      columns: 2,
      align: (left, left),
      stroke: none,
      [Near $U_k = 0$:], [Marginal value $B_k\/U_k$ is huge — fill desperately],
      [As $U_k$ grows:], [Marginal value $B_k\/U_k$ shrinks — diminishing returns],
      [At optimum:], [Marginal value per dollar $=$ price for every active order],
    )
  ]

  #v(0.3em)
  Because of the $B_k$ scaling, this balance point is reached exactly when total spending $= B_k$. Bigger budget $arrow.r$ more fill before saturation. The budget isn't a constraint; it's a _consequence of the curvature_.
]


= Why Log Makes It Convex

Now we connect everything. The risk-neutral model has a non-convexity problem (§3). The log model doesn't (§5). Here's why.

== The Risk-Neutral Problem

Objective: $sum w_i q_i - C_b (bold(D))$.

#align(center)[
  #table(
    columns: 3,
    align: (left, left, left),
    stroke: 0.5pt,
    inset: 6pt,
    [*Component*], [*Curvature*], [*Source*],
    [$sum w_i q_i$ (linear welfare)], [None — flat], [No help],
    [$-C_b$ (minting cost)], [Concave, strength $O(1\/b)$], [Entropy smoothing],
    [Budget constraint $c(p) dot q <= B_k$], [Non-convex, strength $O(1\/b)$], [Price $times$ quantity],
  )
]

The only curvature comes from $-C_b$, which scales as $O(1\/b)$. But the budget non-convexity _also_ scales as $O(1\/b)$ — they grow at the same rate. Neither dominates. *Deadlock.*

At $b = 0$ (LP), there is zero curvature and the budget constraint is fully non-convex. No hope.

== The Risk-Averse Fix

Objective: $sum B_k ln U_k + "retail" - C_b (bold(D))$.

#align(center)[
  #table(
    columns: 3,
    align: (left, left, left),
    stroke: 0.5pt,
    inset: 6pt,
    [*Component*], [*Curvature*], [*Source*],
    [$sum B_k ln U_k$ (log welfare)], [Strictly concave, $O(B_k\/U_k^2)$], [$ln$ — independent of $b$],
    [$-C_b$ (minting cost)], [Concave, strength $O(1\/b)$], [Entropy smoothing — bonus],
    [Budget constraint], [*Gone*], [Absorbed by §4 trick],
  )
]

Two things change simultaneously:

+ *The budget constraint disappears* from the feasible set. The bilinear $c(p) dot q <= B_k$ is gone — absorbed into the objective (§4 trick). No more non-convex feasible set.

+ *Strict curvature appears* in the objective. The $ln$ provides $O(B_k\/U_k^2)$ concavity, independent of $b$. This guarantees a unique maximum.

The $-C_b$ term is now pure bonus — extra concavity on top of what $ln$ already provides. Even at $b = 0$ (no entropy smoothing at all), the program is still strictly concave:

$ max_(bold(q) in cal(C)) quad sum_k B_k ln U_k + sum_(j in.not "MM") w_j q_j - max_k D_k $

The $-max_k D_k$ is concave (not strictly), but $ln$ carries the strict concavity alone. *No annealing needed.*

== The Full Picture

#block(inset: (x: 2em, y: 0.8em), fill: luma(245), radius: 4pt)[
  #align(center)[
    #table(
      columns: 3,
      align: (center, center, center),
      stroke: 0.5pt,
      inset: 6pt,
      [], [*Risk-neutral*], [*Risk-averse*],
      [Welfare], [$sum w_i q_i$ (linear)], [$sum B_k ln U_k$ (log)],
      [Budget constraint], [Explicit, bilinear, non-convex], [Absent — absorbed into objective],
      [Curvature source], [$-C_b$ only ($O(1\/b)$)], [$ln$ ($O(B_k\/U_k^2)$, $b$-independent)],
      [At $b = 0$], [Zero curvature, non-convex — hopeless], [Strict curvature, convex — fine],
      [Unique prices?], [Not always (Prop. 2)], [Always (Thm. 8)],
      [Complexity], [GNEP (intractable in general)], [Convex program (polynomial)],
    )
  ]

  #v(0.5em)
  One modeling change — linear welfare $arrow.r$ log welfare — removes the non-convexity and adds strict curvature. The paper's thesis: this isn't a trick. The linear model was the fiction.
]

#v(2em)
#line(length: 100%)
#v(0.5em)
#text(size: 9pt, style: "italic")[
  This primer covers the five mathematical ideas in "Prediction Markets Are Fisher Markets." For the full proofs, formal statements, and economic motivation, see the paper itself.
]
