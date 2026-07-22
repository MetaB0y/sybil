# Reproducing the cancel/replace evidence

## Immutable inputs

- Evidence implementation:
  `2f1081c9ff700daa21d2cdd21327761853f61015`
- Frozen protocol commit:
  `764bce7457520362a39af7e72587d278032edcd8`
- Protocol:
  `benchmarks/market-structure/protocol-cancel-lifecycle-heldout-2026-07-22-v1.json`
- Consumed held-out seeds: 30000 through 30063

Do not reuse those seeds as new confirmatory evidence. Reproduce analysis from
the retained raw artifact.

## Original run

The one complete held-out run was:

```sh
cargo run --release -p matching-sim \
  --bin market-structure-experiments --features lp -- \
  --protocol benchmarks/market-structure/protocol-cancel-lifecycle-heldout-2026-07-22-v1.json \
  --suite bundle-lifecycle \
  --output /tmp/sybil-cancel-lifecycle-heldout-2026-07-22-v1/paired-runs.jsonl
```

It atomically published 73,728 rows. The SHA-256 of that uncompressed JSONL is
`7f3cf6c99a70de19e03c5d56111399571562f6640a27bf26e9b41917de07a819`.
The repository retains deterministic gzip bytes. Verify the original stream
without keeping another uncompressed copy:

```sh
gzip -cd \
  benchmarks/market-structure/results/cancel-lifecycle-heldout-2026-07-22-v1/raw/paired-runs.jsonl.gz \
  | sha256sum
```

Regenerate the complete paired analysis into a new absent directory:

```sh
uv run scripts/benchmarks/analyze_market_structure.py \
  --protocol benchmarks/market-structure/protocol-cancel-lifecycle-heldout-2026-07-22-v1.json \
  --runs benchmarks/market-structure/results/cancel-lifecycle-heldout-2026-07-22-v1/raw/paired-runs.jsonl.gz \
  --output-dir /tmp/sybil-cancel-lifecycle-analysis
```

Large tidy tables are retained as deterministic gzip. Stream them with, for
example:

```sh
gzip -cd \
  benchmarks/market-structure/results/cancel-lifecycle-heldout-2026-07-22-v1/analysis/paired-differences.csv.gz \
  | sed -n '1,5p'
```

Validate every retained artifact from the repository root:

```sh
sha256sum -c \
  benchmarks/market-structure/results/cancel-lifecycle-heldout-2026-07-22-v1/artifact-manifest.sha256
```

## Environment

The retained run used Linux x86-64, Rust 1.97.0, Cargo 1.97.0, and the
repository lockfile. Python analysis dependencies are pinned in the script's
PEP 723 metadata. Generation, matching, accounting, and verification are
integer-only; NumPy floating-point arithmetic is confined to reporting.
