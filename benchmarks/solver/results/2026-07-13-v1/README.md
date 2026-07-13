# Solver experiment run: 2026-07-13 v1

This directory is the complete retained run of preregistered protocol
`solver-evaluation-v1`.

- Runner implementation: `831dda777e1bc11fb66a13d335401284868a3f03`
- Protocol BLAKE3:
  `9f7871adea363a7a970bec1d72d50d40f9c9c307102c22da8e0d8282da105f59`
- Analysis script SHA-256:
  `2c98ae75eebf8397819fa7452796aa3d0033b1ad7f3f4fdce91010328aa0f77d`
- Integrity: 675/675 declared rows; no missing, duplicate, unexpected, or
  cross-solver fingerprint-mismatched records
- Machine: AMD Ryzen 7 5800X, 32 GB RAM, Linux 6.6.144, Rust 1.97.0

Start with [`summary.md`](summary.md) for generated tables or
[`../../README.md`](../../README.md) for the methodology. The practical
interpretation and deployment recommendation are in
[`../../../../design/solver-benchmark-report-2026-07-13.md`](../../../../design/solver-benchmark-report-2026-07-13.md).

`results.jsonl` is the raw evidence and must not be edited. `protocol.json` and
`metadata.json` establish what was run and where. `summary.json`,
`summary.csv`, `summary.md`, and `figures/` are deterministic derived outputs;
the analysis-script hash above records the exact derivation revision.

The run intentionally retains numerical failures, iteration caps, and the MILP
timeout. Successful-run quality and runtime statistics are conditional and
must be quoted together with their success denominator.
