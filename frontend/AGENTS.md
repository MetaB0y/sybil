# `frontend`

- `web/` is the active application; `archive/` is rationale, not current state
  or backlog. `DATA_MAP.md` owns screen-to-API coverage.
- `web/src/lib/api/schema.d.ts` is generated from the local `sybil-api`
  OpenAPI document. Do not hand-edit it; use `pnpm types:generate`.
- JSON `*_nanos` values are exact decimal strings. Parse them as `bigint` and
  convert only at display boundaries.
- Read [[REST API]] and [[P256 Authentication]] for transport or signing
  changes. `just frontend-check` is the maintained gate.
