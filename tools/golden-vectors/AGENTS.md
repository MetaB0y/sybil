# `sybil-golden-vectors`

Generator for the shared Rust/Solidity validity vectors in
`golden/golden-vectors.json`.

- Never hand-edit the generated JSON or change a vector merely to satisfy a
  test. Change the owning canonical encoder, run `just golden-write`, and
  review the byte-level diff.
- Domain, layout, or public-input changes also require the relevant contract,
  guest-pin, and validity-boundary review.
- Use `just golden-check` for ordinary verification.
