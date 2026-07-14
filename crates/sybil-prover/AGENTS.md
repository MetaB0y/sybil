# `sybil-prover`

Host-side proof-job, artifact, DA, service, and calldata tooling for the main
transition and escape guests.

## Read first

- [[ZK Integration Path]], [[Block Witness]], [[Data Availability]], and
  [[L1 Settlement and Vault]]

## Boundaries

- The default crate stays independent of `matching-sequencer`; store export is
  isolated behind `sequencer-store`.
- A proof job is portable, versioned input. Validate job/artifact bindings
  before serving, publishing DA, or encoding L1 calldata.
- Mock-live output and unsafe adapters are development-only and must remain
  unmistakable in type/config/log surfaces.
- The daemon's redb state is authority. Publish proof payloads through an
  attempt directory, fsync, validate/hash, atomically rename, then commit the
  matching state transition. Recovery may repeat work but never skip an epoch.
- Mock and root-STARK envelopes are never L1-submittable. EVM proving remains
  an explicit disabled backend until its verifier/submission preflight lands.
- Provider references and payload bytes must reproduce the witness root and DA
  commitment; L1 submission must use the same public inputs the guest reveals.
- Never regenerate or silently repin guest commitments during an ordinary test.

Run `cargo test -p sybil-prover --all-features`; use the explicit OpenVM smoke
and commitment workflows for real guest changes. Operational details live in
`docs/runbooks/prover-daemon.md`.
