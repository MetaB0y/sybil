# `sybil-oracle`

Resolution authorization and lifecycle policy. It verifies/represents who may
attest to a payout; the sequencer performs irreversible settlement. This crate
does not fetch external data, move balances, or implement bond escrow.

## Read first

- [[Market Resolution]]
- [[Threat Model]]

## Current implementation

`ResolutionPolicy` currently has one executable variant:
`Immediate { feed_id }`. A registered feed signs a market-bound payout
attestation; signature/feed/market/payout/state checks succeed, then the market
settles immediately. The trusted admin path calls the same immediate state
transition directly for dev/tests. Canonical lifecycle state is deliberately
limited to `Active` and `Resolved`; richer adjudication requires a complete new
policy design rather than reserved protocol shapes.

## Modules

| Module | Owns |
|---|---|
| `attestation.rs`, `feed.rs` | Signed payout attestations and feed identities |
| `registry.rs`, `template.rs` | Feed/template registration |
| `policy.rs` | Executable immediate policy |
| `types.rs` | Executable lifecycle state and completed resolution records |

Payouts are nanodollars in `[0, 1_000_000_000]`; NO receives the complement.
Resolution is irreversible at the sequencer boundary. Run
`cargo test -p sybil-oracle` and the API/oracle integration tests.
