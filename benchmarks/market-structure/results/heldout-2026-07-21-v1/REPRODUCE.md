# Reproducing the evidence package

## Immutable inputs

- Evidence implementation: `29c4651c661cba312f6a1419d06ef9b747e56cc5`
- Frozen protocol commit: `02cc59ab474a97e24d77782141fb4f54a9087be2`
- Protocol: `benchmarks/market-structure/protocol-heldout-2026-07-21-v1.json`
- Consumed held-out seeds: 10000 through 10127

Do not rerun those seeds as new confirmatory evidence. Reproduce analysis from
the retained raw artifact.

## Original commands

The one complete held-out run was:

```sh
cargo run --release -p matching-sim \
  --bin market-structure-experiments --features lp -- \
  --protocol benchmarks/market-structure/protocol-heldout-2026-07-21-v1.json \
  --suite all \
  --output benchmarks/market-structure/results/heldout-2026-07-21-v1/raw/paired-runs.jsonl
```

It atomically published 133,632 rows. The retained copy is deterministic gzip:

```sh
gzip -9 -n -k \
  benchmarks/market-structure/results/heldout-2026-07-21-v1/raw/paired-runs.jsonl
```

The SHA-256 of the uncompressed JSONL produced by that one run is
`a4842c7bcd42f39fd7c3265385a2532c82446937298580115e479f48ec025365`.
Verify it without retaining another copy:

```sh
gzip -cd \
  benchmarks/market-structure/results/heldout-2026-07-21-v1/raw/paired-runs.jsonl.gz \
  | sha256sum
```

The complete identity-free historical capture was:

```sh
uv run scripts/benchmarks/capture_polymarket_spikes.py \
  --protocol benchmarks/market-structure/protocol-heldout-2026-07-21-v1.json \
  --output benchmarks/market-structure/results/heldout-2026-07-21-v1/raw/polymarket-israel-gaza-january-2026.jsonl.gz \
  --manifest benchmarks/market-structure/results/heldout-2026-07-21-v1/raw/polymarket-israel-gaza-january-2026.manifest.json
```

The retained analysis can be regenerated into a new absent directory:

```sh
uv run scripts/benchmarks/analyze_market_structure.py \
  --protocol benchmarks/market-structure/protocol-heldout-2026-07-21-v1.json \
  --runs benchmarks/market-structure/results/heldout-2026-07-21-v1/raw/paired-runs.jsonl.gz \
  --historical benchmarks/market-structure/results/heldout-2026-07-21-v1/raw/polymarket-israel-gaza-january-2026.jsonl.gz \
  --historical-manifest benchmarks/market-structure/results/heldout-2026-07-21-v1/raw/polymarket-israel-gaza-january-2026.manifest.json \
  --output-dir /tmp/sybil-market-structure-heldout-analysis
```

Large tidy tables are retained as gzip. Stream or expand them with, for
example:

```sh
gzip -cd benchmarks/market-structure/results/heldout-2026-07-21-v1/analysis/paired-differences.csv.gz \
  | sed -n '1,5p'
```

Validate all retained artifact hashes from the repository root:

```sh
sha256sum -c benchmarks/market-structure/results/heldout-2026-07-21-v1/artifact-manifest.sha256
```

## Environment

The retained run used Linux x86-64, Rust 1.97.0, Cargo 1.97.0, and the
repository lockfile. Python scripts declare their exact `uv` dependencies in
PEP 723 metadata. The run is integer-only through generation, matching,
accounting, and verification; NumPy floating-point arithmetic is confined to
the reporting layer.
