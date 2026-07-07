# Architecture Decision Records

An **ADR** captures one architecturally-significant decision: the forces that
shaped it, the option chosen, the options rejected, and the consequences we now
live with. ADRs are how this project keeps its *why* — the rationale that is
otherwise lost the moment a design doc is superseded or a Slack thread scrolls
away.

## Why we keep them

Sybil is a consensus system: many of its choices are **load-bearing and
expensive to reverse** (they're baked into the guest commitment, the state-root
schema, or the L1 contracts). When a future change proposes to touch one of
these, the first question is always *"why is it this way?"* — and today that
answer is scattered across math proofs (`design/*.typ`), vault notes
(`docs/architecture/*`), and the code review (`docs/review/*`). An ADR is the
single place that answer lives.

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

| ADR | Title | Status | Consensus-critical? |
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

> These first ten are **backfilled** from decisions already made and already in
> production; their rationale was reconstructed from the design/review estate
> (sources cited in each). New decisions from here forward should get an ADR
> *at the time they're made*.
