# Verification and ZK Stack

**Crates:** `sybil-verifier`, `sybil-zk`, `sybil-witgen`, `sybil-witgen-cli`, `sybil-prover`; `zk/openvm-guest`, `zk/openvm-tools`; `lean/FisherClearing`

## Verdict

One of the better-engineered parts of the repo — real four-layer verification runs on every block, the exact-keyspace qMDB proof design is genuinely sound against hidden-leaf attacks and is implemented twice (native + hand-rolled guest) with golden tests pinning them together, and the OpenVM guest is a 26-line shim over shared code. But **the verifier is not adversarially sound as a ZK circuit today**: it doesn't bind fills to accounts, doesn't guard arithmetic overflow, doesn't verify the sidecar transition, and doesn't check fill-driven digest updates. Proving itself is scaffolding. This is the subsystem where the trust model and the documented trust model diverge most.

## Architecture as built

**Witness production** (sequencer) builds a `BlockWitness` with 17 fields including three full account-snapshot sets (pre / post-system / post) and two full sidecar snapshots. The sequencer computes `state_root` by calling *into* the verifier crate and runs `verify_full` inline after every block (log-only on failure).

**`sybil-verifier` (~5.5k LOC), four layers:**
- **Layer 1 (match):** per-fill limit/quantity/dedup, MM budget, UCP for single-market binary pure-payoff orders, YES+NO=$1 complementarity, resolved-market exclusion (fills only), conditional activation.
- **Layer 2 (settlement):** replays fills onto post-system-state using the **same** `matching_engine` pure functions the sequencer uses, compares derived vs claimed post-state — **balances and positions only**.
- **Layer 3 (block):** recomputes the state root by inserting all typed leaves into a **fresh in-memory commonware qMDB per call** on a singleton worker thread; blake3 parent-hash chain, consecutive height.
- **Layer 4 (orders):** post-system balance/position coverage for submitted orders, carried-resting byte-match, rejection/expiry checks.

**`sybil-zk` (2.6k LOC)** is the guest-safe transition verifier: public-input binding, previous-state authentication via full leaf-set proofs, system-event replay including per-account blake3 digest updates, new-root per-leaf qMDB proofs plus a **next-key ring** proving exact keyspace, then reruns the match/settlement/orders layers. `guest_commitments.rs` (724 LOC) is a hand-rolled SHA-256 MMR verifier mirroring commonware, golden-pinned to the native roots.

**`sybil-prover`** (single 2,493-line `main.rs`) has eight subcommands; the worker writes `proof_status: "not_started"` (no proving in-process), and `mock-live` fabricates hashes under the same status interface. Actual proving is `cargo openvm` via the justfile.

**`lean/FisherClearing`** (1,935 lines, zero sorries) formalizes the clearing paper's math over ℝ (minting-simplex Fenchel duality, price uniqueness, welfare gap) — real theorems, but disconnected from the integer Rust.

**Doc drift:** `Four-Layer Verification.md` claims 38 violation types (code has 36, 34 reachable) and a strict/lenient welfare tolerance mode that does not exist; the crate `AGENTS.md` says the state root is "BLAKE3 hash of post-state" (it's a SHA-256 qMDB root).

## Strengths

- **Exact-keyspace qMDB proof design** (per-leaf inclusion + next-key ring) is sound against hidden-leaf attacks and covered by adversarial tests (hidden leaf fails the ring, reordered proofs, corrupted chunk, forged previous-state leaves).
- **Dual implementation with golden pinning:** native commonware roots vs the hand-rolled guest MMR verifier are locked by byte-level golden tests, so silent divergence is caught.
- Clean prover pipeline layering (store → versioned portable job → prepare → encode → 26-line guest), with native re-verification at every hop.
- Real test depth (79 tests incl. proptest properties: validation monotonicity, fill-order commutativity, state-root order-independence).
- Domain-separated, canonicalized, stability-tested leaf schemas.
- The Lean formalization is real mathematics covering the load-bearing design theorems.

## Findings

| ID | Kind | Sev | Summary |
|----|------|-----|---------|
| [H2](01-critical-bugs.md) | bug | high | Verifier never binds `fill.account_id` to the order's account → settlement can charge an arbitrary account under a valid proof |
| [H3](01-critical-bugs.md) | bug | high | Unchecked i64 muls and lossy i128→i64 casts throughout verifier-critical arithmetic; the "overflow-safe" `arithmetic.rs` is dead code → a witness can prove money into existence |
| [H4](01-critical-bugs.md) | design | high | Sidecar transition committed but never verified (order book, reservations, withdrawals, deposit cursor, market statuses) |
| ZK-1 | bug | medium | `verify_settlement` ignores `events_digest` and `total_deposited` in post-state — fill-driven digest updates are unverified (and the zk digest encoders **omit** the fill/mint tags, so the gap is structural) → breaks the range-inactivity proof primitive |
| ZK-2 | debt | medium | `zk/openvm-tools` tests no longer compile (`BlockWitness` literal missing `pre_state_sidecar`); both `zk/` packages are outside the workspace with zero CI and rot silently; committed guest artifacts predate witness-format changes |
| ZK-3 | inconsistency | medium | Guest accepts non-consecutive heights (`new <= prev` only) while native requires `new == prev+1`; `SybilSettlement` also doesn't enforce `+1` |
| ZK-4 | ops | medium | State root rebuilt in a fresh qMDB per call on a singleton worker thread, ≥2× per block, inline in block production — O(total state) hashing at ~1s cadence (also [D2](01-critical-bugs.md)) |
| ZK-5 | design | medium | Layer-3 native check is **circular** (sequencer writes the root and the verifier checks it with the identical function); Layer 2 shares the sequencer's settlement functions — real independence exists only in the guest path, which runs only in smoke tests — see [Theme 2](02-cross-cutting-themes.md) |
| ZK-6 | bloat | medium | Commitment/digest primitives copy-pasted across three crates with divergence already visible (`hash_header` ×3, digest encoders, `bridge_account_key`, two disagreeing reservation encoders) — see [Theme 6](02-cross-cutting-themes.md) |
| ZK-7 | bloat | medium | `sybil-prover` is a 2,493-line single-file god-binary; `mock-live` fabricates hashes into the real status schema and can outrank a real worker artifact at the same height |
| ZK-8 | design | medium | No authorization/signature verification in the witness or guest — proofs attest a *valid* batch, not a *user-authorized* one (sequencer can forge order placement/cancellation/withdrawal-initiation even under full ZK verification); undocumented |
| ZK-9 | design | medium | Guest/witness scaling wall: full tri-state snapshots + two full-keyspace proof sets per block make proving cost O(total state), independent of activity |
| ZK-10 | bloat | low | Dead verifier code: `arithmetic.rs`, two never-emitted `ViolationKind`s, a dangling doc comment mis-documenting `UniformClearingPriceViolation` |
| ZK-11 | bug | low | `verify_resolved_markets` checks only fills, not orders/rejections, contradicting its own comment and the witness field doc |
| ZK-12 | test-gap | low | Lean proves the ℝ-valued paper, not the integer implementation; nothing enforces the link and no CI builds the Lean project |
| ZK-13 | doc-drift | low | Violation counts (36/37/38), phantom strict/lenient mode, BLAKE3-state-root claim, wrong pre/post state in the Layer 2/4 doc |

## Ambitious ideas

1. **Replace the four partially-overlapping layers with one deterministic STF the guest replays end-to-end:** `apply_block(pre_state, pre_sidecar, inputs) -> (post_state, post_sidecar)`, asserting equality with the committed post. This closes H2, H4, and ZK-1 in one move and matches the project's "coherence from one joint program" philosophy. Keep the layers only as a diagnostic decomposition of the STF's failure output.
2. **Extract a `no_std` `sybil-commitments` crate** (canonical bytes, leaf schemas, digest encodings, `hash_header`, bridge keys, checked nanos arithmetic) consumed by sequencer, verifier, sybil-zk, and guest; `deny(clippy::cast_possible_truncation, clippy::arithmetic_side_effects)` in it and `overflow-checks` in the guest profile. Deletes the three `hash_header` copies and the digest-encoder mirror (see [Theme 6](02-cross-cutting-themes.md)).
3. **Make the witness delta-based now** (the docs already call it "Later"): touched-leaf qMDB paths instead of full tri-state snapshots, and drop `post_system_state` (recoverable from `pre_state` + system events). The single biggest lever on proving cost and DA bytes (ZK-9).
4. **Invert the native Layer 3:** have production verification consume the sequencer's persisted qMDB proofs via the sybil-zk verifier path (the independent implementation), and delete the fresh-rebuild worker threads. Native and guest verification then exercise the same independent code and the circularity (ZK-5) disappears.
5. **Consolidate the prover tooling:** fold `sybil-witgen-cli` into `sybil-prover`, split `main.rs` into modules, make mock artifacts structurally distinguishable, and delete `mock-live` once real artifacts flow.
6. **Bring `zk/` into CI** (a pinned-toolchain `cargo check --tests` + a guest-commit staleness gate) and **wire Lean to the code** (`lake build` in CI + one integer-level bridging theorem or generated test vectors consumed by the Rust proptest suite) — turning the formalization from decoration into a regression oracle.
7. **Add an authorization commitment to the witness** (per-order signature or a signed batch root) so proofs eventually attest user-authorized transitions, and state the current "sequencer can forge intent, not balances" trust model explicitly until then (ZK-8).
