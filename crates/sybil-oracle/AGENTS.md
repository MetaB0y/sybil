# `sybil-oracle`

Resolution authorization and lifecycle policy. It verifies/represents who may
attest to a payout; the sequencer performs irreversible settlement. This crate
does not fetch external data, move balances, or implement bond escrow.

## Read first

- [[Market Resolution]]
- [[Threat Model]]

## Current implementation

Only `ResolutionPolicy::Immediate { feed_id }` is executable. A registered feed
signs a market-bound payout; the trusted dev/admin path reaches the same
irreversible sequencer transition. Canonical lifecycle is only `Active` and
`Resolved`; richer adjudication requires a complete policy design, not reserved
placeholder shapes.

Payouts are integer nanodollars in `[0, 1_000_000_000]`; NO receives the
complement. Policy changes require API/oracle integration coverage.
