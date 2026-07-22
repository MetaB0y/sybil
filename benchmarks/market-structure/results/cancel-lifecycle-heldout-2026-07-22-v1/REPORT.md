# Deferred MM bundle cancel/replace evidence

## Decision

Implement authenticated, whole-bundle cancellation and whole-bundle atomic
replacement before the sequencer actor's block cutoff. Keep both operations
explicit and maker-initiated; do not automatically cancel quotes or describe
cancellation as a free improvement.

Atomic replacement is the preferred quote-refresh operation. In this model it
has the same stale-loss, fill, and trader-surplus outcome as canceling and
waiting for the next normal fresh submission, while avoiding the extra batch of
delay and the first-cutoff coverage hole. Cancellation remains necessary when a
maker intends to withdraw rather than replace liquidity.

The cutoff is actor order, not a client timestamp: an operation succeeds only
if the sequencer actor processes it before dequeuing the block-production
message. A late operation cannot rewrite the already prepared clone.

## Evidence contract

- Frozen protocol:
  `benchmarks/market-structure/protocol-cancel-lifecycle-heldout-2026-07-22-v1.json`
- Pushed implementation revision:
  `2f1081c9ff700daa21d2cdd21327761853f61015`
- Frozen protocol revision:
  `764bce7457520362a39af7e72587d278032edcd8`
- Untouched seeds: 30000 through 30063, consumed once in one complete run
- 73,728 engine rows, 24,576 paired episode groups, 384 configurations
- 73,728 completed rows; zero panics, solver failures, or verifier-invalid rows
- Actual production retained-cash solver and integer match verifier on every
  candidate row

Each episode pairs the current non-cancellable FBA, whole-bundle cancel, and
whole-bundle replacement on the same generated fundamentals, shocked markets,
arrival times, quote bundle, and shared integer MM budget. The grid spans two-
and eight-market bundles, partial and whole-bundle shocks, 500 ms and 10 second
batches, two spreads, four maker reaction times, two jump sizes, and 25%/100%
shared budgets.

## Results

When the maker action won the cutoff race, cancel and replace both removed the
modeled stale fill, but both intentionally removed informed IOC executions that
the current stale bundle would have accepted. Across the complete grid:

- cancel and replace each had a preregistered material fill-rate disadvantage
  versus current behavior in 240 of 384 configurations; the other 144 were not
  material, principally because the action was too late to affect the batch;
- cancel had a material execution-delay disadvantage in 192 configurations and
  a material first-cutoff displayed-coverage disadvantage in 240;
- replace had no material execution-delay or displayed-coverage difference
  versus current behavior in any configuration;
- replace and cancel had identical stale loss, maker markout, natural/informed
  trader surplus, and fill rate in every configuration, while replace had a
  material delay advantage in 192 and displayed-coverage advantage in 240;
- late cancel/replace requests produced the same batch observations as current
  behavior, matching the fail-closed cutoff rule.

The representative 500 ms, two-market/two-shock, one-cent-spread, ten-cent-
jump, 25 ms maker/taker, 25% shared-budget cell makes the transfer explicit:

| Replacement minus current | Paired mean | 95% interval |
|---|---:|---:|
| Maker stale loss | -172,778,313 nanos | [-239,200,762, -111,962,467] |
| Maker markout PnL | +152,685,728 nanos | [+80,712,384, +229,509,267] |
| Fill rate | -425,806 ppm | [-453,888, -395,632] |
| Natural-trader surplus | -63,681,099 nanos | [-117,617,518, -11,289,620] |
| Informed-trader surplus | -89,004,698 nanos | [-134,335,171, -49,446,434] |
| Execution delay | -2.8 ms | [-5.6, -0.6] |

For the same cell, cancel and replace have identical fills and surplus, but
cancel adds exactly one 500 ms batch of execution delay for the natural GTD
flow and removes all displayed liquidity at the first cutoff. This is why the
production lifecycle should expose atomic replacement rather than recommend a
cancel-then-submit sequence.

The maker-PnL improvement and trader-surplus reduction are two sides of the
same execution-price/fill transfer in the declared model. They are not a welfare
proof. The complete tables retain every metric rather than selecting the maker
result alone.

## Production implications

The evidence selects these semantics, subject to the ADR and implementation
tests:

1. Pending deferred MM bundle state remains owned by the sequencer actor.
2. Cancel removes the entire identified bundle or changes nothing.
3. Replace validates every new quote, group interaction, and the shared budget,
   then swaps the entire old bundle in one durable actor turn or changes
   nothing.
4. The actor's processing order is authoritative. Once block preparation wins,
   the operation returns a terminal too-late/not-pending result and cannot
   affect that block.
5. Exact retries are idempotent; conflicting reuse of an operation identity or
   stale sequence is rejected.
6. Settlement arithmetic, fills, commitments, and MM budget verification stay
   unchanged. The lifecycle affects which bundle becomes block input, not how
   an accepted block is settled or verified.

## Limits

This is controlled mechanism evidence, not a calibrated production effect
size. Generated makers and traders do not identify a real equilibrium, fee or
hedge policy, queue, or network. The runner deliberately isolates the actor
cutoff and shared-bundle choice. It does not establish that makers will use
cancellation well, that the measured transfers generalize to live flow, or that
lower stale loss outweighs lower fill and trader surplus.

No benchmark or production change was deployed.
