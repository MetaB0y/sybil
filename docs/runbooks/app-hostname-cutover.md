---
tags: [runbook, deployment, validity, webauthn]
status: current
last_verified: 2026-07-21
---

# Cutover: serve the app from app.sybil.exchange

## What and why

The app moves from `app.62-171-170-238.nip.io` to `app.sybil.exchange`, and the
trading API from the root nip.io host to `api.sybil.exchange`. Operator
dashboards (arena, grafana, prover) deliberately stay on nip.io — they are
behind Caddy basic auth and have no branding or passkey reason to move.

This cannot be done with a proxy or a redirect. The browser reports the page's
real origin in every WebAuthn assertion, and the guest pins it at
`crates/sybil-verifier/src/key_op_auth.rs`. An iframe does not help either:
`key_op_auth.rs` explicitly rejects `crossOrigin: true`, with a test covering
exactly that case. Changing the pin is a guest repin, so it needs a fresh
genesis.

`deploy/validity-pins.json` already read `status: pending_redeploy`, so the
fresh genesis was already queued — this rides along rather than adding a wipe.

## Decision worth reviewing before you run anything

**RP ID is now the registrable domain `sybil.exchange`, not `app.sybil.exchange`.**

A passkey minted under the apex stays valid across subdomains, so moving the app
again later (to the apex, or to another host) will not force another repin and
wipe. The origin check stays exactly as strict — only
`https://app.sybil.exchange` can authorize an action.

If you disagree, it is a two-line revert: `EXPECTED_WEBAUTHN_RP_ID` and
`EXPECTED_RP_ID_HASH` (the hash is `sha256` of the RP ID string —
`app.sybil.exchange` is `d407219155bab3840e34955222947ddeb5707f2159955c32ee3fa1427783056a`).
`NEXT_PUBLIC_WEBAUTHN_RP_ID` and `SYBIL_WEBAUTHN_RP_ID` must be changed to match.

## Already done, live on the host

- DNS A records for `app.sybil.exchange` and `api.sybil.exchange` → 62.171.170.238.
- Let's Encrypt certs for both, expiring 2026-10-19.
- `app.sybil.exchange` currently 301s to the nip.io app; `api.sybil.exchange`
  returns 404. Both live in `/etc/nginx/sites-available/sybil-exchange`, a
  temporary hand-written file.
- Added `/etc/letsencrypt/renewal-hooks/deploy/reload-nginx.sh`. There was no
  deploy hook on this host, so the webroot-authenticated certs (including the
  existing nip.io one) were renewing without ever reloading nginx — it kept
  serving the expiring cert from memory. Unrelated to this cutover, but it would
  have bitten the new certs in ~60 days.

## Code changes in this PR

Protocol (the part that forces the fresh genesis):

- `crates/sybil-verifier/src/key_op_auth.rs` — `EXPECTED_WEBAUTHN_RP_ID`,
  `EXPECTED_WEBAUTHN_ORIGIN`, `EXPECTED_RP_ID_HASH`.
- `crates/sybil-verifier/src/lib.rs` — re-export `EXPECTED_WEBAUTHN_ORIGIN`.

Two latent bugs the apex RP ID exposed, both fixed:

- `key_op_auth.rs` and `crates/sybil-escape-claim/src/tests.rs` built the test
  origin as `https://{EXPECTED_WEBAUTHN_RP_ID}`. That only worked because the RP
  and the app host used to be the same string. They now use
  `EXPECTED_WEBAUTHN_ORIGIN` directly.
- `frontend/web/Dockerfile` had `ARG NEXT_PUBLIC_WEBAUTHN_RP_ID=` (empty), and
  `docker-compose.yml` passed `""`. Empty means the browser falls back to the
  page's own host — previously harmless because that *was* the pinned RP, now
  wrong. It would mint passkeys under `app.sybil.exchange` that the guest
  rejects, and only fail at the first passkey ceremony. Both now default to
  `sybil.exchange`; `docker-compose.override.yml` (local only, never shipped)
  overrides to `localhost` so local dev still works.

Hostnames, mechanical: web app defaults and dev origins, `.env.example`,
`Dockerfile`, `playwright.config.ts`, e2e specs, `deploy/Caddyfile`,
`docker-compose*.yml`, `scripts/post-deploy-smoke.sh`, `scripts/synthetic-probe.sh`,
`arena/live/*`, `DEPLOY.md`, architecture docs.

`deploy/nginx/sybil.conf` was rewritten rather than renamed: app and api each
need their own certificate, and the ops hosts keep the nip.io one, so it is now
three server blocks instead of one. Note the comment about the `http2` listen
parameter — only the first block carries it, because nginx warns when another
block redefines protocol options for the same socket, and 1.24 has no per-server
`http2 on;` directive.

## Validity migration prepared

Both guests were rebuilt on Linux/amd64 with the pinned OpenVM v2.0.0
toolchain. Both executable commitments moved. Their release records,
fingerprint locks, generated protocol pins, and desired deployment pins are
updated, and `deploy/validity-boundary.json` records the required fresh-genesis
boundary. `deploy/validity-pins.json` remains `pending_redeploy`; repository
pins do not claim that a host or verifier adapter has moved.

## Remaining deployment steps

1. On the host, update `/opt/sybil/.env`:
   ```
   SYBIL_WEBAUTHN_RP_ID=sybil.exchange
   SYBIL_WEBAUTHN_ORIGIN=https://app.sybil.exchange
   SYBIL_CORS_ORIGINS=https://app.sybil.exchange
   SYBIL_API_SITE=http://api.sybil.exchange
   SYBIL_APP_SITE=http://app.sybil.exchange
   ```
   These two site labels are deliberately `http://`: host nginx terminates
   public TLS and forwards to Caddy's loopback HTTP listener. The browser/API
   URLs and WebAuthn origin remain `https://`.
2. Replace the temporary `/etc/nginx/sites-available/sybil-exchange` with the
   repo's `deploy/nginx/sybil.conf`, and delete the temporary file and its
   symlink — otherwise two files declare the same `server_name` and nginx picks
   the first one loaded. `nginx -t` should be warning-free.

   **This intentionally kills `app.62-171-170-238.nip.io`.** The reviewed
   decision is not to retain a redirect. The rewritten config drops that
   hostname from every `server_name`, so it falls through to nginx's default
   server and will present a mismatched certificate. Old tweets, bookmarks, and
   any `og:url` pointing there stop working. Passkeys cannot work on that host
   after the repin regardless: an RP ID must be a domain suffix of the page
   origin, and `sybil.exchange` is not a suffix of a nip.io name.
3. `just deploy-reset-state CONFIRM` — the wipe.
4. Export the build-time values, then `just deploy-all`:
   ```
   export NEXT_PUBLIC_API_BASE=https://api.sybil.exchange
   export NEXT_PUBLIC_WS_BASE=wss://api.sybil.exchange
   export NEXT_PUBLIC_WEBAUTHN_RP_ID=sybil.exchange
   ```
   These are baked into the image; exporting them after the build does nothing.
5. Commit the generated `deploy/releases/<id>.json`.

## What to check after

- `curl https://api.sybil.exchange/v1/health`
- Load `https://app.sybil.exchange` — address bar stays on that host, no redirect.
- Create a passkey, then place an order. The passkey ceremony is the real test:
  it exercises RP ID hash, origin, and `crossOrigin` together. Your password
  manager should save it under `sybil.exchange`, not `app.sybil.exchange`.
- Confirm the WebSocket connects (`wss://api.sybil.exchange`) — nginx forwards
  the Upgrade header, but verify rather than assume.
- `arena.` / `grafana.` / `prover.` nip.io hosts still resolve and still prompt
  for basic auth.

## The landing page is already updated

`sybil.exchange` carries a `devnet` entry point in its header, deliberately
inert: it renders as a faded chip that floats a "soon" hint on hover. Turning it
on is one line in the landing repo's `lib/app-link.ts`:

```ts
export const APP_LIVE = true;
```

Do that only once a passkey ceremony has actually succeeded on the new host —
the chip is the public entry point, so enabling it early sends people into a
broken signup.

## Gates run locally

- Both OpenVM guests were rebuilt, and `just zk-rebuild-check` reproduced the
  committed executable and VM commitments exactly.
- `just check-consensus` — passed with the regenerated validity artifacts.
- `just check-all` — passed on Linux with a shared Cargo target and eight Rust
  test threads.
- `just api-contract-check` — 213 positive and 295 negative Schemathesis cases
  passed.
- `just frontend-check` — 414 tests passed and 1 skipped; the production build
  completed.
- `just arena-check` — 365 tests passed.

Not run here: `just itest-compose` and `pnpm e2e`. The L4 browser journey now
targets `app.sybil.exchange`, so it cannot pass until the cutover is live.
