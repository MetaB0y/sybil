# Local Actor Liquidity Soak

Use this profile to observe the full Actor Liquidity stack on a developer Mac
before any server deployment. It is an opt-in local profile, not a release or
deployment command.

## Isolation guarantees

- The wrapper loads `docker-compose.yml`, `docker-compose.override.yml`, and
  `docker-compose.soak.yml`; it never loads `docker-compose.prod.yml`.
- Docker must use a local Unix/named-pipe endpoint. SSH and TCP Docker contexts
  are rejected.
- The Compose project is fixed to `sybil-local-soak`.
- State uses only `sybil-local-soak-*` volumes and generated credentials under
  the ignored `.sybil-soak/` directory.
- All published ports bind to `127.0.0.1` and differ from ordinary local-dev
  ports.
- No recipe contains SSH, the production host, or a deployment command.

## Start

Docker Desktop and an OpenRouter key are required. Supply the key through the
current shell, ignored root `arena.env`, or ignored `arena/.env`; it is never
copied into the generated actor credential file.

```bash
export OPENROUTER_API_KEY=sk-or-v1-...
just local-soak-up
```

The first build can be slow on a cold Apple-Silicon Docker cache. It builds for
the Mac's native container architecture; it does not emulate the deployment
server architecture. The soak overlay gives `sybil-history` 1.5 GiB of RAM
plus a 2 GiB memory+swap ceiling because dense all-market history can outgrow
the base Compose service's small devnet limit during a multi-hour run.

The bootstrap job runs after API readiness and before MM/Arena startup. On an
empty isolated chain it reserves unfunded account 0, creates account 1 as the
$1,000,000 MM, and creates accounts 2–16 as $20,000 noise principals ($300,000
aggregate). A partial 1–16 account set fails closed. A restart preserves balances and positions; it
never silently refills actors.

The mirror then creates the reviewed mirror/native catalogs and stages the
exact committed liquidity universe. Initial convergence can take several
minutes because it reads live Polymarket metadata and reference prices.

## Observe

| Surface | URL |
| --- | --- |
| Product frontend | `http://localhost:3105` |
| Actor liquidity overview | `http://localhost:3105/dev/overview` |
| Arena dashboard | `http://localhost:8601` |
| Grafana | `http://localhost:3101` (`admin` / `sybil-dev`) |
| API | `http://localhost:3100` |

```bash
just local-soak-status
just local-soak-logs
```

For a multi-hour gate, watch universe convergence, MM any-side and two-sided
coverage, 15/15 noise heartbeats, 22–28% rolling noise market coverage, 10–20%
fill-market coverage, fills/rejections, pending order depth, block latency,
process memory, and actor balance/inventory drift. LLM cadence remains
unchanged; the MM covers the full committed universe while the fifteen noise
actors sample it sparsely. The MM-cross metric is post-hoc only: noise prices
come from the previous committed mark and never inspect the next MM epoch.
The standard profile uses `$200` MM depth per side and randomized `$7`–`$150`
noise order notionals.

## Stop or erase

`stop` preserves the isolated chain for later inspection. `clean` deletes only
the fixed local-soak Compose project, its dedicated volumes, and generated
tokens.

```bash
just local-soak-stop
just local-soak-clean
```
