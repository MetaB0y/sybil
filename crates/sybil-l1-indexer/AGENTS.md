# `sybil-l1-indexer`

Sidecar that scans confirmed `SybilVault` logs and submits ordered bridge
lifecycle inputs through service-authenticated API routes.

## Read first

- [[L1 Settlement and Vault]], [[Acknowledged-Write WAL Replay]], and
  [[Deployment Profiles]]
- `sybil-l1-protocol/AGENTS.md`

## Boundaries

- Do not bypass `sybil-api` or mutate sequencer storage directly.
- Confirmation depth, vault/chain identity, deposit ordering, and
  `depositRootByCount` reconciliation are safety checks, not tuning details.
- Cursor files are deployment-bound. Refuse mismatched chain/vault state and
  starts that skip an existing cursor.
- Reorg/root mismatch is fatal; retry only genuinely transient RPC/HTTP errors.
- Public-chain operation still depends on the configured RPC/finality policy.

Run `cargo test -p sybil-l1-indexer`. Exercise compose bridge tests for changes
to scanning, cursor, or API submission behavior.
