# Runbook: Devnet redeploy (May-2026 build → current `main`)

**Owning tickets:** SYB-133 (profiles/preflight), SYB-181/185/198 (consensus
changes), SYB-190 (L1 indexer), SYB-199/205 (parked hotfix chain) ·
**Components:** `contracts/`, `sybil-api`, `matching-sequencer`, `sybil-l1-indexer`, ops

---

## Why this redeploy is not a routine `just deploy-api`

The deployed devnet still runs the **May-2026 build** parked on the jj bookmark
`local-devnet-hotfixes`. The next redeploy jumps it to current `main` — a
**consensus-breaking** jump. Two independent things are stale and must both be
handled, in order:

1. **The on-chain guest-commitment pin is stale.** The deployed
   `OpenVmVerifierAdapter` constructor pins the May-2026 exe commitment
   (`0x00796a20…`). Current `main` rebuilds to a different artifact, so freshly
   built proofs will **not** verify against the old pin. See
   `zk/openvm-guest/README.md` ("Rebuild status and the 2026-07-03 divergence").
2. **The committed state roots changed.** Consensus-surface changes landed on
   `main` since the deployed build — integer `ceil_mul_ratio` fill scaling
   (SEQ-2, `crates/matching-sequencer/src/order_book.rs`), the
   `BlockWitness.l1_deposits` witness schema and deposit-inclusion checks, the
   bridge sidecar deposit/withdrawal checks, replay-nonce fields in account
   state (`last_nonce` in `crates/matching-sequencer/src/account.rs`), and
   fixed-point share units (`SHARE_SCALE` in
   `crates/matching-engine/src/types.rs`). The genesis/state root the old build
   committed is **not** reproducible under current `main`.

**Devnet policy is fresh-genesis redeploy — there is no migration path.** No
state-migration tooling exists in the repo; state is reset by removing the
`sybil-data` volume and letting `sybil-api` re-genesis. `docs/deployment.md`
("Persistence") and `DEPLOY.md` ("Reset app state") are the authoritative
statement of this policy. Do **not** attempt to carry old state forward.

Work the steps below top to bottom: **contracts → state reset → services →
verify → cleanup.**

---

## Preflight (before touching the host)

Run these from a clean checkout of the `main` revision you are deploying.

1. **Read the current guest commitments — never hardcode them.** They changed
   three times on 2026-07-03 alone. Always read the committed source of truth at
   deploy time:

   ```bash
   cat zk/openvm-guest/openvm/release/sybil-openvm-guest.commit.json
   ```

   Example only (yours will differ — read the file):
   `app_exe_commit 0x008471a9…`, `app_vm_commit 0x000c1059…`. The authority order
   is **deployed pin > `commit.json` > lock file** (`zk/openvm-guest/README.md`).

2. **Confirm the source ↔ commitment gate is green on this revision:**

   ```bash
   scripts/zk-guest-fingerprint.sh --check
   ```

   This proves the guest source and its full path-dep closure
   (`crates/sybil-zk`, `crates/sybil-verifier`, `crates/matching-engine`,
   `crates/sybil-l1-protocol`) still match the lock, and that the lock's
   commitment hashes equal `commit.json`. If it fails, **stop** — the tree is
   mid-rebuild and the pin you would deploy is not trustworthy.

3. **Confirm the deployed revision** you are moving off:
   `just deploy-shell` then `cd /opt/sybil && git rev-parse HEAD` (or inspect the
   running image label). Record it so a rollback target exists.

---

## Step 1 — Contracts: repin the verifier adapter

The pin lives in the `OpenVmVerifierAdapter` **constructor** and is immutable
(`contracts/src/OpenVmVerifierAdapter.sol`, args `appExeCommit` / `appVmCommit`).
Repinning therefore means **deploying a new adapter** — you cannot mutate an
existing one.

- **`SybilSettlement` / `SybilVault` do not need new logic**, but each holds a
  verifier reference. `SybilSettlement.setVerifier(newAdapter)`
  (`VERIFIER_ADMIN_ROLE`, bumps `verifierVersion`) repoints settlement without
  redeploy. `SybilVault` pins its verifier in its constructor, so a full
  fresh-genesis (below) is the clean path on devnet — redeploy MockUSDC, the
  adapter, `SybilSettlement`, and `SybilVault` together against a fresh anvil so
  the accepted-root chain also starts empty (heights are monotonic and roots are
  chained; old accepted roots are incompatible with the new state schema).
- **Deploy the adapter with the values you just read from `commit.json`** as the
  constructor `appExeCommit` / `appVmCommit`. Do not paste a hash from this
  runbook or from an old deploy log.

> Gap to be aware of: the only checked-in Foundry script is
> `contracts/script/UnsafeAnvilSmoke.s.sol`, which wires
> `UnsafeAcceptAllVerifierAdapter` (no commitment pin) for Anvil bridge
> plumbing — see `just contracts-anvil-unsafe-smoke`. There is **no** committed
> deploy script that constructs the real `OpenVmVerifierAdapter` from
> `commit.json`; that step is currently manual (`forge create` with the two
> commitment args), and the production `OpenVmHalo2Verifier` bytecode/address is
> still an open item (`docs/architecture/L1 Settlement and Vault.md`, "Open
> questions"). The devnet also runs the **mock prover** (`sybil-prover-mock`),
> so real on-chain verification is not exercised end-to-end today — the pin
> update is still mandatory so the deployed adapter matches the source it claims.

Submitting a root once a real proof exists uses the settlement address:

```bash
just openvm-commit      # rebuild + print app_exe_commit / app_vm_commit (must match commit.json)
# host submitter: cargo run -p sybil-prover -- submit-state-root --settlement <addr> ...
```

---

## Step 2 — State reset (fresh genesis)

Fresh genesis is intentional and destructive. On the deploy host:

```bash
just deploy-reset-state CONFIRM
```

This stops the stack and removes `sybil-data`, `polymarket-data`, `arena-data`,
`prover-jobs`, `prover-artifacts`, and `vmdata`, then brings services back up on
the new images (see the `deploy-reset-state` recipe in the `justfile`).

- `SYBIL_EVENT_SNAPSHOT_DIR=/data/event_snapshots` persists **under** the
  `sybil-data` volume, so it is wiped by this reset — that is correct for a
  fresh genesis. The "do not wipe `/data`" guidance in
  `docs/architecture/Deployment Profiles.md` and `docs/deployment.md` applies to
  **routine restarts only**, not to an intentional consensus redeploy.
- Never use `deploy-reset-state` as a restart step. It is only for a deliberate
  fresh devnet.

---

## Step 3 — Services

Build/transfer the images and start the stack. Compose file selection is the
dev/prod distinction — do not mix them up:

| Posture | Compose invocation |
| --- | --- |
| Local dev | `docker-compose.yml` (auto-loads `docker-compose.override.yml`) |
| Prod / public devnet | `docker-compose.yml` + `docker-compose.prod.yml` |
| Prod + Telegram alerts | add `docker-compose.telegram.yml` |

The `just deploy-*` recipes always apply the prod overlay (and add the Telegram
overlay when `TELEGRAM_BOT_TOKEN` / `TELEGRAM_CHAT_ID` are present in
`/opt/sybil/.env`):

```bash
just deploy-all         # build + load + up (prod, +telegram if configured)
# or piecewise: just deploy-api / deploy-arena / deploy-monitoring / deploy-caddy
```

**Config surface — verify against the authoritative files, do not trust this
list as complete:**

- **Prod preflight (SYB-133) fail-closes the boot** if a dev-only knob is wired
  in. Required prod values: `SYBIL_DEPLOYMENT_PROFILE=prod`,
  `SYBIL_DEV_MODE=false`, `SYBIL_SERVICE_TOKEN` set, `SYBIL_DATA_DIR` set. Full
  classification and the `SYBIL_ALLOW_DEV_KNOBS=1` escape hatch:
  `crates/sybil-api/src/preflight.rs` and
  `docs/architecture/Deployment Profiles.md`. (The dev/prod trust boundary is
  the SYB-173 work; it is enforced via `SYBIL_DEV_MODE` + the profile preflight,
  not a single `SYBIL_ENV` var.)
- **Prod secrets** in `/opt/sybil/.env`: `SYBIL_SERVICE_TOKEN`,
  `GF_SECURITY_ADMIN_PASSWORD`, `CADDY_OPS_AUTH_USER`, `CADDY_OPS_AUTH_HASH`,
  optional `SYBIL_CORS_ORIGINS`; `/opt/sybil/arena.env` carries
  `OPENROUTER_API_KEY`. Authoritative list: `DEPLOY.md` ("Required Prod
  Secrets").
- **Arena metrics (SYB-211):** `--metrics-port` (default `0` = exporter **off**)
  and `--metrics-host` in `arena/live/runner.py`. Off by default; only set a
  port if you want the arena exporter scraped.
- **L1 indexer (SYB-190):** `sybil-l1-indexer` is run separately (not a compose
  service). For a real chain raise `SYBIL_L1_CONFIRMATIONS` to 12–32 (dev-Anvil
  default is `2`; crediting a reorged-away deposit is unrecoverable), and set
  `--cursor-path` / `SYBIL_L1_CURSOR_PATH` so restarts resume the scan instead
  of rescanning from `start_block`. Other vars (`SYBIL_L1_RPC_URL`,
  `SYBIL_L1_VAULT`, `SYBIL_L1_CHAIN_ID`, `SYBIL_L1_START_BLOCK`) are in
  `crates/sybil-l1-indexer/src/main.rs`.
- **Telegram overlay:** `TELEGRAM_BOT_TOKEN`, `TELEGRAM_CHAT_ID` in
  `/opt/sybil/.env`; wiring in `docker-compose.telegram.yml`.

---

## Step 4 — Verify

Run in order; each is a gate, not a suggestion.

1. **Guest fingerprint matches the deployed revision** (re-run on the host's
   checked-out rev, not just locally):

   ```bash
   scripts/zk-guest-fingerprint.sh --check
   ```

2. **API liveness and a fresh, advancing chain:**

   ```bash
   ssh root@172.104.31.54 'curl -sS http://localhost:3000/v1/health'
   ssh root@172.104.31.54 'curl -sS http://localhost:3000/v1/blocks/latest'   # height should be small (fresh genesis) and rising
   ssh root@172.104.31.54 'curl -sS http://localhost:3000/metrics | grep -E "^sybil_block_height"'
   ```

3. **Alerting loaded:** vmalert UI `http://172.104.31.54:8880` shows the rules
   from `deploy/vmalert/rules.yml` + `deploy/vmalert/block-production.yml`
   (see `docker-compose.yml` `vmalert` service). Sanity-check the rule logic:

   ```bash
   promtool test rules deploy/vmalert/tests/block-production_test.yml
   ```

4. **One end-to-end deposit through the L1 indexer** on anvil, exercising
   confirmation depth: submit a `deposit()` to the fresh `SybilVault`, mine past
   `SYBIL_L1_CONFIRMATIONS` blocks, and confirm the indexer credits it
   (`sybil-l1-indexer` logs; `GET /v1/bridge/status` shows the advanced
   `deposit_cursor`/`deposit_root`).

5. **One signed order with a nonce** to confirm replay protection is live on the
   new account-state schema: submit a P256-signed order via `/v1/orders`, then
   re-submit the same nonce and confirm it is rejected as a stale replay nonce
   (`crates/matching-sequencer/src/error.rs`).

---

## Step 5 — Post-deploy cleanup

Once the redeploy is verified healthy, the parked hotfix chain is dead:

- The jj bookmark `local-devnet-hotfixes` (currently the May-2026 build) becomes
  **deletable** — its fixes are superseded by `main`. Delete it per the SYB-199 /
  SYB-205 disposition:

  ```bash
  jj bookmark delete local-devnet-hotfixes
  ```

- Record the newly deployed revision, the four contract addresses (adapter,
  settlement, vault, MockUSDC), and the `app_exe_commit` / `app_vm_commit` you
  pinned, so the next redeploy has an unambiguous "moving off" baseline.
