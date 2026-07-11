---
tags: [client, cli, proposal]
status: proposed
last_verified: 2026-07-11
---

# User CLI Plan

## Goal

Evolve the current operator-only `sybil-admin` binary into a broader user-facing CLI
that agents and humans can use for normal trading workflows, while keeping
administrative market curation as a smaller scoped surface within the same overall
client story.

This deliberately excludes a `market close` concept. Market lifecycle should remain:

- creation
- trading
- resolution via oracle

If production authorization is an admin key, multisig, or other governance key, that
should be expressed through the oracle path rather than a separate "close" action.

## End State

Target binary shape:

```text
sybil
  market   # read-only discovery
  account  # balances, keys, portfolio, fills
  order    # submit, cancel, inspect
  block    # latest, get, stream
  admin    # market create / market resolve
```

The current `sybil-admin` binary should be treated as the first thin slice of this,
not the final UX.

## Principles

- One shared HTTP client layer for all CLI commands.
- JSON output must be a first-class mode so agents can script reliably.
- User commands should default to signed/authenticated flows once P256 registration is available.
- Admin commands should be narrow. Avoid lifecycle verbs that do not correspond to real protocol concepts.
- Read-only commands should work against production safely without dev-mode assumptions.

## Phases

### Phase 1: Read-Only User Surface

Ship a general `sybil` CLI with:

- `sybil market list`
- `sybil market get <id>`
- `sybil market search ...`
- `sybil account get <id>`
- `sybil account portfolio <id>`
- `sybil account fills <id>`
- `sybil block latest`
- `sybil block get <height>`

Why first:

- immediately useful for agents
- no signing or key management complexity
- exercises output format and filtering decisions

### Phase 2: Auth + Order Flow

Add user trading commands:

- `sybil key generate`
- `sybil account register-key`
- `sybil order buy-yes`
- `sybil order buy-no`
- `sybil order sell-yes`
- `sybil order sell-no`
- `sybil order submit --file order.yaml`
- `sybil order cancel --order-id ...`
- `sybil order pending [--account-id ...]`
- `sybil order book --market-id ...`

Design choice:

- support both ergonomic subcommands and a file-driven spec path
- keep canonical signing in one place in a shared client module

### Phase 3: Admin Surface Consolidation

Fold the existing admin workflow into:

- `sybil admin market create --file spec.yaml`
- `sybil admin market resolve --market-id ... --yes|--no|--payout-nanos ...`

This can still use dev-mode endpoints locally, but the long-term production path should
be oracle-authorized rather than dev-mode-gated.

### Phase 4: Agent Ergonomics

Add features specifically useful for autonomous traders:

- `--json` on every command
- stable machine-readable error codes
- environment-based profile selection (`dev`, `staging`, `prod`)
- idempotency helpers for order submission
- optional SSE helpers for block stream consumption

## Recommended Packaging

Short term:

- keep `sybil-admin` as-is for immediate market curation
- extract its HTTP client into a reusable module

Next step:

- add a new `sybil` binary in `crates/sybil-api` or a small adjacent crate
- share transport, serialization, and auth code between `sybil-admin` and `sybil`

Longer term:

- either deprecate `sybil-admin` in favor of `sybil admin ...`
- or keep `sybil-admin` as a thin alias wrapper around the shared CLI core

## Open Questions

- Should the general CLI live under `sybil-api`, or should there be a dedicated `sybil-cli` crate?
- When user trading commands land, do we want YAML-first specs, ergonomic flags first, or both at once?
- How much of account creation/funding should exist in the general CLI versus remaining dev-only helpers?
- Should block streaming be exposed directly in the CLI, or left to SDKs/scripts?
