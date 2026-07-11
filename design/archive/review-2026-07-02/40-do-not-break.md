# Do Not Break — The Load-Bearing Spine

Both review passes push hard toward *delete and simplify*. The failure mode of that work is ripping out something the system actually stands on. This is the consolidated "don't touch without understanding" list — the invariants and boundaries that are **correct and deliberate**, pulled from the strengths sections across the review. Read it before Phase 1 (deletions) and Phase 2 (the STF collapse).

Rule of thumb: **the safety net must exist before you move load.** Land the failing tests, the solver conformance suite, and fail-closed verification ([SYB-182](https://linear.app/sybilmarket/issue/SYB-182), [SYB-197](https://linear.app/sybilmarket/issue/SYB-197)) *before* the big deletions and the `apply_block` refactor — so invariants are enforced while the code shifts under them.

## Invariants that must survive any refactor

1. **Settlement purity boundary.** `matching-engine/settlement.rs` (`compute_fill_settlement`, `derive_minting`) is pure and shared **verbatim** by the sequencer and `sybil-verifier`. The sequencer and verifier must keep computing settlement with the *same* function — that identity is what makes the verifier meaningful. Do not fork it; do not let the sequencer "optimize" its own copy. Keep the i128 intermediates and the documented truncation direction.

2. **The redb commit fence.** redb is the single commit authority; the two-slot A/B qMDB is fenced by it. Anything written to qMDB without a redb fence flip is uncommitted and must be ignored on recovery. Do **not** introduce a second commit point, do **not** pretend cross-database atomicity the system doesn't have, and do **not** make recovery pick "the newest snapshot" — recovery is fence-driven, fail-closed on metadata mismatch. This is the whole crash model.

3. **Exact-keyspace qMDB proof (next-key ring).** The guest's per-leaf inclusion **plus the next-key ring** is what makes hidden-leaf attacks fail; plain inclusion is not enough. When you evolve the witness to be delta-based (SYB-178 / proof-cost work), preserve the exact-keyspace property wherever absence matters. Keep the golden tests that pin the native ↔ hand-rolled-guest roots together — they are the only thing catching silent divergence.

4. **Minting derivation includes the MINT account's shorts.** Position totals used for `derive_minting` include MINT's existing short inventory, so each block adjusts only the *incremental* imbalance, and the sequencer and verifier derive it identically. If you touch minting, change both sides together and preserve this "incremental, not cumulative" property.

5. **OrderBook is the single reservation authority.** Aggregate reservation maps must equal the sum of per-order reservations at every step, and reservations are re-derived from account state on replay (not trusted from WAL rows). Do not let reservations live in two places, and keep replay re-derivation — it is what stops stale WAL rows from over-reserving after a restart.

6. **Router trust-boundary mount table.** Public / dev / internal-dev route groups are mounted conditionally on `dev_mode`, and route-policy tests assert the exact mount table so a write route cannot silently become public. Do not collapse the groups, and do not move a route between groups without updating the asserting test. (Fixing the dev-mode-in-prod issue, [SYB-173](https://linear.app/sybilmarket/issue/SYB-173), means adding a real auth tier — not weakening this boundary.)

7. **Canonical / signable byte stability.** The `OrderDirection` byte encoding is committed by the ZK `events_root` and pinned by a test; canonical signing bytes have insta snapshots. These are consensus surface. The P5 / R2 consolidation into one `sybil-commitments` crate ([SYB-170](https://linear.app/sybilmarket/issue/SYB-170), [SYB-187](https://linear.app/sybilmarket/issue/SYB-187)) must be **behaviorally identical** — same bytes out — and regenerate/verify the golden vectors and ZK artifacts. "Unify the encoders" is not license to change what they emit.

8. **Determinism rests on integer settlement, not the f64 solve.** Fills are witness data the verifier re-checks; block reproducibility comes from integer settlement + (to-be-added) canonical fill normalization, *not* from the floating-point LP. When you introduce money newtypes ([SYB-196](https://linear.app/sybilmarket/issue/SYB-196)), the settlement and reservation outputs must stay bit-identical to today's integer results — newtypes are a type-safety change, not a numeric one.

9. **Block-boundary persistence philosophy.** The block is the transactional unit; no event sourcing; in-flight state (mempool, current solve, transient actor state) is discarded on crash and rebuilt by normal client behavior. Do not drift the persistence model toward event sourcing while chasing the hot/cold split — the docs' own non-goals say this explicitly.

10. **Durable-before-live admission.** The prepare/commit split durably appends an admission (and the 200-OK contract depends on it) *before* the order becomes live in the book. Do not make an order visible/matchable before it is durably logged, and keep the prepare/commit separation that lets persistence failures discard a prepared block cleanly (it is also the mechanism fail-closed verification will reuse).

## "Looks deletable, but isn't"

Some code reads as dead weight but is load-bearing or intended-load-bearing. Handle with care rather than `rm`:

- **`sybil-verifier/src/arithmetic.rs`** — currently has zero callers, so it scans as dead. It is the *intended* overflow-safe arithmetic layer. **Wire it up** (per [SYB-187](https://linear.app/sybilmarket/issue/SYB-187) / H3), don't delete it — unless you inline equivalent checked-i128 helpers at every call site instead.
- **`lean/FisherClearing`** — decorative-seeming (proves the ℝ-valued paper, disconnected from the Rust). It is the *correctness argument* for the mechanism. Keep it, add `lake build` to CI, and ideally bridge one integer-level theorem — do not drop it as "unused."
- **`sybil-signing`'s "mirror without importing"** — ugly and duplicative, but it is currently the guest-safety / dependency boundary. Consolidate it carefully into `sybil-commitments` (P5); do not simply delete the mirror and import the runtime types into guest-safe code.
- **The MINT (`u64::MAX`) system account** — looks like a magic sentinel; it is the counterparty that absorbs per-market YES/NO imbalance and makes conservation hold. Don't "clean it up" into a normal account.

## Safe to delete (both reviews agree)

For contrast — these are genuinely dead and can go in Phase 1 with git/jj history as the only backup: `arena/nba/`, `matching-solver/src/verifier.rs`, `matching-engine/src/book.rs`, the `PipelineResult` pipeline fossils + dead `viz.rs` halves, the conditional-order machinery (end-to-end), one of the two block-stream transports (keep WebSocket), one of the redundant consoles, `apps/composition-demo`, `arena/live/composition_demo/`, the `mm_*` root scripts, and the fictional Mintlify pages. See [SYB-174](https://linear.app/sybilmarket/issue/SYB-174) and [30-roadmap.md](30-roadmap.md) Phase 1.

## The safe refactor order

1. Land regression tests for the critical bugs + the solver conformance suite + fail-closed verification ([SYB-182](https://linear.app/sybilmarket/issue/SYB-182), [SYB-197](https://linear.app/sybilmarket/issue/SYB-197)). *Net the trapeze first.*
2. Delete the agreed-dead code ([SYB-174](https://linear.app/sybilmarket/issue/SYB-174)) — smaller surface for everything after.
3. Extract `sybil-commitments` with byte-identical output + golden vectors ([SYB-170](https://linear.app/sybilmarket/issue/SYB-170)).
4. *Then* collapse the verification layers into the single `apply_block` STF ([SYB-178](https://linear.app/sybilmarket/issue/SYB-178)) and split the sequencer crate — with the conformance suite guarding every step.

Do it in this order and none of the invariants above is ever unguarded while it moves.
