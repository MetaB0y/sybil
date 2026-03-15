# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this crate.

## Purpose

The **sybil-oracle** crate provides a pluggable resolution system for prediction markets. It makes authorization and lifecycle decisions — the sequencer executes them. It does NOT perform settlement, fetch external data, or handle bond escrow.

## Architecture Notes

Before modifying this crate, read these vault notes (`docs/architecture/`):
- [[Oracle Lifecycle]] — resolution flow: propose, challenge, finalize
- [[Market Resolution]] — payout model and fractional resolution

## Market Lifecycle

```
Active → Proposed (challenge window) → Challenged (L1 adjudication) → Resolved
                                    → Resolved (unchallenged)
```

## Oracle Trait

```rust
trait Oracle {
    fn resolve(&self, market_id, payout_nanos, source) -> ResolutionAction;
    fn challenge(&self, proposal_id, challenger, bond, alt_payout) -> ChallengeAction;
    fn check_finalization(&self, market_id, current_time_ms) -> Option<ResolutionAction>;
}
```

**ResolutionAction:**
- `SettleNow` — immediate resolution (admin/L0)
- `Propose { challenge_window_ms }` — initiate challenge period
- `Reject { reason }` — refuse resolution

**ChallengeAction:**
- `Escalate` — escalate to L1 adjudication
- `Reject { reason }` — refuse challenge

## Oracle Sources

| Source | Purpose |
|--------|---------|
| `Admin` | Dev mode, governance multisig |
| `AutomatedL0` | Future: price feeds, API oracles |
| `AdjudicationL1` | Human adjudication panel |

## Key Types

| Type | Purpose |
|------|---------|
| `MarketStatus` | Active, Proposed, Challenged, Resolved, Voided |
| `ResolutionProposal` | Proposed payout + source + timestamp + reason |
| `Challenge` | Challenger + bond + alternative payout + reason |
| `ResolutionRecord` | Immutable record of completed resolution |
| `ProposalId` / `ChallengeId` | Unique identifiers (u64 newtypes) |

## Payout Model

All payouts in nanos (1B = $1):
- YES shares receive `payout_nanos`
- NO shares receive `NANOS_PER_DOLLAR - payout_nanos`
- Fractional resolution supported (e.g., 70%/30%)

## AdminOracle

Reference implementation for dev/testing:
- Immediately settles (no challenge window)
- Validates payouts within [0, NANOS_PER_DOLLAR]
- Rejects already-resolved markets
- Does not support challenges

```rust
let oracle = AdminOracle::new();
let action = oracle.resolve(market_id, payout_nanos, OracleSource::Admin);
// → ResolutionAction::SettleNow { ... }
```

## Module Map

| Module | Purpose |
|--------|---------|
| `types.rs` | MarketStatus, proposals, challenges, records |
| `traits.rs` | Oracle trait and action enums |
| `admin.rs` | AdminOracle implementation |
| `error.rs` | OracleError variants |
