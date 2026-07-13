# Sybil web frontend

The Next.js client for browsing markets, connecting a P256/WebAuthn account,
submitting signed orders, watching blocks, inspecting a portfolio, and viewing
the arena. It consumes the public `sybil-api`; it does not contain exchange
state or matching logic.

## Local development

Requirements: Node 24+ and pnpm 11+.

```bash
cd frontend/web
pnpm install --frozen-lockfile
cp .env.example .env.local       # adjust API/WS origins if needed
pnpm dev
```

The default environment targets the shared devnet. For a local API, set
`NEXT_PUBLIC_API_BASE` and `NEXT_PUBLIC_WS_BASE` to the matching HTTP and
WebSocket origins.

## Checks

From the repository root, `just frontend-check` runs the same install,
typecheck, lint, unit-test, and production-build sequence as CI. Individually:

```bash
pnpm tsc --noEmit
pnpm lint
pnpm test
pnpm build
pnpm e2e                 # requires the configured app/API environment
```

## Data and trust boundaries

- REST and realtime transport live under `src/lib/api/`.
- Canonical signing and WebAuthn helpers live under `src/lib/auth/`.
- The browser stores account-session material; server state remains authoritative.
- `src/lib/api/schema.d.ts` is generated from `sybil-api`'s OpenAPI document.
  It stays committed so frontend builds do not need a running Rust server.
  `pnpm types:generate` exports `ApiDoc` directly from the local Rust revision,
  applies the bigint declaration workaround, and formats the result
  deterministically; it never depends on a deployed API. `pnpm types:check`
  fails when the committed declaration has drifted.
- Read API keys are read-only. Orders, cancels, key changes, and withdrawals
  require a registered signing key.

See [`DATA_MAP.md`](../DATA_MAP.md) for endpoint-to-screen coverage and
[`docs/architecture/05-interfaces/REST API.md`](../../docs/architecture/05-interfaces/REST%20API.md)
for the backend contract. Before changing frontend code, read the local
[`AGENTS.md`](AGENTS.md); this repo's Next.js version may differ from familiar
framework conventions.
