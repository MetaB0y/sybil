---
adr: 0013
title: Exit & escape model — sell-then-withdraw-cash; escape values positions at last clearing price
status: Accepted
date: 2026-07-07
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0013 — Exit & escape model

Refines [ADR-0005](0005-escape-via-operator-replacement.md) (which said escape is
cash-only, positions frozen). Founder steer 2026-07-07: value escaped positions at
the **last batch clearing price** ("prev better" than 50/50 or an L1 oracle).

## Context

Two ways value leaves the system, and the founder asked exactly how each works:
*can users withdraw positions or only cash? how do they sell positions? would the
oracle resolve on L1, or 50/50, or last batch price?*

## Decision

1. **Normal exit = sell-then-withdraw-cash.** A position is exited by **selling it
   in a batch** (normal trading) to get cash, then withdrawing the cash. There is
   **no "withdraw a position" object** and no position-transfer/force-settle
   outside trading. One mechanism, matches today's `WithdrawalLeaf` (cash only).
2. **Escape (operator gone) values positions at the last clearing price.** In
   escape mode a user's payout is:
   `withdrawable = cash + Σ_market (position_qty × last_clearing_price[market]) − open_cash_reservations`,
   floored at 0 and capped at the account's backing. Paid as cash on L1.
   **No L1 oracle; no 50/50.** The last clearing price is the market's most recent
   batch price — the fairest oracle-free estimate of a position's worth.
3. **L1 never resolves markets.** Resolution stays off-chain in the sequencer
   ([[Market Resolution]]); escape never needs an outcome, only a price.

## Why last-clearing-price is safe (and simple)

For binary markets, a YES+NO share pair is minted from exactly $1
([[Minting]]) and coherent clearing makes `price(YES) + price(NO) ≈ $1`
([ADR-0001](0001-eg-fisher-market-matching.md)). So valuing *all* positions at
last-clearing-price and paying out ≈ conserves the collateral — the vault stays
~solvent by construction. Per the simplicity ethos
([ADR-0011](0011-validium-stance-no-backcompat.md)) we accept that impractical
edge cases (prices not summing to exactly $1 by a rounding unit) **bend**: the
per-account cap at backing + floor at 0 keep the vault safe, and any dust is
immaterial. No dispute mechanism, no per-position oracle — deliberately.

## Alternatives considered

- **Cash-only, positions frozen** (prior ADR-0005). Rejected as the default —
  worse UX; a user's open exposure is stranded until operator replacement, even
  though a perfectly good last price exists.
- **L1 oracle resolves outcomes in escape.** Rejected — puts an oracle on-chain
  (large surface, griefable) to answer a question a price already approximates.
- **50/50 / par valuation.** Rejected — arbitrary and unfair to positions the
  market had priced far from 0.5.

## Consequences

**Good:** escape returns *fair* value for open positions, not just cash — much
better than freezing them — with zero new trust (no oracle) and near-perfect
solvency by construction. Simple and elegant.

**Costs / constraints:** the **last clearing price per market must be committed in
the state root** (it exists today only in sequencer `price_tracker` state, used
for portfolio marking) — the escape guest must prove it against the root, so it
becomes a proven market-leaf field. Positions in a market that **never cleared a
batch** have no last price → treat as 0 (or the mint reference); a rare,
impractical edge that bends. This partially supersedes ADR-0005 (positions are
now *valued* in escape, not only recovered via replacement — replacement remains
the path to *continue trading*, not to exit).

**Follow-ups:** commit `last_clearing_price` per market leaf; the escape-claim
guest ([historical escape-claim design](https://github.com/MetaB0y/sybil/blob/main/design/archive/implemented/escape-claim-guest.md)) computes the
formula above; escape-mode UX may be rough but must be tested rigorously
([ADR-0011](0011-validium-stance-no-backcompat.md)).
