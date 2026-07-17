# `sybil_client` — Python client for the Sybil API

Two layers live here:

| Path | Nature | Edit by hand? |
|------|--------|---------------|
| `_generated/` | **Machine-generated** OpenAPI client (models + per-route request builders). | No — regenerate. |
| `client.py`, `types.py`, `__init__.py` | **Hand-written thin ergonomic layer** the live bots import. | Yes. |

The live arena bots import the thin layer only (`from sybil_client import SybilClient, BuyYes, ...`).
The generated package is a typed substrate the thin layer draws on — it is not a public surface.

## Generated package (`_generated/`)

- **Generator:** [`openapi-python-client`](https://github.com/openapi-generators/openapi-python-client)
- **Pinned version:** `0.29.0` — see `GENERATOR_VERSION` in `scripts/regen-sdk.sh` (keep the two in sync).
- **Layout** (`--meta none`, so it is an importable subpackage, not a standalone distribution):
  - `_generated/client.py` — `Client` / `AuthenticatedClient` (httpx wrappers).
  - `_generated/models/` — `attrs` models for every request/response schema (`to_dict` / `from_dict`).
  - `_generated/api/routes*/` — one module per operation (`list_markets`, `get_market`, `submit_order`, …).
  - `_generated/types.py` — `Unset` / `UNSET` sentinels and file-upload helpers.
- **Runtime deps:** the generated code needs `httpx` and `attrs` only — both already declared in
  `arena/pyproject.toml`. The generator itself is **not** a runtime or dev dependency; it is fetched
  ephemerally via `uvx openapi-python-client@0.29.0` at regen time only (tool-only, per task hygiene).
- **Linting:** excluded from ruff (`tool.ruff.extend-exclude`) — its style is the generator's, not ours.
- **Reproducibility:** post-generation hooks are disabled (`scripts/openapi-python-client-config.yml`),
  so the tree is byte-stable across machines given the same spec + pinned generator.

## Spec provenance

The spec is the `utoipa`-generated OpenAPI 3.1 document rendered by the
`sybil-openapi` binary (the same full development superset pinned by
`crates/sybil-api/tests/openapi_drift.rs`). It is **not** vendored;
`regen-sdk.sh` renders it directly, without starting a profile-dependent HTTP
server. The generated package therefore covers public, owner, service, and dev
operations even though a production runtime mounts only its enabled subset.

Do not infer freshness from a commit hash recorded in this README. After an API
schema change, run the regeneration command below and review the resulting diff.
The generated model docstrings record the current unit contract: share units
use `SHARE_SCALE = 1000` per share and money uses `1e9` nanodollars per dollar.
Every `*_nanos` value is an exact decimal string on the JSON wire; generated
models therefore type those fields as `str`.

## Regenerate

```bash
just arena-sdk-regen              # render the canonical full document and regenerate
# or, when the Rust workspace is mid-refactor / not compiling, feed a spec directly:
SYBIL_OPENAPI=path/or/url ./arena/scripts/regen-sdk.sh
```

The script rewrites `_generated/` only; the hand-written thin layer is never touched. Review the diff
before committing — a non-empty diff after a `sybil-api` change is expected and desired.

## Thin ergonomic layer (`client.py` + `types.py`)

The surface the bots actually use — conveniences the raw generated client does not provide:

- **`SybilClient`** — one async `httpx` client; `service_token` auth header; methods returning the
  ergonomic dataclasses in `types.py` (`get_account`, `list_markets`, `get_prices`, `submit_orders`,
  `buy_yes/no` + `sell_yes/no`, `get_portfolio`, `get_account_fills`, `resolve_market`, …).
- **Block streaming** — `stream_block_events()` preserves replay boundaries for
  side-effecting consumers; `stream_blocks()` and `stream_live_blocks()` provide
  all-block and live-only convenience views with height-based resume, duplicate
  suppression, and reconnect backoff.
- **Unit conversions** — the exchange speaks integer **share-units** (`SHARE_SCALE = 1000` per share)
  and integer **nanodollars** (`NANOS_PER_DOLLAR = 1e9` per $). The layer converts at the boundary:
  - `shares_to_quantity_units()` / `quantity_units_to_shares()`
  - decimal-string nanos on the wire to Python `int` values in ergonomic dataclasses
  - display accessors: `Account.balance_dollars`, `Market.yes_price`/`no_price`, `Fill.fill_price`, …
  - `OrderSpec` builders (`BuyYes.at_price(price=0.55, quantity=10)`) that emit nanodollar limit prices
    and share-unit quantities.
- **Passthroughs** the live mirror bots depend on: `polymarket_condition_id`, and SYB-191 replay-nonce
  signing (per-account monotonic nonces in the canonical signed-order bytes).

### Where the layers meet today

`client.py` already delegates its typed response decoding to the generated `attrs` models
(`AccountResponse`, `AccountFillResponse`, `PortfolioResponse`, `PriceHistoryResponse`) rather than
carrying hand-rolled schema knowledge. Request/route plumbing is still hand-written `httpx` calls
(the thin layer needs streaming, retry, and signing behaviour the generated per-route helpers don't
model).

### Migration path (not done in this pass — deliberately)

Widening the delegation to the generated **route** helpers (`_generated.api.routes*`) is viable but
higher-risk because the live bots run on this path:

1. Move plain request/response endpoints (`get_market`, `list_markets`, `get_prices`) onto the
   generated route functions, keeping the thin dataclass return types.
2. Keep the bespoke paths hand-written: `stream_blocks` (WebSocket resume + reconnect), `submit_orders`
   (signing + nonces), anything with retry semantics.
3. Do it endpoint-by-endpoint behind the existing tests; never in one sweep.
