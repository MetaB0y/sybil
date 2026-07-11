---
tags: [moc, guide]
layer: core
status: current
last_verified: 2026-07-11
---

# Sybil — Documentation & Architecture Guide

> **Sybil is a validity-oriented prediction-market exchange.** Traders bet on
> outcomes ("Will X happen?"); every block, all pending orders clear
> *together* at one uniform price; and the system is built so every state
> transition can be independently checked and proven.

This is the front door. It gives you the whole system in one read, then points
you to the right depth. If you read nothing else, read the **six ideas** below —
they explain almost everything.

---

## The one-paragraph version

Simple orders are validated and reserved in a resting book; atomic bundles and
MM submissions are durably deferred to the next block. Once per block the
exchange takes every eligible order and clears them **all at once** with a welfare-maximizing solver —
no first-mover advantage, everyone in the batch gets the same price. The result
(fills, prices, new balances) is sealed into a **block** with a cryptographic
**state root**. A **verifier** re-derives that block from first principles, and a
**ZK prover** turns that check into a succinct proof for Ethereum. Ethereum
anchors collateral and accepted roots; retained witness data supports auditing
and disaster recovery. Conservative cash escape is implemented, while
production data availability and hostile-operator replacement still require an
operational/governance policy. On top of this runs an **arena** of trading bots
that compete through the same public API.

```mermaid
graph LR
    T["Traders &<br/>Bots"] -->|"signed orders (HTTP)"| API
    subgraph core["Core exchange (Rust, integer protocol truth)"]
        API["API + WS stream"] --> MP["Admission<br/>resting + deferred"]
        MP --> SOLVE["Batch solve<br/>(float search, integer landing)"]
        SOLVE --> SETTLE["Settle"]
        SETTLE --> BLOCK["Sealed block<br/>+ state root"]
        ORACLE["Oracle"] -.->|resolve| SETTLE
    end
    BLOCK -->|WS| T
    BLOCK -->|"witness"| VER["Verifier<br/>(4 layers)"]
    VER --> PROVER["ZK prover<br/>(OpenVM)"] --> L1["Ethereum<br/>vault + roots"]
    BLOCK -.->|"retained witness / DA"| ESCAPE["Recovery / conservative<br/>cash escape"]
    ESCAPE -.-> L1
```

---

## Six ideas that explain the whole system

Read these in order — each builds on the last. Each links to the deep note and,
where relevant, the decision record (**[ADR](adr/README.md)**) that says *why*.

**1. Frequent Batch Auctions — time doesn't buy you a better price.**
Instead of a continuous order book where the fastest trader wins, Sybil collects
orders over a short window and clears them simultaneously. Within a batch there
is *no* time priority, so there is no latency race and no front-running to be
had. → [[Frequent Batch Auctions]]

**2. Prediction markets are Fisher markets — one program prices everything.**
The clearing isn't a pile of independent order books; it's a single convex
optimization (an Eisenberg–Gale / Fisher-market equilibrium) whose *dual
variables are the prices*. This is the deep idea: prices across related outcomes
come out **coherent by construction**, which is what will one day let Sybil price
*conditional* and *combinatorial* markets ("A if B") that a normal exchange
can't. → [[Welfare Maximization]], [ADR-0001](adr/0001-eg-fisher-market-matching.md)

**3. Float search, integer truth — fast *and* reproducible.**
Solvers explore in floating point (fast, approximate). But the moment a result
becomes *state*, it's quantized to integers and all money math is exact. That
split is why the exchange can be both quick and **bit-for-bit reproducible** —
essential for a system that gets proven. → [[Nanos and Integer Arithmetic]],
[ADR-0004](adr/0004-float-search-integer-truth.md)

**4. Every block is built to be proven — correctness need not trust the operator.**
The same state-transition code runs natively *and* inside a zero-knowledge VM, so
a block is checked by an independent verifier and can be attested by a succinct
proof on Ethereum. Native/guest verification and the contract path exist;
production prover and verifier-adapter deployment are still operational work. →
[[Four-Layer Verification]], [ADR-0003](adr/0003-guest-host-crate-split.md),
[ADR-0006](adr/0006-witness-v3-full-snapshot.md)

**5. Validity is not availability — recovery needs both code and retained data.**
Canonical witnesses can rebuild the last good state, and the escape-claim path
can prove a conservative cash floor on L1. Disaster recovery is implemented;
hostile-operator replacement still needs governance and production DA. → [[Data Availability]], [[L1 Settlement and Vault]],
[ADR-0005](adr/0005-escape-via-operator-replacement.md)

**6. Bots are first-class — the exchange is an agent arena.**
Trading agents (including LLM-driven ones) compete through the same public API as
humans, with a leaderboard and reproducible scoring. Prediction markets are an
ideal agent benchmark — ground truth arrives when markets resolve. → [[Bot
Framework]], [[LLM Trader]]

---

## How a block flows (and how money escapes)

```mermaid
sequenceDiagram
    participant U as Trader
    participant A as API / Actor
    participant S as Solver
    participant V as Verifier / Guest
    participant L1 as L1 Vault
    U->>A: signed order (P256)
    A->>A: durable WAL append (before live)
    Note over A: once per block
    A->>S: solve the batch
    S-->>A: fills + clearing prices (f64)
    A->>A: quantize → integer settle → state root → seal
    A->>V: block witness
    V->>V: 4-layer re-derivation → ZK proof
    V->>L1: post proof + state root
    Note over U,L1: if the operator fails…
    U->>L1: escape claim (proof of cash at last root) → withdraw
```

Durability, WAL ordering, and the single commit fence are
[ADR-0010](adr/0010-acknowledged-write-wal.md) and
[ADR-0002](adr/0002-qmdb-state-single-commit-fence.md). Signature/replay
discipline is [ADR-0007](adr/0007-canonical-bytes-domain-separation.md).

---

## The documentation map — where things live and why

Sybil separates current reference material from design work and history:

| Tree | Question it answers | When to read |
|---|---|---|
| **[`architecture/`](architecture/Sybil%20Architecture.md)** (the MOC) | *How does it work today?* | Understanding a subsystem |
| **[`adr/`](adr/README.md)** | *Why is it this way?* | Before changing a load-bearing decision |
| **[`SPEC.md`](SPEC.md)** | *How does the whole system connect?* | Orientation and cross-cutting invariants |
| **[`GUIDES.md`](GUIDES.md)** | *What is the shortest safe path for this task?* | Trading, bots, operations, custody, or architectural changes |
| **[`protocol-pins.md`](protocol-pins.md)** | *What exact formats/hashes are current?* | Generated release and migration checks |
| **[`runbooks/`](deployment.md#choose-the-right-procedure)** | *How do I operate it?* | Deploying / on-call |
| **[`design/`](https://github.com/MetaB0y/sybil/blob/main/design/README.md)** | *What are we considering or researching?* | Planning; verify status against code |

**Reading paths:**
- *New to Sybil* → the six ideas above → [[Frequent Batch Auctions]] →
  [[Block Lifecycle]] → [architecture MOC](architecture/Sybil%20Architecture.md).
- *Changing validity-critical code* → [consolidated invariants](SPEC.md#11-consolidated-invariants)
  → the relevant ADR → [[Threat Model]] / [[Block Witness]] / [[State Root Schema]].
- *Operating a deployment* → [[Deployment Profiles]] →
  [deployment index](deployment.md) → the relevant runbook.
- *Building a trading bot* → [[REST API]] → [[P256 Authentication]] →
  [[Bot Framework]] → [[Python SDK]].
- *Doing a concrete task* → [task guide](GUIDES.md) → the linked source or
  runbook; avoid reading the vault front-to-back.

> **Status legend.** Notes carry a `status:` in their frontmatter — `current`
> (built and verified), `planned`, or `deprecated`. When a note describes something
> not yet built, it says so. The implementation and tests are final truth;
> `SPEC.md` and `architecture/` are the maintained explanation. `design/` is
> deliberately forward-looking and `design/archive/` is historical evidence,
> not current guidance.

---

*This guide is intentionally high-level and drift-resistant — it avoids exact
counts (checks, solvers, cadence) that change over time. For precise current
numbers, follow the links. Run `just docs-check` for source/site sync,
`just docs-mermaid` for diagrams, and `just docs-links` for maintained external
links.*
