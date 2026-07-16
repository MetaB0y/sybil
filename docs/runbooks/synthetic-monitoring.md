---
tags: [runbook, monitoring, operations]
status: current
last_verified: 2026-07-15
---

# Synthetic monitoring and alert delivery

> **Executive summary:** every five minutes, a read-only probe checks the public
> API, block advancement, markets, browser CORS, proof lag, and container
> health. It writes results to VictoriaMetrics; vmalert—not the probe—owns
> paging and missing-probe detection.

**Components:** VictoriaMetrics, vmalert, optional Telegram delivery ·
**Script:** `scripts/synthetic-probe.sh`

The five-minute synthetic probe checks the public path from the deployment host
without creating accounts, orders, or any other application state.

## What one run checks

`synthetic-probe.sh` fails fast with one `FAIL: ...` line unless:

- `GET /v1/health` is 2xx, reports `status=ok`, and exposes a positive
  committed height plus a lowercase 64-hex genesis hash;
- `/v1/blocks/latest` advances between samples separated by 1.5 configured
  block intervals;
- `/v1/markets` is a nonempty JSON array;
- an `OPTIONS /v1/onboarding/accounts` preflight from the real app origin
  permits that origin and `POST`;
- when validity is enabled, the proof-status head (`GET /proofs/latest` on the
  prover status API) is within `SYBIL_SMOKE_PROOF_LAG_MAX` blocks (default 30)
  of `/v1/blocks/latest` — see [Proof lag](#proof-lag) below;
- every discovered Compose container is running and is either healthy or has
  no healthcheck, using the exact shared helper also used by
  `post-deploy-smoke.sh`.

Set `SYBIL_SMOKE_DOCKER_SSH=root@host` to perform the same optional container
check over SSH. If Docker is unavailable the HTTP checks still run; on the
deployment host Docker is present and container failures are hard failures.

Manual run:

```bash
cd /opt/sybil
scripts/synthetic-probe.sh \
  --base-url https://172-104-31-54.nip.io \
  --app-origin https://app.172-104-31-54.nip.io \
  --block-interval 10
```

## Alert path

The script writes one gauge sample after every run, plus one proof-lag sample
whenever the lag was computable:

```text
sybil_synthetic_probe_failure{job="sybil-synthetic",instance="<base-url>"} 0|1
sybil_synthetic_proof_lag_blocks{job="sybil-synthetic",instance="<base-url>"} <blocks>
sybil_synthetic_proof_lag_limit_blocks{job="sybil-synthetic",instance="<base-url>"} <blocks>
```

On the host it discovers the compose `victoriametrics` container and posts to
its loopback `/api/v1/import/prometheus` endpoint with the `wget` already used
by that container's healthcheck. This keeps VictoriaMetrics unexposed on a host
port. `SYBIL_SYNTHETIC_VM_URL` can instead name a directly reachable VM URL.

`deploy/vmalert/rules.yml` contains three declarative alerts:

- `SyntheticProbeFailed` fires immediately when the latest result in ten
  minutes is `1`;
- `SyntheticProbeMissing` fires when no result arrives for 15 minutes, covering
  a disabled timer or broken ingestion path;
- `SyntheticProofLagHigh` fires when the probe's measured proof lag stays
  above the emitted `SYBIL_SMOKE_PROOF_LAG_MAX` limit for 10 minutes. In the
  default `fail` mode this is largely redundant with `SyntheticProbeFailed`
  (the probe already fails on high lag); it is the pager for `warn` mode and
  gives a graded lag series either way. The alert and probe therefore keep the
  same ceiling when operators tune it for real-prover cadence.

The existing Telegram overlay loads the same rules and configures vmalert's
notifier as `http://telegram-alerts:8080`. The bridge accepts vmalert's
Alertmanager-v2 `/api/v2/alerts` payload and uses `TELEGRAM_BOT_TOKEN` /
`TELEGRAM_CHAT_ID` from `/opt/sybil/.env`. The probe never calls Telegram and
does not duplicate delivery or deduplication logic.

### L1 indexer alert path

The product VictoriaMetrics config deliberately omits the absent `l1-indexer`
and `validity` targets: static targets create false `up=0` state, while DNS
discovery logs an error for every absent name. GitHub #146 owns an explicit
profile-selected target file/config without Docker-socket access. When the
opt-in `l1-indexer` profile is enabled together with that scrape target, a fatal
integrity error leaves its listener alive, returns 503 from `/healthz`, and
pages through `L1IndexerFatalFailure` on the first nonzero sample. `L1IndexerNotReady`,
`L1IndexerRpcFailureBurst`, and `L1IndexerConfirmedLagHigh` cover sustained
unready state, whole-quorum RPC failures, and authenticated-prefix checkpoint
lag. Provider disagreement, invalid hash binding, and finality regression are
stable fatal kinds; the active trust mode and provider count are exported as
metrics. Their firing and
recovery fixtures live in
`deploy/vmalert/tests/l1-indexer-health_test.yml`; the Compose smoke gate checks
the packaged binary, durable cursor mount, and healthcheck.

The profile is absent in deployments without an L1 vault, so the shared rule
set deliberately does not alert on an absent target. Once the profile is
enabled, configure the host/container supervisor to page if the process or
target disappears. Semantic alert ownership and incident response are in the
[L1 reorg recovery runbook](l1-reorg-recovery.md#indexer-alerts-and-diagnosis).

## Install the five-minute timer

`just deploy-monitoring` and `just deploy-all` copy the probe and shared helper,
install the checked-in units, reload systemd, and idempotently enable/start the
timer. To converge only this unit without recreating monitoring containers, run
`just deploy-install-synthetic-probe`.

The equivalent manual installation is:

```bash
cd /opt/sybil
install -m 0644 deploy/systemd/sybil-synthetic-probe.service \
  /etc/systemd/system/sybil-synthetic-probe.service
install -m 0644 deploy/systemd/sybil-synthetic-probe.timer \
  /etc/systemd/system/sybil-synthetic-probe.timer
systemctl daemon-reload
systemctl enable --now sybil-synthetic-probe.timer
systemctl start sybil-synthetic-probe.service
systemctl status sybil-synthetic-probe.timer
journalctl -u sybil-synthetic-probe.service -n 20 --no-pager
```

Deploy/reload monitoring with the Telegram overlay so the updated rule file is
active:

```bash
just deploy-telegram-alerts
```

Confirm both alerts appear in the vmalert UI and that the timer's successful
sample is queryable in VictoriaMetrics. Use `systemctl list-timers
sybil-synthetic-probe.timer` to confirm the next run.

## Proof lag

Sybil is a validium: blocks are only as good as the proofs that eventually
cover them. `/v1/blocks/latest` advancing while `GET /proofs/latest` (the
`sybil-prover serve` status API, `crates/sybil-prover/src/serve.rs`) stands
still means the proof pipeline is wedged — exactly the failure class of the
2026-07-10 openvm proving-key `bitcode-error` incident, which was invisible to
every existing check because blocks, markets, and containers all stayed green.

How the probe reads the proof-status head, in order:

1. `SYBIL_SMOKE_PROVER_BASE` / `--prover-base` when set — a direct URL,
   normally the Caddy ops vhost with basic-auth credentials embedded
   (`https://user:pass@prover.<host>`). This exercises the full external path.
2. Otherwise `docker exec` into the compose `sybil-prover` container (local
   docker, or over `SYBIL_SMOKE_DOCKER_SSH`), reading
   `http://127.0.0.1:3002/proofs/latest`. This compatibility projection is what the on-box timer uses:
   no secrets needed, same privilege the probe already uses to push metrics.
3. If docker is unavailable and no URL is set, the check SKIPs with one loud
   line — an off-box run without credentials cannot see the prover, and must
   not false-alarm.

Modes (`SYBIL_SMOKE_PROOF_LAG` / `--proof-lag`):

- `fail` (script default): lag above the threshold, a missing prover container, or an
  unusable `/proofs/latest` fails the probe and pages via
  `SyntheticProbeFailed`. Use this for an explicit validity deployment: the
  durable `sybil-prover` daemon pulls the transactional proof-job outbox,
  assembles epochs, and advances the proof-status head after native epoch
  verification.
- `warn`: violations print one `WARN:` line and the probe stays green; the
  `sybil_synthetic_proof_lag_blocks` sample is still pushed, so
  `SyntheticProofLagHigh` still pages if the lag persists. Flip to this mode
  (systemd drop-in setting `Environment=SYBIL_SMOKE_PROOF_LAG=warn`, or edit
  the unit's `ExecStart` flags) while bringing up the daemon in real STARK mode
  (`just prover-daemon-stark ...`), whose proving latency profile is not yet
  established; return to `fail` — with `SYBIL_SMOKE_PROOF_LAG_MAX` raised to
  match observed real-prover cadence — once it holds a steady lag.
- `off`: skip entirely (deployments with no prover at all). The checked-in
  systemd unit selects this for the 2 GB product devnet. Override it to `fail`
  in a systemd drop-in only when the validity profile is enabled.

The 30-block default threshold is one probe period (5 min) at the 10s block
cadence: ~30x the mock prover's normal lag in bounded integration runs, while a
wedged enabled pipeline crosses it within a single probe cycle.

First response for `SyntheticProofLagHigh` or a proof-lag probe failure:

1. Read the journal line: `journalctl -u sybil-synthetic-probe.service -n 50
   --no-pager`. It names the proof height, chain height, and lag.
2. Check the pipeline container with `docker compose ps sybil-prover`, then
   `docker compose logs --tail 100 sybil-prover`.
   Cross-check the internal alerts (`ProverLagHigh`, `ProverArtifactStale`,
   `ProverProofFailed`) in vmalert/Grafana — they read the same artifact store
   from inside and localize the wedge.
3. Real-prover wedges of the pk `bitcode-error` class (openvm proving-key /
   guest-build mismatch after an image or toolchain change): check the worker
   log for `bitcode` / proving-key errors, verify the guest fingerprint with
   `scripts/zk-guest-fingerprint.sh` against the canonical pin, and rebuild
   the proving key if they disagree.
4. `/proofs/latest` 404 ("no proven epoch") on a mature persistent chain means
   the daemon has not completed an epoch or its redb/artifact volume was
   swapped. Inspect `/v1/status`, source-failure metrics, and the `prover-data`
   plus `prover-artifacts` mounts.
5. A restart of the wedged container is safe: leases expire, exact source jobs
   are idempotent, and published orphan artifacts are reconciled. After remediation run the probe once and finish
   with `just deploy-verify` to confirm the full post-deploy gate is green.

## What a failed probe means

The one-line journal reason identifies the first broken contract:

- health failure: public routing, Caddy, API process, or recovery is broken;
- height not advancing: the API can answer reads but sequencing/persistence is
  stalled;
- markets empty: state was reset, restore failed semantically, or mirror/native
  market initialization is broken;
- CORS failure: the API is healthy but browsers cannot onboard/trade from the
  deployed app origin;
- proof-lag failure: blocks advance but the proof pipeline does not — the
  mock prover / prover worker is wedged, dead, or its artifact store is gone
  (see [Proof lag](#proof-lag));
- container failure: a compose dependency is stopped, restarting, or unhealthy;
- result-delivery failure: the user path passed, but VictoriaMetrics or the
  local Docker ingestion path is broken.

## First response

1. Read the exact cause:
   `journalctl -u sybil-synthetic-probe.service -n 50 --no-pager`.
2. Re-run the script manually. Compare direct public API requests with
   `docker compose ... exec sybil-api curl http://127.0.0.1:3000/v1/health` to
   separate Caddy/network faults from API faults.
3. Inspect `docker compose ps` and the affected service logs. For a stalled
   height, check `sybil_persistence_failures`, store/qMDB alerts, disk space,
   memory pressure, and actor mailbox depth before restarting.
4. For CORS, compare `SYBIL_CORS_ORIGINS` and the timer's `--app-origin`; do not
   make production CORS permissive as a workaround.
5. For missing/delivery alerts, check the timer, `victoriametrics` health,
   vmalert rule status, `telegram-alerts` logs, and that both Telegram secrets
   remain set.
6. After remediation, run the probe once. A `0` sample resolves
   `SyntheticProbeFailed`; verify the resolved Telegram notification arrives.

Do not use the state-reset recipe as a first response. If persistence recovery
is implicated, take/preserve a backup and follow the store restore runbook.
