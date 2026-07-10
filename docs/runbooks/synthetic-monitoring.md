# Runbook: synthetic monitoring and alert delivery

**Owning tickets:** SYB-250, SYB-223 item 3 · **Components:** operations,
VictoriaMetrics, vmalert, Telegram · **Script:** `scripts/synthetic-probe.sh`

The five-minute synthetic probe checks the public path from the deployment host
without creating accounts, orders, or any other application state.

## What one run checks

`synthetic-probe.sh` fails fast with one `FAIL: ...` line unless:

- `GET /v1/health` is 2xx and reports `status=ok`;
- `/v1/blocks/latest` advances between samples separated by 1.5 configured
  block intervals;
- `/v1/markets` is a nonempty JSON array;
- an `OPTIONS /v1/accounts` preflight from the real app origin permits that
  origin and `POST`;
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

The script writes one gauge sample after every run:

```text
sybil_synthetic_probe_failure{job="sybil-synthetic",instance="<base-url>"} 0|1
```

On the host it discovers the compose `victoriametrics` container and posts to
its loopback `/api/v1/import/prometheus` endpoint with the `wget` already used
by that container's healthcheck. This keeps VictoriaMetrics unexposed on a host
port. `SYBIL_SYNTHETIC_VM_URL` can instead name a directly reachable VM URL.

`deploy/vmalert/rules.yml` contains two declarative alerts:

- `SyntheticProbeFailed` fires immediately when the latest result in ten
  minutes is `1`;
- `SyntheticProbeMissing` fires when no result arrives for 15 minutes, covering
  a disabled timer or broken ingestion path.

The existing Telegram overlay loads the same rules and configures vmalert's
notifier as `http://telegram-alerts:8080`. The bridge accepts vmalert's
Alertmanager-v2 `/api/v2/alerts` payload and uses `TELEGRAM_BOT_TOKEN` /
`TELEGRAM_CHAT_ID` from `/opt/sybil/.env`. The probe never calls Telegram and
does not duplicate delivery or deduplication logic.

## Install the five-minute timer

`just deploy-sync` copies the probe and shared helper into `/opt/sybil/scripts`
and copies the units under `/opt/sybil/deploy/systemd`. Then the operator runs:

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

## What a failed probe means

The one-line journal reason identifies the first broken contract:

- health failure: public routing, Caddy, API process, or recovery is broken;
- height not advancing: the API can answer reads but sequencing/persistence is
  stalled;
- markets empty: state was reset, restore failed semantically, or mirror/native
  market initialization is broken;
- CORS failure: the API is healthy but browsers cannot onboard/trade from the
  deployed app origin;
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
