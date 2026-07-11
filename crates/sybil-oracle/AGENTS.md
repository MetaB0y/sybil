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
settles immediately. `AdminOracle` is a compatibility facade over the same
immediate state transition for dev/tests.

`MarketStatus::Proposed`, `Challenged`, and `Voided`, plus the `Oracle`
challenge/finalization methods, are reserved extensibility—not evidence that an
optimistic/quorum adjudication system exists. Resolver-side review windows in
`sybil-polymarket` do not change this core trust boundary.

## Modules

| Module | Owns |
|---|---|
| `attestation.rs`, `feed.rs` | Signed payout attestations and feed identities |
| `registry.rs`, `template.rs` | Feed/template registration |
| `policy.rs` | Executable immediate policy |
| `types.rs` | Lifecycle records and reserved states |
| `traits.rs`, `admin.rs` | Compatibility oracle interface/facade |

Payouts are nanodollars in `[0, 1_000_000_000]`; NO receives the complement.
Resolution is irreversible at the sequencer boundary. Run
`cargo test -p sybil-oracle` and the API/oracle integration tests.
