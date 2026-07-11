# `sybil-polymarket`

Untrusted external integration that mirrors selected Polymarket events, streams
reference prices, submits one-shot MM liquidity, and signs clean source-market
resolutions. It talks to Sybil only through `sybil-client`, shared wire types,
and signing helpers; it never imports sequencer or solver state.

## Read first

- [[REST API]] and [[WebSocket Block Stream]]
- [[Market Resolution]]
- [[Order Admission]] and [[MM Budget Constraint]]

## Runtime actors

```text
SyncActor ── subscriptions/market notices ──► FeedActor / MmActor
FeedActor ── latest CLOB reference snapshot ──► MmActor
ResolutionActor ── signed resolved-source outcome ──► sybil-api
AutoResolveActor (optional, native markets only) ── review queue ──► sybil-api
```

- `SyncActor` polls Gamma, creates markets/groups, writes provenance metadata,
  and maintains the mapping store.
- `FeedActor` maintains CLOB WebSocket prices with REST fallback.
- `MmActor` resumes the Sybil block stream and submits one-shot,
  budget-constrained reference liquidity.
- `ResolutionActor` follows closed mirrored markets and submits signed
  attestations only when the source outcome is unambiguous.
- `AutoResolveActor` is disabled by default. It evaluates configured native
  `api_poll` sources with an LLM, fails closed on fetch/parse/range errors, and
  keeps high-confidence proposals in a durable resolver-side review window.
  Approval or expiry still enters the ordinary signed immediate-attestation
  path; the core oracle has no challenge policy.

## Boundaries and invariants

- External Gamma/CLOB data is untrusted input: validate identifiers, shapes,
  active/closed state, and JSON-within-JSON fields.
- Mapping and display/reference metadata are off-block. They must not become
  validity inputs accidentally.
- Preserve source provenance (`condition_id`, event id/title, external URL) on
  mirrored markets.
- MM submissions are flash/IOC-style liquidity and must not become a second
  internal book.
- Use canonical `sybil-signing` bytes and monotonic nonces for authenticated
  actions.
- A source being signed makes it attributable, not objectively true.
- Provider keys and signer material come from environment/files; never log or
  commit them.

## Catalog modes

`curated_markets.json` is the authoritative curated event-id list;
`native_markets.json` is the native-market catalog. Do not copy their contents,
live status, thresholds, or prices into this guide—they change independently.
Loaders validate both files, and malformed configured catalogs fail startup.

## Code map

| Area | Location |
|---|---|
| Configuration/orchestration | `config.rs`, `main.rs` |
| Gamma/CLOB clients | `polymarket/` |
| Sync/mapping/catalog | `sync.rs`, `mapping.rs`, `curated.rs`, `native.rs` |
| Price feed/MM | `feed.rs`, `mm.rs` |
| Resolution | `resolution.rs`, `signer.rs` |
| Optional LLM resolver | `autoresolve.rs`, `llm.rs` |
| Display categorization | `categorize.rs` |

Run `--help` for current flags instead of duplicating the Clap contract here.

```bash
cargo test -p sybil-polymarket
cargo clippy -p sybil-polymarket
```
