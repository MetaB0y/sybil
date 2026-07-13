# Solver experiment run: 2026-07-13 v2

This directory is the complete held-out run of
`solver-evaluation-v2-retained-cash`.

- Frozen implementation: `0f0824ac892d1b9268fa45fded2004f7f9777ff7`
- Protocol BLAKE3:
  `1f85a07b0588618911577dadb4044182d651b3870dd834182f59a3c0f7276e2c`
- Analysis script SHA-256:
  `ab19d98c885e96323fd593fbff5c24b63f3c349ad44e79ae1ae0ce3299f71b9b`
- Integrity: 545/545 declared rows in 135 scenario groups; no missing,
  duplicate, unexpected, or cross-solver fingerprint-mismatched records
- Machine: AMD Ryzen 7 5800X, 32 GB RAM, Linux 6.6.144, Rust 1.97.0

Start with [`summary.md`](summary.md) and its six files in [`figures/`](figures/).
The practical interpretation is in
[`design/solver-benchmark-report-2026-07-13-v2.md`](../../../../design/solver-benchmark-report-2026-07-13-v2.md).

`results.jsonl` is the raw evidence and must not be edited. `protocol.json` and
`metadata.json` establish what ran and where. `summary.json`, `summary.csv`,
`summary.md`, and `figures/` are deterministic analyzer outputs. Every
failure, verifier rejection, and iteration cap remains in its declared
denominator; non-count statistics are conditional on verifier-valid success.

The result revision also contains one post-run, semantics-preserving Clippy
cleanup in the scenario generator (`rank % 2 == 0` to
`rank.is_multiple_of(2)`). The frozen implementation above is the exact runner
revision; no observation was rerun or rewritten after this cleanup.
