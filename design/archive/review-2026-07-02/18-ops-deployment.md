# Deployment, Observability, CI

**Files:** `docker-compose*.yml`, `deploy/`, `justfile`, `scripts/`, `Dockerfile`, `.github/workflows/`, `DEPLOY.md`

## Verdict

A hand-rolled single-Linode deployment driven from a laptop, with a genuinely thoughtful VictoriaMetrics/vmalert/Grafana/Telegram observability stack layered on top. The **alert rules are unusually high quality**. The **posture around them is not**: the monitoring stack lives on the machine it monitors with no external heartbeat, every internal port is on the public internet, dev-mode is on in prod, the OpenRouter key is passed via argv (and committed to git), and the documented runbook describes a deployment stack that no longer exists.

## Architecture as built

One Linode g6-standard-1 (1 vCPU, 1.9 GB RAM + 495 MB swap), IP hardcoded in the justfile and `status.sh`. No IaC; server state is `/opt/sybil/`. Deploy: `just deploy-*` scps compose files + `deploy/`, builds images **locally**, `docker save | ssh docker load`, and restarts. The Dockerfile is a two-stage build producing three binaries with `CARGO_BUILD_JOBS=1` + `codegen-units=1` hardcoded to fit the 2 GB host.

**Compose topology:** base `docker-compose.yml` defines 10 services (api, polymarket, prover serve/worker/mock-live, arena, arena-dashboard, VictoriaMetrics, vmalert, Grafana, node-exporter); `prod.yml` adds persistence, tuning, and Caddy (TLS via nip.io); `telegram.yml` adds a stdlib-only alert bridge. The Next.js frontend appears **nowhere** in deployment.

**Observability:** rich Prometheus metrics from all three services; `deploy/vmalert/rules.yml` is 543 lines / 36 alerts carefully gated to avoid paging on sparse-demo behavior (the last three commits are alert-tuning); one provisioned Grafana dashboard whose expressions all reference real metrics.

**CI:** `ci.yml` (fmt + clippy `-Dwarnings` + test + full docker build + foundry) and `frontend.yml` (typecheck/lint/build only). No arena job, no docs-check, no smoke-test, no zk-guest check, no deploy workflow.

**Doc drift:** `DEPLOY.md` describes a Kamal 2 + GHCR + Traefik + Tempo architecture with `config/deploy.yml` and `.kamal/secrets` — **none of which exist**. `deploy/tempo.yml` is dead config. AGENTS.md references a nonexistent `just deploy-dashboard`.

## Strengths

- **Alert rule quality is far above typical solo-project level:** compound gating expressions that encode real learned failure modes (a stale-GTC incident, a fill-rate threshold that avoids paging on IOC rotation, a divergence gauge joined against recent-fill activity to mask stale per-market series). Every alert-referenced metric was verified to exist in code.
- **Memory discipline is deliberate and documented end-to-end:** Dockerfile build constraints commented with the host budget, `MALLOC_ARENA_MAX`/heap-trim tuning, per-container limits, and alerts wired to the same numbers.
- The dev/prod/telegram compose layering is a clean idea; the Telegram bridge is a 185-line stdlib-only alternative to a full Alertmanager.
- `scripts/smoke-test.sh` is a real E2E test (accounts, markets, orders, fills, resolution, PnL) — good bones for a CI gate.
- `deploy-reset-state` requires a literal `CONFIRM` argument; the Caddyfile handles SSE/WS streaming correctly.

## Findings

| ID | Kind | Sev | Summary |
|----|------|-----|---------|
| [D5](01-critical-bugs.md) | ops | high | All internal ports on `0.0.0.0` (unauthenticated VictoriaMetrics writes, Grafana `admin/admin`, prover, raw API) + `SYBIL_DEV_MODE=true` in prod → one curl can mint accounts or resolve every market |
| [D6](01-critical-bugs.md) | ops | high | Live OpenRouter key committed at `docs/api-keys.md`; also passed via argv → persists in `docker inspect`, shell history, `/proc/*/cmdline` (flagged in the repo's own ops report 5 weeks ago, unfixed) |
| [D7](01-critical-bugs.md) | ops | high | Entire monitoring/alerting stack on the monitored host; base compose discards alerts by default (`-notifier.blackhole`); no external heartbeat, so the documented "zombie host" mode is undetectable |
| OPS-1 | ops | high | No alert on `sybil_persistence_failures` — the most safety-critical ops event (a wedged redb stalls persistence silently while blocks keep producing in memory) |
| OPS-2 | doc-drift | high | `DEPLOY.md` documents a Kamal/GHCR/Traefik/Tempo stack that does not exist; the documented recovery commands would all fail during an incident |
| OPS-3 | ops | medium | No disk-space alert despite a 73%-full 50 GB disk, 26 GB of accumulated images, and a deploy method that ships a full image every deploy with no prune |
| OPS-4 | ops | medium | Arena container death is not directly alertable (no `up{job="sybil-arena"}` rule; all arena alerts gate on series that vanish when the scrape dies) |
| OPS-5 | design | medium | Container memory limits sum to ~4.3 GB on a 1.9 GB host; the recent bump of api to 1400m makes the zombie-host mode reachable by design |
| OPS-6 | test-gap | medium | CI enforces nothing for the Python arena (ruff, pytest) or the docs vault despite both having tooling; `just check-all` omits arena while AGENTS.md calls it the "CI equivalent"; `-Dwarnings` in CI but not `just lint` → built-in local-green/CI-red divergence |
| OPS-7 | ops | medium | CI docker job rebuilds the whole workspace single-threaded with no cache on every push, then discards the image |
| OPS-8 | inconsistency | medium | Dev/prod compose duplicate entire command arrays (compose replaces, never merges lists) — a standing drift generator that caused the last three fix commits |
| OPS-9 | ops | medium | No backup story: chain state, decisions, and mappings live only in Docker volumes on one disk; the deploy path itself once took prod down |
| OPS-10 | bloat | medium | justfile rot: broken `arena-demo`, a nonexistent recipe in AGENTS.md, EOL `docker-compose` v1 binary, a 452-line grab-bag file mixing 5 concerns |
| OPS-11 | bug | low | Telegram bridge marks an alert sent *before* delivery → a transient Telegram failure suppresses that page for 30 min; one failure aborts the rest of the batch |
| OPS-12 | doc-drift | low | Deployed 10s block interval vs the "~1s FBA" spec, with no documented reconciliation |
| OPS-13 | bloat | low | Root clutter: a dated ops report at root, empty `.agents`/`.codex` dirs, `scp -r deploy` ships `__pycache__` to prod (and never deletes removed files) |
| OPS-14 | debt | low | `status.sh` couples ops tooling to log message text, emoji, and hardcoded account IDs |

Note verified during review: the suspected checked-in caches (`__pycache__`, `.pytest_cache`, `.ruff_cache`, `contracts/cache`, `lean/.lake`) are **not** git-tracked — they self-ignore. The gitignore hygiene concern is limited to `scp -r deploy` shipping `deploy/__pycache__` to the server (OPS-13).

## Ambitious ideas

1. **Kill the laptop-build deploy entirely.** CI already builds the image on every main push — push it to a registry (GHCR, tagged `:sha` + `:latest`) and reduce deployment to `ssh server 'docker compose pull && up -d'`. This removes the dev-machine SPOF (documented in the ops report), gives instant rollback, makes `DEPLOY.md` one paragraph, and lets the `CARGO_BUILD_JOBS=1` hack go since builds leave the 2 GB host.
2. **Make `/opt/sybil/.env` the single secrets channel:** the OpenRouter key joins the Telegram creds there; arena reads it from the environment (drop the `--api-key` argv); deploy recipes lose all key parameters. Zero secrets in process listings, history, or `docker inspect`.
3. **Collapse the four compose files to one with profiles** and move all service tuning flags into env vars with clap/argparse fallbacks — the base-vs-prod command-array duplication that generated the last three fix commits disappears (OPS-8).
4. **Network-isolate by default:** remove every host port except Caddy 80/443; route Grafana (with auth) and the dashboard through Caddy subdomains; VictoriaMetrics/vmalert/prover become compose-network-only; add a `just tunnel` recipe for operator access. Turns "six unauthenticated services on the internet" into a single audited ingress (D5).
5. **Build real death-detection for the known zombie-host mode:** a vmalert Watchdog → dead-man's-switch endpoint, an external HTTPS probe of `/v1/health`, and a Linode-API auto-reboot script (D7).
6. **Restructure the justfile as `just` modules** (root keeps build/test/lint/check-all; `mod zk`, `mod deploy`, `mod docs`), add `arena-test`/`arena-lint`/`docs-check` to `check-all`, and make CI literally run `just check-all` so the two can never drift (OPS-6, OPS-10).
7. **Adopt the ops report as a pattern:** `docs/ops/` with dated incident reports and a `RUNBOOK.md` generated from the justfile doc-comments, replacing the fictional `DEPLOY.md` (OPS-2).
