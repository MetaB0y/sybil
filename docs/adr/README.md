# Architecture Decision Records

An **ADR** captures one architecturally-significant decision: the forces that
shaped it, the option chosen, the options rejected, and the consequences we now
live with. ADRs are how this project keeps its *why* — the rationale that is
otherwise lost the moment a design doc is superseded or a Slack thread scrolls
away.

## Why we keep them

Sybil is a single-operator validium: many of its choices are **load-bearing and
expensive to reverse** (they're baked into the guest commitment, the state-root
schema, or the L1 contracts). When a future change proposes to touch one of
these, the first question is always *"why is it this way?"* — and today that
answer may begin in math proofs (`design/*.typ`), architecture notes, or a dated
review. An ADR is the stable place the accepted answer lives.

A good ADR is **short** (one page), **honest about trade-offs** (every decision
costs something), and **immutable once accepted**. We do not edit an accepted
ADR to reflect a new decision — we write a new ADR that supersedes it. The trail
of superseded records *is* the architectural history.

## How to use them

1. Copy [`0000-template.md`](0000-template.md) to the next number.
2. Fill in Context → Decision → Alternatives → Consequences. Keep it tight.
3. Set status `Proposed`; open it for review. On agreement, flip to `Accepted`.
4. If a later ADR overturns it, set this one to `Superseded by ADR-NNNN` and say
   so in the new record's Context.

Write an ADR when a decision is **hard to reverse**, **cross-cutting**, or
**surprising** — something a competent newcomer would ask "wait, why?" about.
Do *not* write one for a local, easily-changed implementation choice.

## Status vocabulary

`Proposed` · `Accepted` · `Superseded by ADR-NNNN` · `Deprecated` (no longer
relevant, not replaced).

## Index

| ADR | Title | Status | Validity-critical? |
|---|---|---|---|
| [0001](0001-eg-fisher-market-matching.md) | Eisenberg–Gale / Fisher-market matching (not LMSR or a CLOB) | Accepted | Yes |
| [0002](0002-qmdb-state-single-commit-fence.md) | QMDB authenticated state behind a single redb commit fence | Accepted | Yes |
| [0003](0003-guest-host-crate-split.md) | Guest-safe / host-only crate split | Accepted | Yes |
| [0004](0004-float-search-integer-truth.md) | Float-search, integer-truth | Accepted | Yes |
| [0005](0005-escape-via-operator-replacement.md) | Escape via operator replacement; L1 recovers cash only | Accepted | Yes |
| [0006](0006-witness-v3-full-snapshot.md) | Block witness = full-state snapshot per block | Accepted | Yes |
| [0007](0007-canonical-bytes-domain-separation.md) | Domain-separated canonical bytes with genesis binding | Accepted | Yes |
| [0008](0008-in-guest-p256-openvm-ecc.md) | In-guest P-256 verification via OpenVM accelerated ECC | Accepted | Yes |
| [0009](0009-fresh-genesis-for-consensus-changes.md) | Fresh genesis for consensus-schema changes in devnet | Accepted | Yes |
| [0010](0010-acknowledged-write-wal.md) | Single-sequenced acknowledged-write WAL (durable-before-live) | Accepted | Yes |
| [0011](0011-validium-stance-no-backcompat.md) | Project stance: private validium, no backward-compat, simplicity-first | Accepted | — |
| [0012](0012-privacy-and-data-availability.md) | Privacy & DA model — public root+proof, private contents | Accepted | Yes |
| [0013](0013-exit-and-escape-model.md) | Exit & escape — sell-for-cash; escape values positions at last clearing price | Accepted | Yes |
| [0014](0014-webauthn-first-auth.md) | WebAuthn / passkeys primary, verified in-guest | Accepted | Yes |
| [0015](0015-deposit-quarantine.md) | Deposit quarantine — unresolvable keys park in committed ledger, frontier never skips | Accepted | Yes |
| [0016](0016-public-market-tape-and-recovery-da-boundaries.md) | Public market tape, private canonical blocks, and distinct recovery DA | Accepted | No |

> These first ten are **backfilled** from decisions already made and already in
> production; their rationale was reconstructed from the design/review estate
> (sources cited in each). New decisions from here forward should get an ADR
> *at the time they're made*.
>
> **0011–0014 capture the 2026-07-07 founder reset** (private-validium framing,
> no backward-compat, WebAuthn-first, escape at last-clearing-price). Where
> 0001–0010 say **"consensus-critical," read "validity-critical"** — the correct
> term for a single-operator validium ([ADR-0011](0011-validium-stance-no-backcompat.md));
> a terminology pass over the older records is owed. Some older ADRs are refined
> by these: 0005→0013 (escape valuation), 0006→0012 (private DA), 0009 is relaxed
> by 0011 (fresh genesis is now free).
