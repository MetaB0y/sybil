# Canonical clearing-price selection experiments

## Outcome

GitHub issue
[#190](https://github.com/MetaB0y/sybil/issues/190) investigated whether the
verifier should derive one exact price when a landed allocation admits multiple
supporting prices. The consensus-canonicalization feature was implemented,
tested, then reverted on 2026-07-20.

The verifier continues to require the safety properties that matter for any
accepted price: uniform clearing, literal order limits, price complementarity,
exact shared MM budgets, settlement, and conservation. It does not require one
particular point inside the valid price set. Solver-side price centering remains
a possible non-consensus quality improvement.

## CPS-001 — raw residual imbalance

- Date: 2026-07-18
- Status: rejected
- Hypothesis: residual buys select the upper feasible boundary, residual sells
  select the lower boundary, and balance selects the midpoint.
- Result: raw residual quantity is misleading for flash liquidity. A shared
  cross-market MM budget can make an apparently unfilled quote non-executable.

## CPS-002 — maximum entropy on the exact equilibrium-price face

- Date: 2026-07-18
- Status: mathematically valid, rejected for current consensus integration
- Hypothesis: choose the maximum-Shannon-entropy point of the exact
  zero-temperature retained-cash price face.
- Result: this is the principled zero-temperature limit proved by the Fisher
  market paper. It is appropriate when the exact continuous equilibrium face is
  available. The landed integer allocation, however, frequently has no exact
  KKT-supporting price after quantity rounding.

## CPS-003 — paced bounds on landed fills

- Date: 2026-07-18
- Status: rejected
- Result: applying the recomputed MM pacing factor to filled as well as
  residual quantity made ordinary integer landings expose an empty price face.
  Literal fill limits remained individually rational, but the landed point was
  not an exact continuous equilibrium.

## CPS-004 — canonical repricing followed by budget trimming

- Date: 2026-07-18
- Status: rejected
- Result: a new canonical price can make a previously affordable shared-MM
  allocation exceed budget. Trimming the MM leg changes net demand, minting
  complementarity, welfare, and potentially the next canonical price. Price
  selection was therefore not merely choosing among economically equivalent
  settlements.

## CPS-005 — minimax residual-KKT relaxation

- Date: 2026-07-18
- Status: rejected after audit
- Hypothesis: crossed residual KKT bounds are integer dust, so minimize one
  common relaxation and maximize entropy on the resulting face.
- Initial evidence: one multi-MM flash landing crossed by only 95 nanos after
  rational pacing was recomputed from rounded quantities.
- Audit: temporary instrumentation over
  `cargo test -p matching-solver --features lp -- --nocapture` observed positive
  relaxations from a few nanos through `646,788,214` nanos. Generated cases
  repeatedly reached `400,000,000` and `646,788,214` nanos.
- Counterexample: a 70 buyer and 20 seller, both half-filled with equal
  residual quantity, were accepted by relaxing each residual bound 25 cents
  and selecting 45. Both sides could profitably continue trading, so the
  allocation was not a retained-cash equilibrium.
- Decision: an unbounded relaxation cannot be described as exact equilibrium
  validity.

## CPS-006 — hard landed-fill face only

- Date: 2026-07-20
- Status: rejected as consensus policy; mechanically viable
- Hypothesis: ignore unfilled orders, construct only the literal filled-order
  and minting face, then choose its maximum-entropy point.
- Result: the experiment removed roughly 400 implementation lines. All 10
  engine tests, all 7 verifier-backed generated solver-conformance tests, and
  all 151 verifier tests passed. Of 48 solver unit tests, 47 passed; the only
  failure expected residual sell liquidity to choose 22.5 rather than the
  hard-face price of 50.
- Counterexample: a central hard-face price can violate a shared MM budget even
  when another price on the same filled-limit face is affordable. The existing
  finalizer then trims quantities, so this is not allocation-preserving.

## Decision

Exact canonical-price equality is not currently a consensus invariant.
Individual rationality alone does not imply shared-MM affordability, but the
verifier already checks hard budgets directly. Conversely, canonicalizing one
price does not make the untrusted allocation optimal or complete.

For now:

1. accept any uniform, limit-valid, budget-feasible settlement price;
2. keep all integer settlement and conservation checks authoritative;
3. leave interior-price preference to solver policy and diagnostics; and
4. reconsider consensus canonicalization only with a carefully specified
   integer allocation/price certificate and escape-valuation design.

