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
```

- `SyncActor` polls Gamma, creates markets/groups, writes provenance metadata,
  and maintains the mapping store.
- `FeedActor` maintains CLOB WebSocket prices with REST fallback.
- `MmActor` resumes the Sybil block stream and submits one-shot,
  budget-constrained reference liquidity.
- `ResolutionActor` follows closed mirrored markets and submits signed
  attestations only when the source outcome is unambiguous.

## Boundaries and invariants

- External Gamma/CLOB data is untrusted input: validate identifiers, shapes,
  active/closed state, and JSON-within-JSON fields.
- Mapping and display/reference metadata are off-block. They must not become
  validity inputs accidentally.
- The mapping file is atomically replaced, schema-versioned, and bound to one
  Sybil genesis. A mismatch fails startup and requires clearing integration
  state; market-presence probes are not a chain-identity protocol.
- Preserve source provenance (`condition_id`, event id/title, external URL) on
  mirrored markets.
- MM submissions are flash/IOC-style liquidity and must not become a second
  internal book.
- Use canonical `sybil-signing` bytes and monotonic nonces for authenticated
  actions.
- A source being signed makes it attributable, not objectively true.
- Provider keys and signer material come from environment/files; never log or
  commit them.
- Sync, feed, MM, and optional resolution actors publish write-only progress
  to the owning process's private health/metrics boundary. The API retains
  independent reference-price expiry but does not infer actor health.

## Curation

`curated_markets.json` is the authoritative curated Polymarket event-id list.
Native catalogs and static-anchor liquidity belong to `sybil-native`; this
crate must not create or resolve native markets.

## Code map

| Area | Location |
|---|---|
| Configuration/orchestration | `config.rs`, `main.rs` |
| Gamma/CLOB clients | `polymarket/` |
| Sync runtime/mapping/curation | `sync.rs`, `mapping.rs`, `curated.rs` |
| Pure sync/group/metadata planning | `sync/planning.rs` |
| Price feed | `feed.rs` |
| Shared MM runtime | `../sybil-market-maker/` |
| Resolution | `resolution.rs`, `signer.rs` |
| Display categorization | `categorize.rs` |

Run `--help` for current flags instead of duplicating the Clap contract here.

```bash
cargo test -p sybil-polymarket
cargo clippy -p sybil-polymarket
```
