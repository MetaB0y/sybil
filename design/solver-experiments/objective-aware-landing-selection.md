# Objective-aware integer landing selection

Date: 2026-07-17

Status: accepted development change; synthetic and replay evidence, not held-out
paper evidence.

## Question

Once integer landing has several candidates supported by the same primary
prices, should it minimize the minting-duality residual exactly, or treat
numerically indistinguishable residuals as ties and optimize the solver's
retained-cash objective?

The prior selector compared the nearest-face, primary-basis, and certified
target candidates and chose the smallest
`|C_0(D) - p·D|`. On a large neutral book this preferred a seven-nanodollar
residual over an eight-nanodollar residual while sacrificing about `$2.54` of
retained objective. The support metric had accidentally become a secondary
objective at a precision where it only measured floating-point and integer
tie-breaking.

## Experiment OLS-001

The candidate policy was changed to:

1. compute the best minting-duality residual;
2. exclude candidates more than `1,000` nanodollars (`$0.000001`) above it;
3. among the remaining support-equivalent candidates, maximize the actual
   landed retained-cash objective;
4. break exact objective ties by smaller support residual and then stable
   candidate order; and
5. retain the existing hard failure when even the best residual exceeds
   `$0.05`.

The one-microdollar band is three orders of magnitude below the deployment's
current `$0.001` minimum resting-order notional. It is a
numerical-equivalence band, not permission to exchange meaningful price support
for objective value. The known utility-band variant that produced a `$21.14`
support discrepancy remains ineligible.

Four complete development matrices were compared against the exact preceding
implementation. Timing is omitted because the policy evaluates the same three
already-available candidates and benchmark timing varied between sequential
runs.

| Matrix | Rows | Result |
|---|---:|---|
| Sequencer replay | 160 | All non-timing outputs unchanged |
| Pacing development | 630 | Bundle max retained gap `0.4936% → 0.0507%`; max landing loss `0.5344% → 0.0531%` |
| Structural-oracle development | 244 | Both bundle backends max retained gap `0.3795% → 0`; max landing loss `0.3796% → 0.0023%` |
| Price-pacing development | 236 | Direct-dual max retained gap `0.4259% → 0.1953%`; max landing loss `0.4259% → 0.1961%` |

Availability and termination status were unchanged in all four matrices. Across
every changed bundle, RC-FW, structural-oracle, and direct-dual row, landed
retained objective was non-decreasing:

- pacing: 14 bundle and 11 RC-FW rows changed;
- structural: 3/4 bundle and 5/6 RC-FW rows changed, depending on backend; and
- price-pacing: 7 direct-dual and 11 RC-FW rows changed.

The largest single improvement was `0.4278%` for direct dual; the bundle row
that motivated the experiment improved `0.5373%`. No solver result became
invalid.

## Trade-offs and decision

One pacing-bundle row now reaches the final budget-repair step. It remains
verifier-valid, uses `99.9999%` of both MM budgets, and improves retained
objective by `0.5373%`. Its post-repair minting-duality residual rises to
`$0.000551643`, still about 90 times below the `$0.05` hard gate. The maximum
support residual across the tested shared-landing solvers remains below
`$0.001`.

Accept the policy. It makes the hierarchy explicit:

```text
hard feasibility and price support
    > retained-cash objective
    > distance to one continuous representative
    > arbitrary LP basis order
```

This is both simpler economically and stronger empirically than exact
nanodollar residual minimization. A focused unit test fixes the support-band,
objective, and deterministic tie-break semantics. Future work should make the
post-budget-repair support metric an explicit final gate if realistic replay
ever approaches the current `$0.05` limit; the present evidence does not
justify another repair heuristic.
