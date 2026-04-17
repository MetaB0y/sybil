---
tags: [infrastructure, zk]
layer: sequencer
crate: matching-sequencer
status: current
last_verified: 2026-03-15
---

Every block carries two cryptographic commitments: a state root and a parent hash. Together they form a hash chain that makes the sequence of blocks tamper-evident and independently verifiable — the foundation for [[ZK Integration Path|ZK proof integration]].

The **state root** is a BLAKE3 hash of the canonical encoding of all account state: every account's balance and every position across all markets. After [[Settlement|settlement]] completes and all balances and positions are updated, the sequencer serializes the account store into a deterministic byte representation and hashes it. This produces a compact 32-byte fingerprint of the entire exchange state. Anyone with the same set of accounts can independently compute the state root and verify it matches — this is exactly what the [[Four-Layer Verification|block integrity verification layer]] does.

The **parent hash** is the BLAKE3 hash of the previous block's header. Each block header includes the parent hash, creating a chain: block N's header hash becomes block N+1's parent hash. To verify any block, you need its predecessor. To verify the entire history, you start from the genesis block and walk forward. This is the same chaining structure used by blockchains, and it enables the [[ZK Integration Path|Validium architecture]]: the ZK prover attests that each state transition (from parent state root to new state root via the fills in this block) is correct, and the on-chain contract only needs to verify the proof and update the latest state root.

## Key Properties
- State root: BLAKE3 hash of canonical account encoding (balances + positions)
- Parent hash: BLAKE3 hash of previous block's header
- Deterministic serialization — anyone can reproduce the state root
- Hash chain makes block sequence tamper-evident
- Foundation for [[ZK Integration Path|Validium]] proof posting

## Where This Lives
> `crates/matching-sequencer/src/sequencer.rs` — state root computation after settlement
> `crates/matching-sequencer/src/block.rs` — `BlockHeader` with `state_root` and `parent_hash`

## See Also
- [[Canonical Serialization]] — byte-level spec for what gets hashed
- [[Block Lifecycle]] — state root computation is the final step
- [[Block Witness]] — captures pre/post state for verification
- [[Four-Layer Verification]] — Layer 3 verifies state root and parent hash
- [[ZK Integration Path]] — state roots anchor the on-chain proof chain
