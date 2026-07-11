---
status: current
---

# Passkey `keyop-state` failure: root cause and redeploy runbook

Date: 2026-07-11  
Target revision: `2fb99e9b` (`origin/main` at investigation start)  
Public API: `https://172-104-31-54.nip.io`  
Public web app: `https://app.172-104-31-54.nip.io`

## Verdict

The production API image does not mount
`GET /v1/accounts/{id}/keyop-state`. The empty 404 is an Axum route miss, not
the handler's account-not-found response. Current `main` mounts the route and
its Raw P256 and WebAuthn key registration/revocation flows pass their focused
integration tests. No runtime code fix was needed or applied; this document is
the only working-copy change.

There is one important qualification to the reported “deployed frontend versus
deployed API” version-skew description. A fresh fetch of the live web app on
2026-07-11 returned a pre-Stage-1b settings bundle: it contains
`/keys/register`, but contains neither `keyop-state` nor the reported
`failed to load key-operation signing state` string. Therefore the two images
currently served are a coherent old generation. The reported error must have
come from a newer cached/open-tab or locally served frontend using the stale
production API. The backend failure is still unambiguously a stale deploy, and
both images must be replaced together.

A fresh genesis **is required** before current `main` can be treated as the
same valid deployment. The deployed generation predates witness v8 and v9 and
Commonware 2026.5; there is no reviewed in-place validity migration.

## Production HTTP evidence

Read-only probes on 2026-07-11 produced:

| Request | Result | Meaning |
|---|---|---|
| `GET /v1/health` | `200 application/json`, `{"status":"ok","height":6553,"genesis_hash":"ecf25142..."}` | Caddy and the API are live |
| `GET /v1/markets` | `200 application/json`, 266,077-byte body | An established public API route works |
| `GET /v1/accounts/1` without a bearer | `401 application/json`, `{"error":"Missing bearer token","code":"UNAUTHORIZED"}` | A real account route reaches API middleware |
| `GET /v1/accounts/{1,2,3}/keyop-state` | `404`, zero bytes, no content type | Route is not mounted |
| `GET /v1/accounts/1/definitely-missing-route` | `404`, zero bytes, no content type | Exact same shape as the keyop failure |
| `GET /openapi.json` | `200`; zero `keyop-state` occurrences | Running schema confirms route absence |

The production OpenAPI still describes the older `SignedRegisterKeyRequest`:
it requires `nonce` and has no `bound_keys_digest_hex` or
`bound_events_digest_hex`. Current Stage 1b instead requires the two state
digests. Commit `c2b333cd0d24` added witness v9, the API route, the state-bound
request fields, and the frontend `getKeyOpBinding()` call together. Thus the
running API is conclusively pre-Stage-1b; this is not an account id, auth,
reverse-proxy, or handler bug.

The topology is also worth making explicit. [`deploy/Caddyfile`](../deploy/Caddyfile)
routes the root nip.io hostname exclusively to `sybil-api`; the web UI is on
the `app.` hostname. The root returning an empty 404 for `/` is expected and is
not a web outage.

## Current-main key-operation flow

### First key

Current `main` has two onboarding forms:

1. The preferred browser path atomically sends `POST /v1/accounts` with
   `initial_key`. It is public and demo-balance capped.
2. The legacy/operator path creates a bare account with the service token and
   then sends service-token-gated `POST /v1/accounts/{id}/keys`. This unsigned
   endpoint accepts only the first key. A second unsigned registration returns
   409 and directs the caller to the signed endpoint.

A production-like native API exercise verified the second form:

- bare account create without service token: 401;
- bare account create with service token: 200;
- first-key POST without service token: 401;
- first-key POST with service token: 200;
- second unsigned key with service token: 409;
- public `GET keyop-state` for that account: 200 with nonempty
  `keys_digest_hex` and the current `events_digest_hex`;
- nonexistent-account `GET keyop-state`: handler-level 404 JSON
  `{"error":"Account 999999 not found","code":"NOT_FOUND"}`.

### Additional key registration

The browser flow in
[`frontend/web/src/lib/account/settings.ts`](../frontend/web/src/lib/account/settings.ts)
is:

1. fetch `genesis_hash` and public `GET /v1/accounts/{id}/keyop-state`;
2. construct `canonicalKeyRegistrationBytes(account_id, new KeyRecord,
   genesis_hash, keys_digest, events_digest)`;
3. authorize those bytes with the active session key, either as a raw P256
   signature or a WebAuthn assertion whose challenge is the SHA-256 of those
   bytes;
4. submit the new key, signer identity, authorization envelope, and both bound
   digests to `POST /v1/accounts/{id}/keys/register`.

The API returns the digests directly from the sequencer account's
`account.keys_digest` and `account.events_digest`. Admission's
`validate_keyop_state_binding()` compares the submitted values to those same
two fields before writing the control-plane WAL. Either mismatch becomes
`SequencerError::KeyOpStateStale`, mapped by the API to HTTP 409. The canonical
Rust bytes are owned by
[`crates/sybil-verifier/src/account_keys.rs`](../crates/sybil-verifier/src/account_keys.rs):
the domain, genesis hash, little-endian account id, full key record, key digest,
and event digest are all signed.

The focused `account_management` integration suite passed signed Raw P256
registration, stale-binding replay rejection, wrong/unknown signer rejection,
WebAuthn registration, WebAuthn-signed agent-key addition, WebAuthn-signed
revocation, a subsequently valid sealed block, and last-key lockout.

### Revocation

Revocation is present at `POST /v1/accounts/{id}/keys/revoke`. The frontend
fetches a fresh binding, signs `canonicalKeyRevocationBytes(...)`, and submits
the same two bound digests. Admission uses the same stale-state check. It also
checks the target's complete key record and refuses to revoke the last active
key with 409.

## Why a fresh genesis is required

This is a validity reset, not a routine database schema upgrade:

- [`crates/sybil-verifier/src/witness_schema.rs`](../crates/sybil-verifier/src/witness_schema.rs)
  pins `WITNESS_FORMAT_VERSION = 9` and rejects any other canonical witness
  version. The deployed image was created before witness-v8 commit
  `e1f126fb9673` and witness-v9 commit `c2b333cd0d24`.
- Current matching/sequencer and verifier manifests pin Commonware `2026.5.0`;
  the pre-migration revision used `2026.4.0`. The migration changed qMDB/MMR
  root and proof semantics and regenerated the golden state/event roots.
- Persistent redb uses `STORE_LAYOUT_VERSION = 1` in
  [`crates/matching-sequencer/src/store/tables.rs`](../crates/matching-sequencer/src/store/tables.rs).
  `initialize_or_validate_layout()` checks only that number. It has not been
  bumped and there is no v7/v8/v9 or Commonware state migration behind it.
- `Store::load_state()` does not decode the latest stored witness merely to
  check its canonical version. It notices that a latest witness row exists,
  opens the fenced qMDB, and calls `ensure_state_qmdb_root()`. If the stored
  root differs, it rebuilds the fenced typed-state tree from redb using the
  **current** leaf schema and Commonware implementation. It then requires the
  rebuilt root to equal the old committed block header. A remaining mismatch
  returns `StoreError::CorruptLayout`; `sybil-api` logs restore failure and
  exits. Opening an old qMDB journal may fail even earlier if its storage
  encoding is incompatible.
- Even if an accidental dataset happened to boot, continuing an old accepted
  root chain under new canonical witness bytes and new guest commitments would
  be protocol-invalid. [`docs/architecture/04-verification/Block Witness.md`](../docs/architecture/04-verification/Block%20Witness.md)
  explicitly says not to attempt an in-place migration, and
  [`docs/runbooks/fresh-genesis-redeploy.md`](../docs/runbooks/fresh-genesis-redeploy.md)
  requires a fresh genesis for this case.

Answer: **yes, run `just deploy-reset-state CONFIRM`; preserving the current
`sybil-data` volume is not supported.** The command removes sequencer
accounts/markets/orders/history, Polymarket mappings, arena decisions, proof
jobs/artifacts, and VictoriaMetrics history. The recipe's exact volume list is
`sybil-data`, `polymarket-data`, `arena-data`, `prover-jobs`,
`prover-artifacts`, `sybil_prover-jobs`, `sybil_prover-artifacts`, and
`vmdata`. Caddy certificate/config volumes are not removed.

Take a backup first as incident evidence. This conclusion is appropriate only
because this is the private single-sequencer devnet/validium reset window; it
must not be generalized to discarding real user funds.

## WebAuthn configuration for this host

The actual browser app origin is `https://app.172-104-31-54.nip.io`; the root
hostname is the API and is not the relying party. Required host-side
`/opt/sybil/.env` values are:

```dotenv
SYBIL_WEBAUTHN_RP_ID=app.172-104-31-54.nip.io
SYBIL_WEBAUTHN_ORIGIN=https://app.172-104-31-54.nip.io
SYBIL_CORS_ORIGINS=https://app.172-104-31-54.nip.io
```

The matching frontend build values are:

```bash
NEXT_PUBLIC_API_BASE=https://172-104-31-54.nip.io
NEXT_PUBLIC_WS_BASE=wss://172-104-31-54.nip.io
NEXT_PUBLIC_WEBAUTHN_RP_ID=app.172-104-31-54.nip.io
```

The API defaults (`localhost`, `http://localhost:3000`) would reject production
registrations/assertions by RP-ID hash and origin. The frontend RP-ID default is
empty, which lets the browser derive the current page host and is compatible
with this topology, but explicitly baking the exact app host keeps creation and
discoverable assertion behavior aligned with the server and avoids an
environment-dependent image. Production Compose requires the two server
values to be nonempty. The live CORS preflight was green for the `app.` origin,
but the server's private `.env` values could not be read under this lane's
no-SSH constraint.

## Ordered redeploy runbook

Do not rely on `just deploy-all`: it currently builds but does not transfer the
new `sybil-web` image. For an ordinary compatible release the correct pair is
`just deploy-api` followed by `just deploy-web`. This release needs a more
careful first step because `deploy-reset-state` restarts whatever images are
already on the host: stage the target API and web images **before** removing the
volumes, so the old binary cannot create the new genesis.

### 1. Freeze, configure, and back up

From this exact clean checkout, record `jj log -r @ -n 1`, the two checked-in
guest commitment JSON files, and a store backup/restore-drill result as required
by the fresh-genesis runbook. Update `/opt/sybil/.env` to the three server-side
values above and retain the existing secrets.

Export the build-time frontend values locally:

```bash
export NEXT_PUBLIC_API_BASE=https://172-104-31-54.nip.io
export NEXT_PUBLIC_WS_BASE=wss://172-104-31-54.nip.io
export NEXT_PUBLIC_WEBAUTHN_RP_ID=app.172-104-31-54.nip.io
```

### 2. Stage target images without starting them

Build one image at a time to stay within the host's memory limit:

```bash
just deploy-sync
just deploy-prod-env-check

DOCKER_BUILDKIT=1 COMPOSE_DOCKER_CLI_BUILD=1 \
  DOCKER_DEFAULT_PLATFORM=linux/amd64 docker-compose build sybil-api
docker save sybil-api:latest | ssh root@172.104.31.54 docker load

DOCKER_BUILDKIT=1 COMPOSE_DOCKER_CLI_BUILD=1 \
  DOCKER_DEFAULT_PLATFORM=linux/amd64 docker-compose build sybil-web
docker save sybil-web:latest | ssh root@172.104.31.54 docker load
```

These are the build/load halves of `deploy-api` and `deploy-web`, intentionally
without their `compose up` and final smoke steps. Invoking `just deploy-api`
against the old volume first would start the v9 binary on incompatible state
and make its automatic `deploy-verify` fail before the reset.

### 3. Reset once, onto the staged images

```bash
just deploy-reset-state CONFIRM
```

This is destructive and wipes the volumes listed above. It must run only after
both staged image loads succeeded. If any required target image was not staged,
stop rather than let an old image establish the new genesis.

If arena source also changed in the chosen revision, run `just deploy-arena`
after the reset. It is not required to fix the key-operation route.

### 4. Verify the deployed stack

```bash
just deploy-verify
just deploy-verify-restart
```

`deploy-verify` hard-checks health, CORS, atomic first-key onboarding, service
route policy, markets, and deterministic fills. `deploy-verify-restart` causes
about 20 seconds of API downtime and verifies the fresh persisted state can be
reopened.

Then prove the exact missing route against a newly created account. This curl
uses a raw P256 key because WebAuthn attestation must be created by a browser;
it validates the same atomic account-plus-first-key HTTP path used by the
post-deploy onboarding gate, followed by `keyop-state`:

```bash
API=https://172-104-31-54.nip.io
KEY_FILE=$(mktemp)
openssl ecparam -name prime256v1 -genkey -noout -out "$KEY_FILE"
PUBKEY_HEX=$(openssl ec -in "$KEY_FILE" -pubout -conv_form compressed \
  -outform DER 2>/dev/null | tail -c 33 | xxd -p -c 256)
rm -f "$KEY_FILE"

ONBOARD=$(curl -fsS -X POST "$API/v1/accounts" \
  -H 'content-type: application/json' \
  --data "{\"initial_balance_nanos\":1000000000000,\"initial_key\":{\"public_key_hex\":\"$PUBKEY_HEX\"}}")
ACCOUNT_ID=$(printf '%s' "$ONBOARD" | \
  python3 -c 'import json,sys; print(json.load(sys.stdin)["account_id"])')
printf '%s\n' "$ONBOARD"
curl -fsS -D - "$API/v1/accounts/$ACCOUNT_ID/keyop-state"
```

Expected: onboarding returns 200 JSON, and the final request returns
`HTTP/2 200`, `application/json`, the same `account_id`, and 64-hex-character
`keys_digest_hex` and `events_digest_hex` fields. A zero-byte 404 is a failed
deployment.

Finally, complete one real browser passkey journey on
`https://app.172-104-31-54.nip.io`: create a passkey, add an agent key, revoke
that agent key, log out, and use discoverable passkey sign-in. Curl cannot prove
authenticator RP/origin/challenge behavior. Record the new genesis hash, first
accepted root, deployed revision, guest/adapter pins, and all smoke results in
the private deployment log before reopening order intake.

## Deployment result

The fresh-genesis redeploy completed on 2026-07-11 from revision `2fb99e9b`.
Both images were built locally and transferred with
`docker save | ssh docker load`; no image was built on the 2 GB deployment
host. The old chain was backed up and restored in isolation before reset:

- backup: `/opt/sybil/backups/sybil-store-20260711T153650Z-1744439`;
- old height: `6750`;
- old state root:
  `96d3cdf96719b62d4cd7d2afd486cdf8dc62fc8a7d9263490b169ce31351557f`;
- new genesis hash:
  `ac3ccbe821bdbfb706c9d13c791e55957c3b42334eb04873e01b636ef0962acf`.

`just deploy-verify` passed 47/47 live checks with no skips, including CORS,
atomic first-key onboarding, signed order/cancel, reservation release, route
policy, native and mirrored markets, and a deterministic fill on an advancing
chain. `just deploy-verify-restart` then returned the API to healthy in 19
seconds with a stable restart count and no OOM kill. A separate live probe of
account 20 returned `GET /v1/accounts/20/keyop-state` as 200 with valid 64-hex
key and event digests; the deployed OpenAPI contains the route.

The remaining manual check is a real browser WebAuthn ceremony on the `app.`
host. It is intentionally not represented as complete by the API and curl
checks above.

## Gates run in this investigation

| Gate | Result |
|---|---|
| Production health/markets/account/unknown-route/keyop/OpenAPI probes | PASS — route absence confirmed |
| Live `app.` frontend HTML/settings bundle inspection | PASS — live bundle identified as pre-Stage-1b |
| Native production-like API + service-token/first-key/keyop curl sequence | PASS |
| `cargo test -p sybil-api --test account_management -- --test-threads=1` | PASS — 10/10 |
| `cargo test -p sybil-api --test route_policy -- --test-threads=1` | PASS — 9/9 |
| Frontend Vitest suite (including canonical key-op and WebAuthn tests) | PASS — 210 passed, 1 skipped |
| `just golden-check` | PASS |
| `just docs-check` (with uv cache/tool dirs redirected to writable temp) | PASS |
| Local production Docker builds (`sybil-api`, `sybil-web`) | PASS — staged and deployed without building on the host |
| Live post-deploy smoke gate | PASS — 47/47, no skips |
| Restart-resilience gate | PASS — healthy in 19s, no OOM/restart loop |
| Live `keyop-state` probe | PASS — 200 with valid 64-hex digests |
| Live WebAuthn browser ceremony after redeploy | PENDING — requires a real browser authenticator |
