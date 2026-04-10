---
tags: [zk]
layer: verification
crate: sybil-verifier
status: current
last_verified: 2026-04-10
---

The Block Witness is a complete audit trail produced alongside every block. It contains everything an independent party needs to verify that the block was produced correctly — without trusting the sequencer or having access to its internal state. It is the input to the [[Four-Layer Verification|4-layer verification pipeline]] and, in the future, to the [[ZK Integration Path|ZK prover]].

A witness captures the full state transition: pre-state (all account snapshots before this batch), post-state (after settlement), the complete list of orders submitted, any rejections with reasons, any `admin_events` applied between blocks, all fills with quantities, prices, and `account_id`, clearing prices per market, total welfare, MM constraints, and market group definitions. Account snapshots now also include `events_digest`, the per-account running hash used for lightweight inactivity proofs. With this data, any verifier can independently re-run settlement from pre-state + fills and confirm the post-state matches. It can check that fills respect order limits, that prices satisfy complementarity (YES + NO = $1), that MM budgets aren't exceeded, that the state root is correct, and that no orders were falsely rejected.

Today, the witness is used by `matching-sim` for offline verification in tests and benchmarks. The verification logic runs the same checks that would eventually be compiled into a SNARK circuit via OpenVM. By designing the witness as a self-contained verification input now, the system ensures that every invariant the ZK circuit will enforce is already being tested on every batch in development.

## Key Properties
- Self-contained: everything needed for independent block verification
- Pre-state + fills → post-state is independently reproducible
- Contains: orders, rejections, admin events, fills, clearing prices, welfare, MM constraints, market groups, account snapshots
- Input to [[Four-Layer Verification]] (today) and [[ZK Integration Path|ZK prover]] (future)
- Produced alongside every block by the sequencer

## Where This Lives
> `crates/sybil-verifier/src/types.rs` — `BlockWitness`, `AccountSnapshot`, `WitnessBlockHeader`

## See Also
- [[Four-Layer Verification]] — the checks run against the witness
- [[Block Lifecycle]] — witness produced in the final step
- [[State Root and Parent Hash]] — the commitments the witness validates
- [[ZK Integration Path]] — witness becomes ZK prover input
