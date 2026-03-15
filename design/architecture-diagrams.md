# Architecture Diagrams

---

## 1. System Overview — Three Layers

Sybil is a **prediction market exchange** using Frequent Batch Auctions. Traders place orders on binary-outcome markets (e.g. "Will X happen? YES/NO"). Every few seconds, all pending orders are batched and matched by a welfare-maximizing solver. Fills are settled, a block is produced, and (future) a ZK proof is posted to Ethereum.

```mermaid
graph TB
    subgraph traders["TRADERS"]
        direction LR
        WEB["Web / API clients"]
        BOTS["AI Trading Bots<br/>Python SDK"]
    end

    subgraph core["CORE — Batch Auction Engine"]
        direction TB

        API["REST API + SSE stream"]

        subgraph batch["Batch Processing"]
            direction LR
            COLLECT["Collect orders<br/>into mempool"]
            SOLVE["Solve batch<br/>welfare-maximizing matching"]
            SETTLE["Settle fills<br/>update balances & positions"]
        end

        BLOCK["Sealed Block<br/>fills · prices · state root"]
        ORACLE["Oracle<br/>resolves market outcomes"]

        API --> COLLECT
        COLLECT --> SOLVE
        SOLVE --> SETTLE
        SETTLE --> BLOCK
        ORACLE -.->|"resolve"| SETTLE
    end

    subgraph arena["AI ARENA"]
        direction LR
        COMPETITION["AI Trading Bots<br/>backtesting on historical data & news"]
        DASHBOARD["Streamlit dashboard<br/>pipeline visualization"]
    end

    subgraph zk["ZK LAYER"]
        direction LR
        VERIFIER["Block Verifier<br/>4 independent checks"]
        PROVER["ZK Prover<br/>OpenVM · SNARK"]
        L1["Ethereum L1<br/>state roots · proofs<br/>deposits · escape hatch"]
        VERIFIER -.-> PROVER -.-> L1
    end

    traders -->|"HTTP"| API
    BLOCK -->|"SSE stream"| traders
    BLOCK -->|"BlockWitness"| VERIFIER

    COMPETITION -->|"HTTP"| API

    classDef coreBox fill:#dbeafe,stroke:#2563eb,stroke-width:2px,color:#1e3a5f
    classDef arenaBox fill:#ede9fe,stroke:#7c3aed,stroke-width:2px,color:#3b1f6e
    classDef zkBox fill:#d1fae5,stroke:#059669,stroke-width:2px,color:#064e3b
    classDef traderBox fill:#fef3c7,stroke:#d97706,stroke-width:2px,color:#78350f

    class core coreBox
    class arena arenaBox
    class zk zkBox
    class traders traderBox
```

**Core** is the exchange. Orders flow in → batched → solved → settled → block sealed. This is the `matching-sequencer` crate, which internally uses `matching-engine` (domain types) and `matching-solver` (optimization). The oracle resolves markets when outcomes are known.

**AI Arena** is the external simulation layer. AI bots (informed traders, market makers, noise traders) backtest strategies against historical data and news via a Python SDK. The Streamlit dashboard visualizes pipeline convergence and performance.

**ZK Layer** provides trust. The block verifier validates correctness across 4 layers (match validity, settlement, block integrity, order validation). Today it runs offline in tests. Future: the same verification logic compiles into a SNARK circuit via OpenVM, posting proofs to Ethereum L1 in a Validium architecture — off-chain data, on-chain proofs.

---

## 1b. Core Internals — Engineering Deep-Dive

Zooms into the Core layer showing technical details: order representation, sequencer internals, solver phases, settlement mechanics, and state commitments.

```mermaid
graph TB
    IN["P256 Signed Orders"]

    IN --> SINGLE["Single-market orders"]
    IN --> MULTI["Bundles · Spreads"]
    IN --> MMQ["MM quotes"]

    SINGLE & MULTI & MMQ --> LOCAL["LocalSolver<br/>per-market clearing"]
    LOCAL --> MMATCH["MultiMarketSolver<br/>cross-market matching"]

    MMATCH --> DUAL["DualMaster · Lagrangian relaxation"]
    DUAL --> MMA["MmAllocator · budget knapsack"]
    DUAL -.->|"iterate with λ"| LOCAL

    MMA --> LP["LP · HiGHS"]
    MMA --> MILP["MILP · SCIP"]

    LP & MILP --> UCP["UCP Enforcement<br/>reprice · P_YES+P_NO=$1"]

    UCP --> SETTLE["Settlement<br/>mint shares · update balances"]

    SETTLE --> BLOCK["Block<br/>fills · prices · blake3 state root"]
    SETTLE --> WITNESS["BlockWitness<br/>pre/post state · ZK audit trail"]
    SETTLE --> PEND["Pending orders<br/>unfilled carry over · TTL"]

    classDef inputStyle fill:#fef3c7,stroke:#d97706,color:#78350f
    classDef solverStyle fill:#dbeafe,stroke:#2563eb,color:#1e3a5f
    classDef exactStyle fill:#ede9fe,stroke:#7c3aed,color:#3b1f6e
    classDef ucpStyle fill:#fce4ec,stroke:#e11d48,color:#881337
    classDef settleStyle fill:#dcfce7,stroke:#16a34a,color:#14532d
    classDef outStyle fill:#f5f5f4,stroke:#78716c,color:#292524

    class IN,SINGLE,MULTI,MMQ inputStyle
    class LOCAL,MMATCH,DUAL,MMA solverStyle
    class LP,MILP exactStyle
    class UCP ucpStyle
    class SETTLE settleStyle
    class BLOCK,WITNESS,PEND outStyle
```

**Key technical properties:**
- **Payoff vectors**: Every order is a vector over atomic market states — unifies simple orders, bundles, spreads, and conditionals into one representation. Max 5 markets, 32 states per order (stack-allocated).
- **Integer arithmetic**: All prices and quantities in nanos (1 dollar = 10^9). No floating point anywhere. Overflow-safe via i128 intermediates in settlement.
- **Welfare objective**: `Σ (limit_price - clearing_price) × fill_qty`. The solver maximizes total trader surplus, not volume.
- **UCP (Uniform Clearing Price)**: One price per outcome per market. Enforced post-pipeline — fills are repriced, limit-violating fills dropped, YES/NO quantities balanced.
- **Minting**: When a BuyYes and BuyNo fill match, $1 creates a YES+NO position pair. No counterparty needed — the protocol mints shares.
- **State commitment**: blake3 hash of all account state. Parent hash chains blocks. Designed for ZK proof integration via `BlockWitness`.
- **Pending orders**: Unfilled orders persist across batches with TTL expiry (default 3 batches). MM quotes are one-shot — consumed each batch.

---

## 2. Solver Pipeline

```mermaid
flowchart LR
    IN["Problem"]

    subgraph pipeline["Solver Pipeline"]
        direction LR
        LOCAL["LocalSolver<br/>per-market prices<br/>O(n log n)"]
        MULTI["MultiMarketSolver<br/>bundles & spreads"]
        DUAL["DualMaster<br/>Lagrangian relaxation"]
        MMA["MmAllocator<br/>budget knapsack"]

        LOCAL --> MULTI --> DUAL --> MMA
        DUAL -.->|"iterate"| LOCAL
    end

    subgraph exact["Exact Solvers"]
        direction TB
        MILP["MILP · SCIP"]
        LP["LP · HiGHS"]
    end

    subgraph post["Post-processing"]
        direction TB
        UCP["UCP Enforcement<br/>reprice · trim · filter"]
        CHECK{"welfare >= 0?"}
        UCP --> CHECK
    end

    OUT["MatchingResult"]
    NONE["no fills"]

    IN --> LOCAL
    MMA --> exact
    exact --> UCP
    CHECK -->|"yes"| OUT
    CHECK -->|"no"| NONE

    classDef phase fill:#dbeafe,stroke:#2563eb,color:#1e3a5f
    classDef exactStyle fill:#ede9fe,stroke:#7c3aed,color:#3b1f6e
    classDef postStyle fill:#fef9c3,stroke:#ca8a04,color:#713f12

    class pipeline phase
    class exact exactStyle
    class post postStyle
```

The pipeline runs 4 phases sequentially. DualMaster iterates back through LocalSolver with Lagrangian multiplier updates until prices converge. Feature-gated exact solvers (MILP/LP) run in parallel after the heuristic phases. UCP enforcement reprices all fills at final clearing prices — if total welfare goes negative, the batch produces no fills.

---

## 3. Block Lifecycle — Production, Verification, Settlement

```mermaid
graph TB
    SUBMIT["Orders submitted"]
    SUBMIT --> MEMPOOL["Mempool"] --> SOLVE["Solver"] --> SETTLE["Settlement"]

    SETTLE --> BLOCK["Block<br/>fills · prices · state root"]
    SETTLE --> WITNESS["BlockWitness<br/>pre/post state · full audit trail"]

    BLOCK --> SSE["SSE / REST → Traders"]

    WITNESS -.-> V1["1. Match verification"]
    WITNESS -.-> V2["2. Settlement verification"]
    WITNESS -.-> V3["3. Block integrity"]
    WITNESS -.-> V4["4. Order validation"]

    WITNESS -.->|"future"| ZK["OpenVM → SNARK → Ethereum L1"]

    classDef prodStyle fill:#dbeafe,stroke:#2563eb,color:#1e3a5f
    classDef outStyle fill:#dcfce7,stroke:#16a34a,color:#14532d
    classDef verifyStyle fill:#f5f5f4,stroke:#78716c,color:#292524
    classDef zkStyle fill:#fef3c7,stroke:#d97706,stroke-dasharray:5,color:#78350f

    class SUBMIT,MEMPOOL,SOLVE,SETTLE prodStyle
    class BLOCK,SSE outStyle
    class WITNESS,V1,V2,V3,V4 verifyStyle
    class ZK zkStyle
```

The sequencer produces two outputs: a **Block** (served to traders via SSE/REST) and a **BlockWitness** (complete audit trail). Today the witness is only used by `matching-sim` for offline 4-layer verification. Future: the witness feeds into a ZK prover for on-chain proof posting.

---

## 4. Crate Dependencies

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
    API -.->|"HTTP"| ARENA["arena/ · Python"]

    SCENARIOS --> SIM["matching-sim"]

    classDef foundation fill:#dbeafe,stroke:#2563eb,stroke-width:2px,color:#1e3a5f
    classDef mid fill:#e0f2fe,stroke:#0284c7,color:#0c4a6e
    classDef top fill:#f0f9ff,stroke:#38bdf8,color:#0c4a6e
    classDef py fill:#ede9fe,stroke:#7c3aed,color:#3b1f6e

    class ENGINE foundation
    class SOLVER,ORACLE,VERIFIER,SCENARIOS mid
    class SEQ,SIM,API top
    class ARENA py
```

*Note: `matching-sim` also depends on `matching-solver` and `sybil-verifier` — omitted from the diagram to keep arrows clean. It's a dev tool that pulls from multiple crates for benchmarking.*
