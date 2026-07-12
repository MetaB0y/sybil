# ADR 0017: Bound durable account history and expose retention

- Status: accepted
- Date: 2026-07-12

## Context

Fills, equity samples, and account events are durable derived views, but their
redb tables grew without limit. Their HTTP responses also implied complete
history and converted durable-store failures into empty successful responses.
This was both a disk-exhaustion risk and an unsafe accounting contract for bots.

## Decision

Each stream has a timestamp-first secondary index, an age window, and a global
row ceiling. Index and primary rows are written and deleted atomically. Global
ceilings are enforced in the block commit transaction; age deletion uses the
bounded maintenance pass. Production retains fills and events for 30 days,
equity for 31 days, and caps them at 1M, 1M, and 2M rows respectively. Local
defaults remain unbounded (`0`).

HTTP fills and events now use response envelopes. All three account-history
responses expose `retention_min_timestamp_ms` and `history_truncated`; `all`
means all retained rows. Durable read errors propagate as errors. Canonical
portfolio state and the all-time fill counter are not pruned.

Fill pages also expose a per-account pruned-through height and `cursor_gap`. The
Python bot client refuses to advance a non-sentinel cursor across such a gap
and requires reconciliation from canonical portfolio state.

Pruned-through markers are persisted per account in the same deletion
transaction, so a new empty account is distinguishable from a pruned-empty
account. The `0.0` start sentinel also gaps once that account has lost fills.
In-memory dev serving declares `history_scope=memory` and never claims complete
durable history.

The commit path persists the current fill delta (or current-height cache rows
for post-commit callers), never older hot-cache rows. Rewriting the full cache
would resurrect rows removed by retention.

## Consequences

Clients must unwrap fill/event envelopes and must not describe retained counts
or exports as all-time. Stable cursors remain stable inside retained history.
The extra equity day keeps the existing 30-day view within the age window;
range reads also retain one pre-window opening anchor so charts and leaderboard
PnL use the correct boundary value. Equity responses represent at most 5,000
points using bounded-memory downsampling and disclose source count/downsampling.
Rollups remain future work.

The first devnet deployment that enables nonzero global caps uses a fresh
genesis/store. Legacy indexes rebuild in streaming memory, but converging a
very large previously-unbounded table to a hard cap is intentionally not part
of the live block path migration plan.
