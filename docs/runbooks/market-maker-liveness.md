---
tags: [runbook, market-maker, observability]
status: current
last_verified: 2026-07-20
---

# Market-maker liveness

> **Executive summary:** an MM is healthy only when it is fresh, in normal
> risk mode, covering at least 95% of eligible markets, and getting a bundle
> accepted within two blocks. PnL is diagnostic in prelaunch, not a paging
> threshold. Investigate every persistent breach until the invariant recovers.

**Alerts:** `NativeMarketMakerNotReady`,
`NativeMarketMakerNoQuotes`, `NativeMarketMakerCoverageLow`,
`NativeMarketMakerTwoSidedCoverageLow`,
`NativeMarketMakerProductLiquidityLow`,
`NativeMarketMakerSubmissionStalled`, `NativeMarketMakerReduceOnly`,
`NativeMarketMakerPairedInventoryStuck`, and their Polymarket equivalents.

## Operating contract

| Signal | Healthy bar | Meaning |
| --- | --- | --- |
| `*_ready` | `1` | The owner process has passed all quote-loop checks. |
| quoted / eligible | at least 95%; normally 100% | Optional inventory work must not displace baseline market coverage. |
| two-sided / eligible | at least 95% in normal mode | Markets have both cash-backed YES and NO quotes. |
| synthetic liquid / eligible | at least 95%; normally 100% | The public market view has nonzero rolling liquidity, closing the gap between generated quotes and product behavior. |
| `*_submission_lag_blocks` | at most 2 | The API is accepting current IOC bundles. |
| `*_mode{mode="normal"}` | `1` | Directional exposure is below the configured cap. |
| `*_paired_position_units` | drains when elevated | YES+NO complete sets are automatically sold together and converted back to cash. |
| `*_quote_capacity_limited` | `0` | The order cap is not omitting eligible markets. |

The metric prefixes are `sybil_native_mm_` and `sybil_polymarket_mm_`.
Polymarket also exposes `*_ineligible_markets{reason=...}` for missing, stale,
and out-of-band references. Account cash, deposits, marked value, PnL,
conservative cash-plus-complete-set floor, and worst-case drawdown are shown in
Grafana. Negative PnL alone is not an incident in prelaunch.

## Triage

1. Open the Native MM or Polymarket MM rows in the Sybil Grafana dashboard.
   Confirm whether the failure is eligibility, coverage, submission, risk, or
   redemption. Compare actor-local quote coverage with the external
   `product liquid` series; a gap means accepted quotes are not reaching the
   displayed price/liquidity projection.
2. Read the owner readiness payload and recent logs:

   ```bash
   docker compose exec -T sybil-native-mm curl -fsS http://127.0.0.1:9104/readyz
   docker compose exec -T sybil-polymarket curl -fsS http://127.0.0.1:9105/readyz
   docker compose logs --since=15m sybil-native-mm sybil-polymarket sybil-api
   ```

3. Follow the first failing invariant:

   - **No eligible markets:** for Polymarket, compare missing/stale/out-of-band
     counts with feed token age. For native markets this is unexpected because
     catalog anchors do not depend on an external feed.
   - **No or partial coverage:** compare eligible, quoted, two-sided, quote
     order count, position inventory, and the capacity-limited gauge. A larger
     catalog needs at least two baseline order slots per eligible market.
   - **Product liquidity low:** inspect markets where
     `liquidity_avg10_nanos` is zero. Compare their public YES mark with the
     MM bid/ask center. Check that NO limits are mapped to the complementary
     YES price and that no-fill marks include the current flash bundle.
   - **Submission stalled:** inspect the structured API rejection and the
     submission-failure counter. Confirm the MM account still exists and the
     service token authorizes account reads and order submission.
   - **Reduce-only:** directional exposure crossed the configured cap. Confirm
     directional inventory is falling. Do not raise the cap merely to clear
     the alert without understanding the accumulated position.
   - **Paired inventory stuck:** compaction quantity should be nonzero while
     paired inventory falls. If it does not, inspect compaction failures,
     ordinary IOC admission, and settlement; paired YES+NO is collateral, not
     directional exposure and redemption does not use the MM quote budget.

4. Correlate actual market activity separately. If trader submissions are
   present but fills remain zero, `LiveTradingNoFills` identifies a matching or
   crossing problem even when the MM quote loop itself is healthy.

## Recovery proof

Keep the incident open until all of these hold continuously for 10 minutes:

- readiness is `1`;
- quoted and two-sided coverage are each at least 95%;
- external nonzero-liquidity coverage is at least 95%;
- accepted-submission lag is at most two blocks and successes keep increasing;
- risk mode is normal;
- paired inventory is below 1,000 shares or is visibly decreasing; and
- the corresponding vmalert rules have cleared.

Capture the readiness payload, relevant Grafana interval, rejection logs, root
cause, and durable fix. A restart that only resets counters is not a fix.

## Validation

Run `just monitoring-check` after changing metrics, rules, or dashboards. It
validates Compose, Prometheus configuration, alert syntax, and the checked-in
promtool scenarios.
