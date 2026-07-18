---
tags: [audit, code-quality, l1, solidity, abi, differential-testing]
layer: verification
status: current
date: 2026-07-18
last_verified: 2026-07-18
---

# Solidity/L1 differential-semantics audit — 2026-07-18

## Result

The reviewed Rust protocol crate, generated Alloy bindings, and Solidity
contracts now share executable checks for withdrawal nullifiers, contract
selectors, and vault event topics. The sequencer no longer owns a duplicate
withdrawal-nullifier ABI encoder, and Rust rejects non-canonical withdrawal
event payloads instead of partially decoding a valid prefix.

No public provider was called, no transaction or proof was submitted, and no
witness/public-input hash, guest commitment, or deployment pin changed. The
additive golden-corpus schema advanced from version 5 to 7 and its generated
protocol-pins inventory was refreshed.

## Scope and evidence boundary

Reviewed:

- deposit and withdrawal hash domains and ABI word layout;
- Rust event signatures, topic counts, data words, dynamic tails, and high-byte
  rejection;
- generated Alloy call/event selectors versus the Solidity source interface;
- normal withdrawal nullifier ownership;
- the deposit, root, withdrawal, cancellation, and escape contract lifecycles;
- indexer provider unanimity, finalized-block selection, block-hash pinning,
  deposit-root reconciliation, fatal latch, cursor binding, and retry policy;
- checked-in cross-runtime golden generation; and
- existing issues for real-value proof, deployment-domain, and Ethereum
  finality trust.

Architecture and instructions read:

- `crates/sybil-l1-protocol/AGENTS.md`;
- `crates/sybil-l1-indexer/AGENTS.md`;
- `contracts/AGENTS.md`;
- `L1 Settlement and Vault`;
- `Operator Replacement`;
- `State Root Schema`;
- `Canonical Serialization`;
- `ZK Integration Path`;
- `Data Availability`;
- `Acknowledged-Write WAL Replay`;
- `Deployment Profiles`;
- ADR-0013 and ADR-0015; and
- the L1 reorg-recovery runbook.

The method followed the Solidity ABI's canonical head/tail layout and used
independent runtime consumers: the Rust protocol implementation, Alloy-generated
bindings, and Solidity expressions compiled and executed by Foundry. This pass
did not claim cryptographic Ethereum inclusion/finality or production verifier
security; those are explicitly open work.

## Findings

### L1D-1 — Withdrawal logs admitted non-canonical trailing data

Severity: high. Disposition: fixed.

The Rust parser required only a minimum data length. A fixed-layout
`WithdrawalQueued` or `WithdrawalFinalized` log could therefore contain an
arbitrary suffix and still be accepted. `WithdrawalCancelled` ignored the
dynamic `reason` tail completely, so an invalid offset, impossible length,
non-zero padding, or trailing bytes could survive as the same typed event.

The parser now:

- requires the exact fixed-layout length;
- validates the cancellation string offset, length, padded total length, and
  zero padding even though the product type intentionally omits the reason;
- rejects length arithmetic that cannot fit the host; and
- has negative tests for each malformed shape.

The first implementation used the wrong dynamic offset; its regression test
failed and forced the correction from 96 to the canonical 128-byte head. That
is useful falsification evidence rather than a test written only after the
implementation already passed.

### L1D-2 — The sequencer duplicated a value-moving ABI encoder

Severity: high. Disposition: fixed.

`matching-sequencer` independently encoded the withdrawal-nullifier domain and
seven ABI fields even though `sybil-l1-protocol` is the declared owner of L1
domains and ABI bytes. Either copy could drift while retaining internally
consistent tests.

The sequencer now calls `sybil_l1_protocol::withdrawal_nullifier`. The duplicate
word enum, encoder, padding, and integer helpers were deleted. Existing
sequencer nullifier behavior remains covered.

### L1D-3 — ABI bindings and withdrawal nullifiers lacked one shared
cross-runtime gate

Severity: medium. Disposition: fixed.

The existing corpus covered deposits and proof/public-input encodings, but did
not pin the normal withdrawal nullifier or assert that actual host bindings and
Solidity expose the same selectors/topics.

Golden schema version 7 now contains:

- a normal withdrawal-nullifier fixture;
- settlement call/getter selectors;
- vault getter and escape selectors; and
- deposit/withdrawal event topic hashes.

The generator obtains selectors and topics from the generated Alloy bindings.
Foundry independently computes them from the Solidity types/signatures and
computes the nullifier with Solidity `abi.encode`. The protocol crate also
checks its handwritten event signatures and getter calldata against those
binding-derived values. A single checked-in JSON corpus therefore connects all
three boundaries without hand-copying expected hashes.

### L1D-4 — Indexer and contract lifecycle complexity is justified

Severity: none. Disposition: retained.

The reviewed finalized-provider quorum, vault/chain identity, log block-hash
pin, cumulative deposit-root reconciliation, monotonic cursor, and durable
fatal latch each defend a distinct replacement/reorg failure. Combining or
removing them would weaken an explicit invariant rather than simplify an
accidental boundary.

The contract timelocks, pause/escape distinction, root/deposit checkpoint
checks, and rollback-on-token-failure tests similarly cover independent
money-path states. They remain complex on purpose.

## Existing work retained

No duplicate issue was opened:

- GitHub #92 owns binding the normal-withdrawal deployment domain into
  committed validity state.
- GitHub #88 owns cryptographic verification of finalized Ethereum headers,
  receipts, and vault storage.
- GitHub #56 and #57 own production contract deployment and the real on-chain
  verifier.
- GitHub #55 owns the complete escape-hatch path.
- GitHub #89 owns capital-backed real-value account/quarantine economics.

Those are security or product architecture projects, not bounded cleanups that
should be improvised inside this audit.

## Verification

Passed:

- `cargo test -p sybil-l1-protocol`;
- `cargo test -p sybil-l1-indexer`;
- `cargo test -p sybil-l1-abi`;
- `cargo test -p sybil-escape-claim`;
- non-proving `sybil-custody` tests;
- `just golden-check`;
- `just contracts-fmt-check`;
- `just contracts-build`; and
- `just contracts-test` — 81 tests across five suites; and
- `just contracts-coverage` — 77/78 production branches (98.72%), above every
  per-contract and aggregate floor.

The proof-generation tests remain intentionally ignored and were not invoked.

## Residual risk

The differential boundary now detects ordinary source/binding/parser drift, but
it does not prove that a public-chain receipt is canonical or that a submitted
proof is sound. Real-value operation still depends on the open issues above.
Further mutation/fuzz campaign sizing and a live Anvil bridge round trip would
require a stable threat/workload profile; adding arbitrary campaign counts here
would be activity rather than a clear quality win.
