# Market-structure evidence

This package compares Sybil's frequent batch auction (FBA) with explicit,
conventional continuous-limit-order-book (CLOB) baselines. It is research
machinery, not an exchange-validity input and not evidence of customer demand.

The comparison keeps three evidence tiers separate:

1. **Public historical description.** Projected Polymarket trade records can
   establish that a sweep or markout occurred. Anonymous/public records do not
   identify professional market makers, reconstruct missing cancellations, or
   reveal the price a counterfactual FBA would have produced.
2. **Paired controlled experiments.** Both engines receive the same generated
   fundamentals, information times, order intents, latencies, budgets, and
   random seed. These runs identify consequences of the declared model, not a
   production effect size.
3. **External evidence.** Natural experiments and published market-structure
   work test whether the mechanisms seen in controlled runs generalize. Equity
   findings are informative but are not prediction-market estimates.

Do not pool these tiers into one headline number. Every claim must name its
tier, comparator, regime, and uncertainty.

## Provenance states

- `protocol-development.json` is diagnostic. Its seeds and results are never
  publishable evidence.
- `protocol-heldout-2026-07-21-v1.json` is frozen against pushed implementation
  revision `29c4651c661cba312f6a1419d06ef9b747e56cc5`. It consumes untouched
  seeds 10000 through 10127 exactly once.
- A versioned protocol becomes frozen only after the implementation revision
  is pushed and its hash is recorded. Frozen held-out seeds must not have been
  run during development.
- A retained result directory contains the exact protocol, implementation
  revision, environment, complete raw rows (including failures), tidy paired
  rows, deterministic summary, and a short interpretation.

Do not run the frozen protocol partially for inspection or reuse its consumed
seeds after the complete retained run.

Historical API captures retain only fields needed for the analysis. Participant
addresses and profile metadata are discarded. Transaction hashes remain as
public provenance anchors.

## Commands

`--help` owns the runner's flags and defaults:

```sh
cargo run -p matching-sim --bin market-structure-experiments --features lp -- --help
```

A bounded diagnostic run is:

```sh
cargo run -p matching-sim --bin market-structure-experiments --features lp -- \
  --protocol benchmarks/market-structure/protocol-development.json \
  --suite all --max-configs 1 --seed-count 1 \
  --output /tmp/sybil-market-structure-development.jsonl
```

The runner refuses seeds outside the protocol's active range, and a development
protocol cannot unlock its held-out embargo. It writes via a temporary file and
renames only after every attempted row is flushed. The frozen protocol and
retained result README will contain the exact regeneration commands used for
publishable evidence.

The historical case-study capture is independent of simulation seeds:

```sh
uv run scripts/benchmarks/capture_polymarket_spikes.py \
  --protocol benchmarks/market-structure/protocol-development.json \
  --output /tmp/polymarket-spikes.jsonl.gz \
  --manifest /tmp/polymarket-spikes.manifest.json
```

It fails rather than retain a partial event family or a truncated API page.
Wallets, names, pseudonyms, biographies, and profile images are never written.
Resting-side trade records remain anonymous and are not classified as market
makers.

Analyze a run artifact, optionally joining the complete historical tape, into
strictly paired long-form and summary tables:

```sh
uv run scripts/benchmarks/analyze_market_structure.py \
  --protocol benchmarks/market-structure/protocol-development.json \
  --runs /tmp/sybil-market-structure-development.jsonl \
  --historical /tmp/polymarket-spikes.jsonl.gz \
  --historical-manifest /tmp/polymarket-spikes.manifest.json \
  --output-dir /tmp/sybil-market-structure-analysis
```

The analyzer refuses missing engine rows, mismatched tape or protocol hashes,
duplicate rows, inconsistent seed sets, and historical identity fields. It
leaves conditional metrics undefined when either paired engine has no defined
value and reports zero-fill episodes through their unconditional metrics.

## Publication boundary

A result may support a founder-facing statement only when:

- the paired engines share exogenous inputs and resource constraints;
- the CLOB baseline includes cancellation, price-time priority, and a declared
  risk/collateral policy; displayed, individually executable, and simultaneous
  worst-case coverage are not conflated;
- solver failures, caps, invalid rows, and zero-fill episodes are retained;
- uncertainty is clustered at the independent episode seed;
- sensitivity results include conditions that favor continuous execution;
- no simulated volume, fill, account, or PnL is described as traction; and
- the claim sheet links the statement to a retained table or historical row.
