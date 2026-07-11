---
tags: [guide, tasks]
status: current
---

# Task guide

> **Use this page to choose the shortest verified path.** It intentionally
> routes to existing source-of-truth docs instead of repeating full procedures.

| Goal | Start here | Success signal |
|---|---|---|
| Exercise signed trading | `just smoke` and [[P256 Authentication]] | Account, signed order, block, fill, and resolution assertions pass |
| Recover a quarantined deposit | Portfolio → **L1 deposits**, then Settings if registration is incomplete | Matching parked value is automatically credited after successful account/signing-key registration |
| Build a bot | [`arena/README.md`](https://github.com/MetaB0y/sybil/blob/main/arena/README.md) | Bot trades through the public API; `just arena-check` passes |
| Deploy or recover | [Deployment index](deployment.md) | Post-deploy and restart gates pass against persistent state |
| Retain an escape path | `crates/sybil-custody/README.md` and [[Operator Replacement]] | Saved openings authenticate to the intended root; real proof path is explicitly verified |
| Change architecture | Relevant crate `AGENTS.md` → architecture note → ADR | Invariants, focused tests, docs checks, and generated pins stay green |

## Signed trading

Run `just smoke` for the maintained HTTP lifecycle. When debugging canonical
bytes directly, use `crates/sybil-client/examples/smoke_sign.rs`; it calls
`sybil-signing` rather than reimplementing layouts. Before building another
client, read [[REST API]] and [[P256 Authentication]].

## Bots and simulations

```bash
cargo run --release -p sybil-api -- --dev-mode --port 3001
cd arena && uv sync && uv run python examples/full_competition.py
```

The arena is an API consumer, not part of exchange validity. Keep strategy,
news, and LLM concerns there; do not import sequencer internals into bots.

## Operations and recovery

Use the [deployment index](deployment.md) to select exactly one runbook. Normal
deploy, fresh genesis, byte backup restore, and witness-import recovery are
different operations. Never reset state as a substitute for diagnosing a
failed restore or migration.

## User custody and escape

Build `sybil-custody`, inspect `--help`, and follow its crate README for
`snapshot`, `reconstruct`, and `escape-claim`. An own-leaf snapshot supports a
user claim; continuing the whole exchange requires the full canonical DA
payload. Unsafe Anvil fixture proofs are not evidence of a real verifier path.

## Quarantined L1 deposits

The portfolio's **L1 deposits** panel shows the connected account's exact
32-byte Sybil routing key. It is derived from the account id and is not a
passkey or signing public key. Use that exact value in the devnet vault deposit
call.

If the indexer cannot resolve a deposit key, it still consumes the deposit in
order and parks the value in the state-committed quarantine ledger. Successful
account creation with an initial signing key, or any later signing-key
registration for the matching account, automatically moves the full parked
amount into that account. The web API currently exposes aggregate quarantine
totals, not owner-scoped entries, so the portfolio cannot confirm the status of
a specific missing deposit. **No L1 quarantine-refund transaction exists
today.** Do not promise or instruct users to request one; registration and
automatic claim are the implemented recovery path.

## Changing the system

| Change | Required context | Focused gate |
|---|---|---|
| Domain arithmetic/order shape | `matching-engine/AGENTS.md` | `cargo test -p matching-engine` |
| Solver | `matching-solver/AGENTS.md`, [[Solver Landscape]] | solver tests plus integer landing/verifier tests |
| Sequencing/persistence | `matching-sequencer/AGENTS.md`, [[Block Lifecycle]], [[Persistence]] | restart, WAL, fence, and witness tests |
| Witness/state/guest | verifier/ZK crate guidance, [[Block Witness]] | golden check, guest fingerprint/commitment workflow, fresh-genesis decision |
| API or frontend data | API guidance, `frontend/DATA_MAP.md` | OpenAPI regeneration plus frontend staleness tests |
| Contract/custody | `contracts/AGENTS.md`, [[L1 Settlement and Vault]] | Foundry, Rust/Solidity vectors, custody gates |

Run `just docs-check`, `just docs-mermaid`, and `just docs-links` whenever the
architecture or its operational surface changes.
