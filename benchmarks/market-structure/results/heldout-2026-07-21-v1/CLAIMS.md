# Market-structure claim sheet

Every statement below names its evidence tier. Controlled effects describe the
frozen synthetic model; historical rows describe public trades; neither is
traction.

| Candidate claim | Status | Allowed wording | Evidence anchor |
|---|---|---|---|
| FBA universally protects makers from stale quotes. | Rejected | Current FBA protects makers only when the modeled CLOB cancellation is slower than informed flow; it is worse when cancellation is as fast or faster. | `paired-summary.csv`: jump, `sybil-fba` vs `clob-firm-reserve`, `maker_stale_quote_loss_nanos`; 135 lower, 105 higher, 84 tied. |
| 500 ms FBA can materially improve maker outcomes in news jumps. | Supported with conditions | In the controlled 60-cent-jump, 1-cent-quote case with a maker reacting at 500 ms and an informed taker at 25 ms, 500 ms FBA materially improves maker PnL per filled share and price error without a preregistered material fill or delay penalty. Do not generalize that joint tradeoff result to every jump cell. | Case `micro-jump-b500-s10000000-mr500-tr25-n1-j600000000`; 128 paired seeds. |
| Longer batches strengthen the advantage without cost. | Rejected | The representative protection persists at 10 s, but delay is materially worse by 4.397 s (95% CI 3.931–4.882 s). | Case `micro-jump-b10000-s10000000-mr2000-tr500-n1-j600000000`. |
| More same-batch informed traders improve price competition. | Not supported | One, two, and eight informed traders have identical advantage/disadvantage counts in this fixed-capacity, one-sided IOC design. Richer price schedules and multiple makers remain untested. | Jump summaries grouped by `informed_trader_count`. |
| FBA makers are more profitable before incentives in quiet markets. | Rejected in this model | Quiet CLOB makers earn positive spread PnL; the FBA maker earns zero because natural traders match each other. | All nine quiet cases in `engine-metrics.csv.gz`. |
| Quiet FBA can improve ordinary-trader fills. | Supported with tradeoff | At 5- and 10-cent half-spreads, FBA materially improves fill rate, but adds batch delay and does not improve maker PnL. | Quiet `fill_rate_ppm`, `execution_delay_ms`, and maker markout rows. |
| FBA covers more markets than a competent shared-risk CLOB. | Rejected | Displayed, single-market executable, and simultaneous-worst-case coverage are identical in all 18 portfolio cases. | Portfolio `sybil-fba` vs `clob-shared-risk` coverage rows. |
| Atomic shared-budget clearing improves fills under scarce capital. | Supported with conditions | At 10% budget, FBA improves fill rate by 15.51–20.80 points across 8/72 markets and three flow shapes; the advantage is below 5 points at 25% and zero at 50%. | Portfolio `fill_rate_ppm` rows and `REPORT.md`. |
| Higher FBA maker PnL is net welfare creation. | Rejected as a general claim | At 50% portfolio budget, joint maker-plus-natural surplus is identical; higher maker PnL is a trader transfer. At 10%, additional fills do raise joint surplus in this model. | 72-market uniform engine rows. |
| Cancellable FBA strictly improves current Sybil. | Rejected | Cancellation lowers stale loss in 165 configurations but materially lowers fill rate in those same configurations; it is a future-design sensitivity. | `fba-cancellable-sensitivity` vs `sybil-fba`. |
| Polymarket sweeps prove market-maker losses or an FBA counterfactual. | Rejected | Public rows establish anonymous resting-side executions and gross settlement markouts only. | `historical-summary.json`; four known hashes, complete 31-market event. |
| The four known sweeps are fabricated or isolated. | Rejected / contextualized | All four reconcile exactly and total -$2,505.26 resting-side settlement markout, but rank 33–258 by loss rather than being the four largest cases. | `historical-summary.json`. |
| The experiments demonstrate traction. | Prohibited | No simulated account, fill, volume, PnL, or order is customer activity. | Protocol publication boundary. |

## Follow-up issues

- [#200](https://github.com/MetaB0y/sybil/issues/200): design atomic
  cancel/replace semantics for acknowledged deferred MM bundles;
- [#201](https://github.com/MetaB0y/sybil/issues/201): model adaptive maker
  competition and clearing-price transfers in FBA, carrying forward the
  negative prior art from closed [#190](https://github.com/MetaB0y/sybil/issues/190);
- [#202](https://github.com/MetaB0y/sybil/issues/202): capture timestamped quote
  lifecycles for calibrated market-structure replay.
