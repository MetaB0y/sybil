# `zk`

Standalone OpenVM workspaces outside the root Cargo workspace. Read
[[ZK Integration Path]] and the affected guest's README first.

- Both guests are validity surfaces with independent commitments. Changes in a
  guest's path-dependency closure can move its executable commitment.
- Ordinary tests must not regenerate commitments, setup/key material, locks,
  or deployment pins. Restore pinned key material; do not replace it locally.
- An intentional guest change uses the `openvm-commit-all`, fingerprint,
  protocol-pin, validity-pin, and validity-boundary workflows as one reviewed
  migration.
- `openvm-tools` only serializes prepared inputs with the pinned OpenVM stack;
  do not pull that dependency graph into normal server builds.
