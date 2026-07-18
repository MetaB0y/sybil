# `sybil-l1-indexer`

Sidecar that scans confirmed `SybilVault` logs and submits ordered bridge
lifecycle inputs through service-authenticated API routes.

## Read first

- [[L1 Settlement and Vault]], [[Acknowledged-Write WAL Replay]], and
  [[Deployment Profiles]]
- `sybil-l1-protocol/AGENTS.md`

## Boundaries

- Do not bypass `sybil-api` or mutate sequencer storage directly.
- Finalized-provider unanimity, vault/chain/source identity, deposit ordering,
  block-hash pinning, and `depositRootByCount` reconciliation are safety checks,
  not tuning details. Confirmation depth is local-unsafe mode only.
- Cursor files are deployment-bound. Refuse mismatched chain/vault state and
  starts that skip an existing cursor.
- Reorg/root/provider/finality mismatch is fatal and durably latched; retry only
  genuinely transient whole-quorum RPC/HTTP errors.
- Public-chain operation assumes at least one configured independently operated
  provider is honest; never silently drop a provider from an acknowledged set.

Run `cargo test -p sybil-l1-indexer`. Exercise compose bridge tests for changes
to scanning, cursor, or API submission behavior.
