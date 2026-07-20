# `sybil-market-maker`

Reusable off-chain flash-liquidity actor and pure quote engine. It consumes
typed market registrations plus optional external reference snapshots and
submits one IOC MM quote bundle per live block, plus an ordinary IOC request
when owned complete sets can be converted back to cash.

## Read first

- [[Order Admission]]
- [[MM Budget Constraint]]
- [[WebSocket Block Stream]]

## Boundaries

- This crate has no market-discovery, catalog, Polymarket, or resolver logic.
- Mirrored and native processes own their price sources and feed typed messages.
- Quotes remain one-shot and use one shared MM budget per submitted quote
  bundle; complete-set conversion stays outside that budget.
- Replay repairs lifecycle/inventory state but never emits historical quotes.
- Both owning processes must construct `ValidatedMmConfig` before creating an
  account or actor; unchecked floats must not reach integer protocol inputs.
