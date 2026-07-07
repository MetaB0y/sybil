---
adr: NNNN
title: <short imperative phrase>
status: Proposed
date: YYYY-MM-DD
validity_critical: false
supersedes: []
superseded_by: []
---

# ADR-NNNN — <Title>

## Context

What forces are in play? What problem are we solving, and what constraints
bound the solution (performance, validity, security, effort, the shape of the
existing system)? State the facts a reader needs to judge the decision, not the
decision itself. If this touches the guest commitment / state root / L1
contracts, say so here.

## Decision

The choice, stated plainly in one or two sentences. Then the *mechanism*: how it
works, concretely enough that someone could find it in the code.

## Alternatives considered

- **<Option A>** — why it was rejected (or deferred).
- **<Option B>** — why it was rejected.

Be fair to the rejected options — an ADR that strawman's the alternatives is
worthless six months later when someone reconsiders.

## Consequences

**Good:** what this buys us.

**Costs / constraints:** what it makes harder, what it locks in, what future
work it complicates. Every real decision has these — name them.

**Follow-ups:** tickets or later ADRs this implies.
