# Pacing-bundle development artifact — 14 July 2026

This directory retains the complete diagnostic run of
`protocol-pacing-development.json` at immutable Sybil source revision
`0b62dc1f`. It is development evidence: seeds 16000–18403 were observed while
the solvers, integer landing, and measurement code were changing. Do not cite
it as held-out or confirmatory evidence.

The artifact contains every one of the 555 declared solver outcomes. The
analyzer reports 0 missing or duplicate records and 0 cross-solver scenario
fingerprint mismatches. Failures and verifier-invalid negative controls remain
in their original denominators; no other solver substitutes for a failure.

The authoritative generated outputs are `results.jsonl`, `summary.json`,
`summary.csv`, `summary.md`, and `figures/`. `metadata.json` records the host,
toolchain, protocol hash, and run timestamp. Scenario fingerprints use a
canonical encoding of map-backed market and market-maker fields. A separate
fresh-process replay produced identical fingerprints and non-timing outputs on
all 555 rows.

The corresponding interpretation, including the former 67.9% landing gap and
the remaining extreme-range support failure, is in
`design/pacing-bundle-landing-tail-study-2026-07-14.md`.

Reproduce with:

```bash
cargo run --release -p matching-sim --all-features \
  --bin solver-experiments -- \
  --protocol benchmarks/solver/protocol-pacing-development.json \
  --source-revision 0b62dc1f \
  --output-dir benchmarks/solver/results/2026-07-14-pacing-development-v2 \
  --overwrite

python3 scripts/benchmarks/analyze_solver_experiments.py \
  benchmarks/solver/results/2026-07-14-pacing-development-v2
```

