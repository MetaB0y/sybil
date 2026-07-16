# `sybil-market-maker`

Reusable off-chain flash-liquidity actor and pure quote engine. It consumes
typed market registrations plus optional external reference snapshots and
submits one atomic IOC MM bundle per live block.

## Read first

- [[Order Admission]]
- [[MM Budget Constraint]]
- [[WebSocket Block Stream]]

## Boundaries

- This crate has no market-discovery, catalog, Polymarket, or resolver logic.
- Mirrored and native processes own their price sources and feed typed messages.
- Quotes remain one-shot and use one shared MM budget per submitted bundle.
- Replay repairs lifecycle/inventory state but never emits historical quotes.

```bash
cargo test -p sybil-market-maker
cargo clippy -p sybil-market-maker --all-targets -- -D warnings
```
