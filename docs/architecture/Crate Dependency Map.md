---
tags: [infrastructure]
layer: core
status: current
last_verified: 2026-07-03
---

The Rust workspace is organized as a directed acyclic graph (DAG) of crates, with `matching-engine` as the foundation. Every crate depends on `matching-engine` for core types — orders, fills, markets, nanos, market groups, MM constraints. No crate depends upward: the engine knows nothing about solvers, and solvers know nothing about the API.

The dependency DAG flows in three tiers. **Foundation**: `matching-engine` defines the domain model with zero solver logic. **Middle tier**: `matching-solver` (optimization algorithms), `sybil-oracle` (resolution decisions), `sybil-verifier` (block verification and commitment schemas), and `matching-scenarios` (test data generation) all depend on the engine but not on each other. **Top tier**: `matching-sequencer` composes solver + oracle + verifier into the block production pipeline. `sybil-api` wraps the sequencer as an HTTP server. `matching-sim` pulls from scenarios + solver + verifier for benchmarking. `sequencer-sim` is a dev-only harness that drives the sequencer over many batches with synthetic agents (the `sybil-sim` binary); it depends on `matching-sequencer` so that the sequencer library itself ships no simulation code to `sybil-api`.

The ZK boundary is deliberately split from block production. `sybil-zk`
depends on `sybil-verifier` with native qMDB runtime features disabled and
contains the guest-safe transition verifier. `sybil-prover` owns the host-side
proof job type, job-to-guest-input conversion, worker/API artifact surface,
DA publication, and L1 calldata encoding. Its default build depends on
`sybil-verifier` and `sybil-zk` but not on `matching-sequencer`; the optional
`sequencer-store` feature adds the `witgen` subcommands that read persisted
block/proof material from the sequencer store. The sequencer does not depend on
`sybil-zk`; it produces and persists blocks, witnesses, and qMDB proof
material.

The client tier sits below the DTO crate. `sybil-api-types` is the shared source of truth for request/response shapes; `sybil-client` is THE Rust HTTP client for `sybil-api` (SYB-171), typed against those DTOs. Both in-tree Rust consumers use it: `sybil-polymarket` (the mirror + market maker) and the `sybil-admin` CLI (a binary inside `sybil-api`). It replaced two hand-written clients that had drifted independently — there is now exactly one, so no third should be added. It owns base-url + optional service-token (`SYBIL_SERVICE_TOKEN`) auth, response decoding, and the SSE block-stream consumption; each consumer keeps its own concerns (the mirror's reconnect loop and poisoned-market parsing, the admin's audit log) around it.

The Python `arena/` sits outside the Rust workspace entirely, connected only via HTTP to `sybil-api`. This clean boundary means the Python bots can be developed, tested, and deployed independently of the Rust code — they only need a running server. The separation also means the arena doesn't need to compile any Rust code, which is important for Python-first developers who want to build bots without a Rust toolchain.

```mermaid
graph TB
    ENGINE["matching-engine<br/>core types · orders · markets"]

    ENGINE --> SOLVER["matching-solver"]
    ENGINE --> ORACLE["sybil-oracle"]
    ENGINE --> VERIFIER["sybil-verifier"]
    ENGINE --> SCENARIOS["matching-scenarios"]

    SOLVER --> SEQ["matching-sequencer"]
    ORACLE --> SEQ
    VERIFIER --> SEQ

    SEQ --> API["sybil-api"]
    SEQ --> SEQSIM["sequencer-sim<br/>dev-only · sybil-sim bin"]
    API -.->|"HTTP"| ARENA["arena/ · Python"]

    SCENARIOS --> SIM["matching-sim"]
    VERIFIER --> ZK["sybil-zk"]
    VERIFIER --> PROVER["sybil-prover"]
    ZK --> PROVER
    SEQ -.->|"sequencer-store feature"| PROVER
    ZK --> OPENVM["zk/openvm-guest"]

    TYPES["sybil-api-types<br/>shared request/response DTOs"] --> CLIENT["sybil-client<br/>THE Rust API client · SYB-171"]
    CLIENT --> POLY["sybil-polymarket<br/>mirror + market maker"]
    CLIENT --> API
    POLY -.->|"HTTP"| API
```

*Note: `matching-sim` also depends on `matching-solver` and `sybil-verifier` — omitted to keep arrows clean. It's a dev tool for benchmarking.*

## Key Properties
- `matching-engine` is the sole foundation — all crates depend on it
- No upward dependencies: engine doesn't know about solvers, solvers don't know about API
- Sequencer composes middle-tier crates into the block production pipeline
- `sybil-zk` is guest-safe verification; `sybil-prover` owns portable proof jobs and host-side prover input construction
- `sybil-prover witgen ...` is sequencer-side tooling for exporting latest-block proof jobs from the store, gated behind `sequencer-store`
- Default `sybil-prover` builds are the proof-job CLI/service boundary and settlement calldata encoder; they do not depend on the sequencer
- Sequencer owns block production and persistence, not prover input assembly
- `sybil-client` is THE Rust HTTP client (SYB-171): `sybil-polymarket` and the `sybil-admin` CLI both depend on it; no hand-written duplicate should be reintroduced
- Arena connects via HTTP only — no Rust compilation required
- `matching-sim` is a dev tool that cross-cuts multiple crates for benchmarking
- `sequencer-sim` is a dev-only crate: it depends on `matching-sequencer` so the sequencer library stays free of simulation/agent code (nothing `sybil-api` links pulls it in)

## Where This Lives
> `Cargo.toml` — workspace member list and dependency declarations
> Each crate's `Cargo.toml` — specific dependency graph edges

## See Also
- [[Sybil Architecture]] — top-level system overview
- [[Block Lifecycle]] — the pipeline the sequencer orchestrates
- [[REST API]] — the HTTP boundary between Rust and Python
