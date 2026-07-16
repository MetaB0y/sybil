# `sybil-api`

Axum transport and operations layer around `SequencerHandle`. It owns routing,
OpenAPI, deployment preflight, CORS/rate limits, WebAuthn ceremonies, realtime
streams, proof/DA endpoints, and conversion to shared `sybil-api-types`. It does
not own exchange mutation or settlement rules.

## Read first

- [[REST API]] and [[WebSocket Block Stream]]
- [[P256 Authentication]]
- [[Deployment Profiles]]

## Sources of truth

Do not maintain an endpoint list here:

- runtime schema: `GET /openapi.json`;
- route policy and mounted tables: `src/app.rs` plus route-policy tests;
- shared DTOs: `crates/sybil-api-types`;
- handlers: `src/routes/`.

## Trust boundaries

- Public, service, and dev route groups are explicit. Production service routes
  require `SYBIL_SERVICE_TOKEN`; dev-only routes are not mounted in prod.
- Public onboarding accepts only an initial key and receives a server-selected
  grant. Its durable lifetime account-id stock and dedicated route/client rate
  budgets must remain separate from service/dev explicit funding. Account ids
  are never reclaimed or reused.
- Production preflight fails closed on dev mode, missing service auth,
  persistence, or invalid WebAuthn configuration.
- Browser CORS is same-origin unless an explicit allowlist is configured.
- Forwarded client IP headers are ignored unless the socket peer belongs to an
  explicit `SYBIL_HTTP_TRUSTED_PROXY_CIDRS` network.
- Public trading accepts only supported single-market `OrderSpec` shapes.
- First-key bootstrap is service-gated and zero-key-only. Additional key
  registration/revocation is state-bound and signed by an active key.
- Signed actions bind the genesis domain and strictly increasing account nonce;
  WebAuthn additionally binds RP ID, origin, challenge, and UV/UP policy.
- Read API keys can authorize reads only.

## Code map

| Area | Location |
|---|---|
| Router/OpenAPI/policy | `app.rs` |
| Config/preflight and worker lifecycle | `config.rs`, `preflight.rs`, `main.rs` |
| App state/off-block ref data | `state.rs` |
| REST handlers | `routes/`; account product-history reads in `routes/accounts/history.rs` |
| WebAuthn | `webauthn.rs`, account routes |
| Realtime | `ws.rs` |
| Admin CLI | `bin/sybil_admin.rs` |

When the API surface changes, regenerate/check both frontend and Python clients
and run the OpenAPI drift/route-policy tests.
