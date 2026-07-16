# Form-L OpenVM guest

This standalone OpenVM v2.0.0 package verifies a selective escape claim against
an accepted qMDB state root. It reveals the 32-byte
`sybil/openvm/escape-claim/v1` public-input hash consumed by the vault's
dedicated escape verifier adapter.

Its VM configuration and commitment records are independent of
`zk/openvm-guest`; changing one guest never moves the other guest's VM config or
release files. The source fingerprint closure additionally contains
`sybil-escape-claim`, `sybil-zk`, `sybil-verifier`, `matching-engine`, and
`sybil-l1-protocol`.

Safe Stage-2 commands (build/commit/run only):

```bash
just zk-escape-smoke
just openvm-escape-commit
scripts/zk-guest-fingerprint.sh --write
just zk-rebuild-check
```

The current-source commitments are `app_exe_commit 0x008c8f97…` and
`app_vm_commit 0x00618538…`; the full values live in the committed release
record and fingerprint lock.

Do not run OpenVM setup, keygen, or proving on the constrained development box.
