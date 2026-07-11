# Sybil Architecture Review — 2026-07 (historical)

> **Archived:** this audit describes the repository as inspected on 2026-07-02.
> Many findings and proposed deletions were subsequently addressed or made
> obsolete. Preserve it for rationale and regression archaeology, but use
> [`../../../docs/SPEC.md`](../../../docs/SPEC.md), current architecture notes,
> ADRs, code, and tests for present behavior.

This directory is a full, code-grounded architectural review of the Sybil monorepo. It was produced by deep-reading every subsystem (Rust workspace, Python arena, Solidity contracts, ZK stack, frontends, docs, and ops) and cross-checking the load-bearing findings against the source. It is written for implementers: every finding cites files and lines, states the failure precisely, and gives a concrete fix.

The goal is the one the maintainer set: make Sybil **amazing — crispy clear and nice.** That means honest critique, not cheerleading. Sybil has an unusually strong spine (the joint-EG matching thesis, the block-boundary persistence fence, the four-layer verification design, the Obsidian vault). It is also, in the maintainer's own words, "not in a great state, sometimes bloated and inconsistent and maybe buggy." Both are true. This review separates the spine worth keeping from the sediment worth deleting, and names the bugs worth fixing before anything else.

## How to read this

Start at the top and stop when you have what you need.

| Doc | What it is | Read if you want… |
|-----|-----------|-------------------|
| [00-executive-summary.md](00-executive-summary.md) | The verdict, the through-lines, the north star | The 10-minute picture |
| [01-critical-bugs.md](01-critical-bugs.md) | Severity-ranked register of correctness/safety bugs, verified against code | To know what to fix first |
| [02-cross-cutting-themes.md](02-cross-cutting-themes.md) | The ten patterns that recur across subsystems | To understand *why* the repo feels the way it does |
| [30-roadmap.md](30-roadmap.md) | A sequenced, ambitious plan (delete / merge / build) | To plan the work |
| [40-do-not-break.md](40-do-not-break.md) | The load-bearing spine — invariants to protect during the deletion/refactor phases | Before you delete or refactor anything |
| [41-meta-recommendations.md](41-meta-recommendations.md) | Process, scope, and sequencing advice around the review | Before you act on any of it |

Per-subsystem deep reviews:

| Doc | Subsystem |
|-----|-----------|
| [10-core-math-and-solvers.md](10-core-math-and-solvers.md) | `matching-engine`, `matching-solver`, scenarios, sim, fuzz |
| [11-sequencer.md](11-sequencer.md) | `matching-sequencer` (block lifecycle + state + settlement) |
| [12-api.md](12-api.md) | `sybil-api`, `sybil-api-types`, `sybil-signing`, streaming, auth |
| [13-polymarket-mirror.md](13-polymarket-mirror.md) | `sybil-polymarket` |
| [14-oracle-l1-contracts.md](14-oracle-l1-contracts.md) | `sybil-oracle`, `sybil-l1-protocol`, `sybil-l1-indexer`, `contracts/` |
| [15-verification-zk.md](15-verification-zk.md) | `sybil-verifier`, `sybil-zk`, `sybil-witgen`, `sybil-prover`, `zk/`, `lean/` |
| [16-arena.md](16-arena.md) | `arena/` — live runner, sim, markets, nba, dashboards |
| [17-frontend.md](17-frontend.md) | `frontend/web`, Alpine console, `viz/`, presentation strata |
| [18-ops-deployment.md](18-ops-deployment.md) | compose, deploy pipeline, observability, CI, secrets |
| [19-workspace-consistency.md](19-workspace-consistency.md) | crate boundaries, deps, error/log idioms, naming |
| [20-documentation-estate.md](20-documentation-estate.md) | the vault, Mintlify site, AGENTS.md hierarchy, drift |

## Scope and method

- **Method.** Thirteen parallel deep-read passes over the codebase, each returning an architecture description written *from the code*, strengths, evidence-bearing findings, and ambitious restructuring ideas. The highest-severity findings were then re-verified by reading the cited source directly. Findings tagged **VERIFIED** below were confirmed line-by-line during this review; others are reported from the survey with file:line evidence and should be confirmed before acting.
- **State reviewed.** The working copy as of the change *"fix: stabilize devnet alerting and startup"* (uncommitted edits included). Line numbers are current as of that snapshot and will drift; treat them as anchors, not addresses.
- **Not included.** Linear context: the Linear MCP server was offered for authorization but not authorized during this session, so current-sprint tickets and known-issue history are **not** folded into this review. If you re-run with Linear connected, cross-reference the critical-bug register against open tickets to avoid duplicate work.

## The one-paragraph version

Sybil is a genuinely ambitious prediction-market matching engine with a correct and elegant core idea, wrapped in a layer of aspirational generality the code does not yet implement, guarded by safety checks that only log, deployed with dev-mode backdoors open to the internet, and documented in five overlapping estates at different truth levels. Fix the money-leak bugs and the fail-open verification first; then delete the sediment (a god-crate's worth of dead experiments, three redundant frontends, a fictional docs site); then finish the two half-built migrations (hot/cold state split, one canonical-encodings crate) and enforce the integer/no-panic conventions in the type system and CI. Do that and Sybil becomes what it is clearly trying to be.
