#set document(title: "MINT Account P&L Under Price Complementarity")
#set text(font: "New Computer Modern", size: 10pt)
#set page(margin: (x: 1.5in, y: 1.2in), numbering: "1")
#set par(justify: true, leading: 0.55em)
#set heading(numbering: "1.")
#show heading.where(level: 1): it => block(above: 1.5em, below: 0.8em)[#it]
#show heading.where(level: 2): it => block(above: 1.2em, below: 0.6em)[#it]

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
    MINT Account P\&L Under Price Complementarity
  ]
  #v(0.5em)
  #text(size: 11pt)[Minting Is Fair-Valued at Clearing Prices]
  #v(0.3em)
  #text(size: 9pt, style: "italic")[Note — April 2026]
]

#v(1em)

#block(inset: (x: 2em))[
  #text(weight: "bold")[Summary.]
  In Sybil's batch auction, group minting creates position imbalances that a system MINT account absorbs. We prove that MINT's expected P\&L is exactly zero under the clearing prices, because clearing prices satisfy the simplex constraint (Theorem 1 of the companion paper). MINT's variance is bounded by the minting volume. This note connects the implementation-level MINT account to the mathematical framework in _Prediction Markets Are Fisher Markets_ and its Lean4 formalization.
]

#v(1em)

= Setup

Consider a batch auction with $K$ mutually exclusive outcomes in a market group. The solver clears the batch and produces clearing prices $bold(p) = (p_1, dots, p_K)$ and fill quantities $bold(q)$.

After settling fills against real accounts, the _net demand_ for each outcome is $D_k = sum_(i in "buy"(k)) q_i - sum_(j in "sell"(k)) q_j$, and the total positions across all (non-MINT) accounts are:

$
T_k^"pre" = "total position for outcome" k "across all accounts"
$

In an independent binary market, minting creates YES+NO pairs, so $T_"YES" = T_"NO"$. But in a _market group_, the solver can mint YES shares for one market without minting corresponding shares for others (group minting). This creates an imbalance: $T_k != T_j$ for some $k, j$.

The MINT account absorbs the imbalance. For each market, define the _incremental imbalance_:

$
delta_k = T_k - T_"ref"
$

where $T_"ref" = min_k T_k$, so that MINT shorts the excess for each outcome. In the binary case, the code computes $"diff" = T_"YES" - T_"NO"$ directly.

= The MINT Account

After settling fills, the sequencer:

1. Computes $"diff"_m = T_"YES"^m - T_"NO"^m$ for each market $m$.
2. If $"diff"_m > 0$: MINT receives position $-"diff"_m$ in YES and cash $p_"YES"^m dot "diff"_m$.
3. If $"diff"_m < 0$: MINT receives position $+"diff"_m$ in NO (i.e., $-|"diff"_m|$) and cash $p_"NO"^m dot |"diff"_m|$.

After this adjustment, $T_"YES"^m = T_"NO"^m$ for every market (position balance invariant). The verifier independently re-derives this from the witness.

= MINT Expected P\&L Is Zero

#theorem(name: "MINT Fair Valuation")[
  _Let $bold(p)^*$ be the clearing prices of a batch auction. If $bold(p)^*$ satisfies the simplex constraint $sum_k p_k^* = 1$ (which holds for all $b >= 0$ by Minting--Simplex Duality), then MINT's expected P\&L valued at clearing prices is exactly zero._
] <thm-mint-pnl>

_Proof._ Consider a single binary market with imbalance $d = T_"YES" - T_"NO" > 0$ (the $d < 0$ case is symmetric). MINT receives:

- Cash: $+p_"YES" dot d$
- Position: $-d$ shares of YES

At resolution, MINT's position has _expected_ value (at clearing prices):

$
EE["position value"] = p_"YES" dot (-d) dot 1 + p_"NO" dot (-d) dot 0 = -p_"YES" dot d
$

because YES shares pay \$1 if YES wins (probability $p_"YES"$) and \$0 otherwise. MINT's expected P\&L is:

$
"E[P&L]" = underbrace(p_"YES" dot d, "cash received") + underbrace((-p_"YES" dot d), "expected position value") = 0 #h(1fr) square
$

#proposition(name: "Multi-Outcome Extension")[
  _For a market group with $K$ outcomes and MINT positions $bold(s) = (s_1, dots, s_K)$ where $s_k <= 0$, MINT's expected P\&L is:_
  $ "E[P&L]" = underbrace(sum_k |s_k| dot p_k, "cash received") + underbrace(sum_k s_k dot p_k, "expected position value") = 0 $
  _since $s_k <= 0$ implies $|s_k| = -s_k$._
] <prop-multi>

_Proof._ Each outcome $j$ wins with probability $p_j$ (at clearing prices). When outcome $j$ wins, MINT's position payout is $s_j dot 1 = s_j$ (the short position resolves at \$1 per share). The expected position value is $sum_k p_k dot s_k$. The cash received is $sum_k |s_k| dot p_k = -sum_k s_k dot p_k$. Their sum is zero. #h(1fr) $square$

= Why Clearing Prices Are on the Simplex

The zero-P\&L result rests entirely on $sum_k p_k = 1$. This is not imposed by fiat — it is a theorem:

*Theorem 1* (Minting--Simplex Duality, proven in Lean4). _The Fenchel conjugate of the minting cost $V(bold(D)) = max_k D_k$ is the indicator of the probability simplex._ Every feasible set of clearing prices satisfies $bold(p) in Delta = {bold(p) >= 0 : sum p_k = 1}$.

The Lean4 proof is at `lean/FisherClearing/Duality/MintingSimplex.lean`. The key lemma: for $bold(p) in Delta$, convexity gives $angle.l bold(p), bold(D) angle.r = sum p_k D_k <= max_k D_k dot sum p_k = max_k D_k$, so the conjugate is 0. Off the simplex, scaling $bold(D)$ sends the objective to $+infinity$.

With entropy smoothing ($b > 0$), prices are the softmax of net demand — automatically on the simplex. At $b = 0$, prices are LP dual variables, and the simplex constraint is enforced by the epigraph formulation.

= Variance and Worst-Case Loss

While expected P\&L is zero, MINT carries variance:

#proposition(name: "MINT Worst-Case Loss")[
  _Per market, MINT's worst-case loss is at most $|d| dot (1 - p_"win")$ where $d$ is the imbalance and $p_"win"$ is the probability of the outcome MINT shorted. Across all markets and blocks, the total worst-case loss is bounded by the total minting volume (in nanos)._
] <prop-variance>

_Proof._ MINT shorted $|d|$ shares at price $p$. If the shorted outcome wins, MINT pays $|d| dot 1$ and received $|d| dot p$, for a loss of $|d| dot (1 - p)$. If the other outcome wins, MINT's loss is zero and profit is $|d| dot p$. The worst case is the loss scenario. #h(1fr) $square$

Over many independent markets and blocks, the law of large numbers drives realized P\&L toward zero. The variance decreases as $1/n$ where $n$ is the number of independent minting events.

= Operational Invariants

The implementation maintains three invariants verified every block:

1. *Position balance*: $T_"YES"^m = T_"NO"^m$ for every market $m$, across all accounts including MINT.

2. *Money conservation*: total system balance (sum of all account balances) is constant. MINT receives cash equal to $p dot |d|$; no cash is created or destroyed.

3. *Clearing price existence*: every market with nonzero imbalance must have clearing prices in the witness. The sequencer panics and the verifier flags `MintingWithoutClearingPrice` if this fails.

The verifier independently re-derives MINT adjustments from the witness (pre-state + fills + clearing prices) and checks that the result matches the claimed post-state. This is Layer 2 (settlement verification). Layer 3 (block integrity) then verifies the state root hash covers the MINT-adjusted state.

= Connection to Existing Proofs

#align(center)[
  #table(
    columns: 3,
    align: (left, left, left),
    stroke: none,
    [*Result*], [*Source*], [*Role*],
    [@thm-mint-pnl (MINT P\&L = 0)], [This note], [Minting is fair-valued],
    [Minting--Simplex Duality], [`MintingSimplex.lean`], [$sum p_k = 1$: no-arbitrage],
    [LMSR--Entropy Duality], [`LmsrEntropy.lean`], [Smoothed prices on simplex],
    [Price Uniqueness], [`PriceUniqueness.lean`], [Prices unique for $b > 0$],
    [Welfare Gap Bound], [`WelfareGap.lean`], [Bounds cost of budget binding],
    [Reduced-Form Utility], [`Utility.lean`], [MM utility is well-behaved],
  )
]

The full chain: MintingSimplex ensures prices are probabilities $arrow.r$ @thm-mint-pnl ensures MINT is fair-valued $arrow.r$ Position balance + money conservation are verified by the 4-layer verifier $arrow.r$ future ZK circuit proves the same constraints in zero knowledge.
