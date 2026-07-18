# `sybil-api-types`

Dependency-light source of truth for REST and WebSocket DTOs. The server,
shared Rust client, generated Python client, and generated TypeScript schema
derive their wire contract from here.

## Read first

- [[REST API]], [[WebSocket Block Stream]], and [[Block Data Boundaries]]

## Boundaries

- Keep protocol units explicit in field names/docs; nanos and share-units are
  integers, not display values.
- Additive fields need serde defaults when old persisted/wire payloads may be
  decoded. Breaking changes are acceptable only as deliberate coordinated
  migrations.
- Canonical state/signing serialization does not live here.
- Do not hand-edit generated frontend or Python shapes; regenerate them after
  DTO/OpenAPI changes.

Public-surface changes require the frontend and arena schema-generation gates.
