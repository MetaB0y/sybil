# `sybil-api`

Axum transport/operations around `SequencerHandle`; exchange mutation and
settlement remain sequencer-owned.

## Read first

- [[REST API]], [[WebSocket Block Stream]], [[P256 Authentication]], and
  [[Deployment Profiles]]

## Sources of truth

- Runtime schema: `GET /openapi.json`
- Mounted route policy: registries in `src/app.rs` and the reviewed
  `tests/fixtures/route-policy.snapshot`
- Shared DTOs: `sybil-api-types`; handlers: `src/routes/`

## Trust boundaries

- Public, service, and dev routes are distinct; dev routes are absent in
  production and service routes require `SYBIL_SERVICE_TOKEN`.
- Public onboarding receives a server-selected fixed grant and has separate
  lifetime account stock and rate budgets. Account IDs are never reused.
- Locked-profile preflight fails closed on dev mode, missing auth/persistence,
  and invalid WebAuthn configuration.
- Proxy headers affect client identity only from explicitly trusted peer CIDRs.
- Public trading accepts only the supported single-market shapes.
- First-key bootstrap is zero-key-only; later key changes are signed,
  state-bound operations.
- Signed actions bind genesis and monotonic nonce. WebAuthn also binds RP ID,
  origin, challenge, and UP/UV policy.
- Read API keys cannot mutate state.

API changes require OpenAPI drift/route-policy tests and regeneration checks
for both frontend and Python clients.
