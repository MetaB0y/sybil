---
adr: 0011
title: Project stance — private validium, single operator, no backward-compat, simplicity-first
status: Accepted
date: 2026-07-07
validity_critical: false
supersedes: []
superseded_by: []
---

# ADR-0011 — Project stance: private validium, single operator, simplicity-first

## Context

Several earlier ADRs and designs (0001–0010) used framing that quietly assumed
things that aren't true for this project — most importantly the word
"consensus," a fear of schema changes, and backward-compatibility. Valery
(founder, 2026-07-07) corrected the stance. This ADR records it once so every
other decision inherits it.

## Decision

Four standing positions:

1. **It is a validium with a single sequencer and a single prover — not a
   consensus system.** There is no multi-party agreement. The correct word for
   "must be reproduced identically / is committed by the proof" is
   **validity-critical** / **soundness-critical** / **proven**, *not*
   "consensus-critical." (The 0001–0010 ADRs and docs use "consensus"; they get
   a terminology pass.)

2. **No backward compatibility until at least autumn 2026.** We are not live and
   can re-genesis / restart as many times as we like. **Fresh genesis has zero
   cost.** Therefore: no serde-default migration shims, no version-wrapper compat
   types, no "batch changes into one redeploy window," no "reserve a byte slot
   now to avoid a second schema move." A schema change is just: change it and
   restart.

3. **Flatten version wrappers.** Prefer a flat `struct V4 { …all fields… }` over
   `struct V4 { base: V3, … }`, here and everywhere.

4. **Simplicity and elegance are paramount; practical over rigorous.** Prefer the
   simple/elegant design even if edge cases we consider *impractical* bend. Do
   not build a complicated system without being sure of the requirements and the
   founder's intent. Strong, flexible foundations — but simple. UX matters (see
   [ADR-0014](0014-webauthn-first-auth.md)); the one exception is escape-mode UX,
   which may be rough (assumed never used) but must still be **tested rigorously**.

## Alternatives considered

- **Keep "consensus" framing / design for backward-compat now.** Rejected — it's
  inaccurate (single operator) and adds complexity we're explicitly not paying
  for pre-launch.

## Consequences

**Good:** deletes a whole layer of complexity from every downstream decision
(migration, batching, byte-reservation, compat shims); makes schema evolution
trivial while pre-launch; keeps the codebase simple and honest about what it is.

**Costs / constraints:** when we *do* go live (≥ autumn 2026), a
backward-compat / migration story must be designed then — this ADR's "just
restart" freedom ends at first real user funds. Track that as a hard boundary.
The terminology pass across 0001–0010 + docs is owed.

**Follow-ups:** terminology pass (consensus → validity/proven); this stance
moots the D1 "reserve the capability-mask slot now" reasoning in the ratification
packet (just add it when built).
