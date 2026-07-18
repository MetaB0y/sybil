# `sybil-client`

The single shared Rust client for `sybil-api`. In-tree Rust consumers extend
this crate rather than creating another HTTP/WebSocket wrapper.

## Read first

- [[REST API]], [[WebSocket Block Stream]], and [[P256 Authentication]]

## Boundaries

- Stay typed against `sybil-api-types`; do not duplicate DTOs.
- Preserve service-token handling, structured API errors, TLS defaults, and
  resumable realtime semantics.
- Client helpers may prepare requests but canonical signature bytes remain in
  `sybil-signing`.
- Examples are smoke/development tools; never log or persist private keys.
