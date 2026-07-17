# Solver research loop

This is the Sybil analogue of `autoresearch/program.md`. It is deliberately a
guarded Pareto loop rather than a single-score hill climber.

## Fixed surface

- Do not tune the verifier, corpus, protocol, analyzer, or acceptance policy in
  the same change as a solver candidate.
- Work on one named solver idea at a time in a disposable `jj` change.
- Use development protocols for iteration. Freeze a surviving implementation
  before consuming new held-out seeds or future production replays.
- Never hide panics, timeouts, numerical failures, iteration caps, budget
  repairs, verifier failures, or integer landing loss.

## Loop

1. Record the baseline source revision and full development summaries.
2. State one falsifiable idea and append it to the relevant file under
   `design/solver-experiments/`.
3. Implement the smallest coherent candidate.
4. Run focused correctness tests, then the replay smoke.
5. Run the full relevant development protocol and analyzer.
6. Keep the candidate only if the hard gates pass and it is a credible Pareto
   improvement. Otherwise abandon the code but retain the result and diagnosis
   in the experiment log.
7. Periodically re-run surviving candidates on all solver protocols and only
   then promote one frozen change.

## Hard gates

- Complete record matrix and identical problem fingerprints.
- No new panic, solver failure, timeout, empty result, or verifier-invalid row.
- Every landed allocation respects limits, market clearing, groups, and MM
  budgets.
- No material supporting-price, integer-landing, or minting-duality regression.

## Soft scorecard

Judge the vector, not a weighted sum:

- availability and termination distribution;
- retained-cash objective and integer welfare P50/P95/max gaps;
- certificate P50/P95/max and iteration/oracle work;
- landing loss, allocation movement, budget repair, and minting-duality tails;
- wall-clock P50/P95/P99/max on the same machine;
- code size, dependency cost, conceptual surface, and proof burden.

Small noisy wins do not justify complexity. Equal performance with materially
simpler code is a win. A candidate with a meaningful quality gain and a modest
latency cost is a review decision, not an automatic discard.

## Current signal boundary

The current mix is suitable for autonomous bursts, not an unbounded unattended
search. Generated stress cases cover scale and numerical extremes; the first
replay corpus adds correlated multi-batch resting-book shapes. It is still one
agent simulation with originally slack maker budgets. Before long runs become
authoritative, add multiple independently seeded lifecycle corpora and
privacy-reviewed redacted captures from the deployed solver boundary.
