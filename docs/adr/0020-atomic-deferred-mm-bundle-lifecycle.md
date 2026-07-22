---
adr: 0020
title: Authenticated atomic lifecycle for deferred MM bundles
status: Accepted
date: 2026-07-22
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0020 — Authenticated atomic lifecycle for deferred MM bundles

## Context

Market-maker quote bundles are acknowledged and durably deferred until the next
batch, but cannot currently be canceled or replaced. A stale bundle can
therefore execute even when its maker reacted before the block cutoff. Treating
its orders independently is unsafe: the quotes share one flash-liquidity budget
and may span a `MarketGroup`, so partial removal, replacement, or admission can
change the maker's risk and self-trade-prevention assumptions.

The preregistered held-out experiment retained under
`benchmarks/market-structure/results/cancel-lifecycle-heldout-2026-07-22-v1`
compared current behavior, whole-bundle cancel, and whole-bundle replacement.
Cancel and replace removed modeled stale fills when they won the cutoff race,
but also reduced fills and trader surplus. Replacement preserved displayed
coverage and avoided cancel-then-submit delay. This is a risk-control tradeoff,
not a free welfare improvement.

Authorization and replay order are validity-sensitive. The lifecycle therefore
touches canonical signing bytes, the account replay nonce, block-witness
authorization, the acknowledged-write WAL, OpenAPI clients, and the guest
commitment. Settlement arithmetic, shared-budget accounting, and state-root
balances do not need new numeric semantics.

## Decision

Deferred MM liquidity has an **authenticated, whole-bundle, actor-owned
lifecycle**. Public makers submit a signed atomic bundle and may later sign an
atomic replacement or cancellation. The operator-only service route remains an
explicit privileged ingestion boundary for first-party supervised makers; it
cannot impersonate a public signature in the witness.

An active bundle is identified by `(account_id, bundle_id, revision)`. The
client chooses an opaque 32-byte `bundle_id`; initial submission is revision
zero. Replacement names the exact active revision and installs revision plus
one. Cancellation names the exact active revision and removes it. Every signed
action also carries the account's strictly increasing trading nonce and the
genesis-bound canonical signing domain. The authenticated account, never a
request-supplied maker identifier, owns the bundle and its MM constraint.

There is at most one active deferred MM bundle per account. Submission,
replacement, and cancellation are all-or-nothing across every order, market,
group interaction, and the single shared integer budget:

```text
Absent --submit(r=0)--> Pending(r)
Pending(r) --replace(expected=r, new=r+1)--> Pending(r+1)
Pending(r) --cancel(expected=r)--> Absent
Pending(r) --block preparation--> Clearing(r) --commit/reject--> Absent
```

The sequencer actor's processing order is the cutoff. A lifecycle message that
the actor processes before the block-production message may change the pending
bundle. Once block preparation has dequeued it into the isolated candidate,
the bundle is no longer pending: a later cancel or replace returns a terminal
`not_pending` result and cannot rewrite the candidate. Client timestamps never
decide this race.

The actor validates the complete candidate against a clone, appends one
acknowledged-write row, and only then swaps live pending state. A failed check or
WAL append changes nothing. Replacement is one WAL action, not a cancel followed
by a submission. Recovery replays the same global actor order and reconstructs
the same active revision. The block commit fence consumes all pending lifecycle
rows with the normal acknowledged-write interval.

An exact retry of the latest action is idempotent and returns its recorded
result. Reusing its nonce or operation digest for different bytes is rejected;
an older revision or nonce is stale. The bounded retry receipt is operational
WAL/recovery state, not a new settlement balance. API responses distinguish
accepted, stale revision, conflicting retry, not pending/too late, validation
failure, and durability failure. Public streams publish only committed block
effects; the synchronous durable receipt is authoritative before the cutoff.

The validity witness carries the exact signed bundle action in actor order and
binds accepted or rejected block effects to it. The signing and witness schemas
advance together under a fresh genesis, with regenerated vectors, OpenAPI
clients, guest fingerprints, commitments, and protocol pins. Old stores and
witnesses are rejected rather than translated.

## Alternatives considered

- **Keep bundles non-cancellable.** Operationally simple, but leaves a known
  interval in which an acknowledged quote must remain live after its owner has
  reacted.
- **Expose cancellation only.** Necessary for withdrawal, but encourages a
  cancel-then-submit refresh that loses a batch and creates a displayed
  liquidity hole. The held-out comparison favored atomic replacement for
  refresh without claiming a welfare improvement.
- **Cancel individual orders or partially accept a replacement.** Rejected
  because it breaks the shared-budget and grouped-market contract. The unit the
  maker signed is the unit the sequencer admits, replaces, cancels, and proves.
- **Let timestamps beat the cutoff.** Rejected because clocks are not a
  deterministic ownership boundary and would allow a late request to rewrite
  an already prepared candidate.
- **Use a concurrent bundle registry beside the actor.** Rejected because it
  creates a second writer and an ordering race with block preparation and the
  acknowledged-write WAL.

## Consequences

**Good:** makers can withdraw stale liquidity or refresh it without a coverage
gap; exact ownership, replay, and cutoff behavior is deterministic; a crash
cannot lose an acknowledged lifecycle action; grouped quotes and shared budgets
remain atomic; validity proves the user intent that selected the candidate
bundle.

**Costs / constraints:** successful stale-risk control deliberately reduces
some fills and trader surplus; one active bundle per account requires makers
that need independent strategies to use independent authenticated accounts;
signature, witness, store, guest, API, and client artifacts migrate together;
the actor remains the single lifecycle throughput boundary. A maker whose
action loses the cutoff must observe the terminal result and construct a fresh
revision after that block.

**Implementation:** #200 owns the lifecycle and #174 owns the signed public
atomic-bundle entry point it builds on. The change requires a fresh genesis and
coordinated repin before any later deployment; this ADR does not authorize a
deployment.

**Follow-ups:** production telemetry must separate submit, replace, cancel,
retry, stale, and cutoff outcomes. Any future relaxation of one active bundle
per account or addition of automated cancellation needs a new decision because
it changes budget isolation and market-structure behavior.
