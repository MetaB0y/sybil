# Repository guidance

## Workflow

- Use `jj`, not git: inspect with `jj status`, `jj log`, and `jj diff --git`.
- Keep every logical change in its own `jj` change; start a new change at each
  boundary. Do not squash distinct changes without a concrete, stated reason.
- GitHub Issues in `MetaB0y/sybil` are authoritative; do not use Linear. Set
  `GH_REPO=MetaB0y/sybil` for `gh`, add active work to Project 1 with Stage and
  Priority, and keep durable decisions in docs/ADRs.

## Sources of truth

- Read the nearest `AGENTS.md`. Every workspace crate has one; `just docs-sync`
  checks required instruction roots.
- `docs/SPEC.md` is the linear overview. `docs/architecture/` is canonical
  current architecture; local files route to the relevant notes.
- `design/` contains proposals/research and `design/archive/` is historical.
- Source, generated schemas, and `--help` output own inventories, defaults, and
  CLI/API contracts. Do not duplicate them in instruction files.
- Keep local `AGENTS.md` files limited to non-obvious boundaries, invariants,
  generated-file ownership, and specialized checks.

## Global invariants

- Protocol money, quantities, settlement, commitments, and verification are
  integer-only. Floating-point search or display logic stays outside protocol
  truth and behind integer landing/verification.
- Prefer supervised actors with owned state over shared mutable state.
- Early development favors clean design over compatibility, but wire,
  persistence, signature, and validity changes remain explicit coordinated
  migrations.
- Do not hand-edit generated OpenAPI clients, golden vectors, protocol pins, or
  guest commitments; use their owning regeneration workflows and review diffs.
- GitHub issue comments are not architecture records. Update current docs after
  significant data-flow, trust-boundary, or ownership changes.

## Checks

- Use the focused gate in the nearest `AGENTS.md`.
- `just check-fast` is the normal Rust gate; `just check-features` exercises
  optional surfaces, `just check-consensus` checks validity artifacts, and
  `just check-all` is the complete CI-equivalent gate.
- Follow `DEPLOY.md` and `deploy/AGENTS.md` for operations. Never infer deployed
  validity state from source pins.
