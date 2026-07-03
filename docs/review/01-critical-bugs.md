# Critical & High-Severity Bug Register

This is the "fix first" list: correctness and safety defects, ranked by severity and blast radius. Each entry states the exact failure, the evidence, and the fix. Items marked **VERIFIED** were confirmed by reading the cited code directly during this review; items marked **REPORTED** carry file:line evidence from the survey and should be confirmed before acting (they were consistent with everything else verified, but were not independently re-read).

Line numbers are anchors as of the reviewed working copy and will drift.

---

## Tier 1 — Value leaks and attestable-wrong state

These either lose money at runtime or let a proof attest to invalid state. They are the reason to make verification fail-closed (see [Bug H1](#h1)).

### C1 — Multi-market orders are accepted end-to-end but every solver mis-models them in release **[VERIFIED]**

- **Where:** `crates/matching-solver/src/lp_solver.rs:78` (and eg/conic/iterlp/milp equivalents); `crates/sybil-api/src/convert.rs` (Spread/Bundle/Custom builders); `crates/matching-sequencer/src/sequencer.rs:1485-1490`; `crates/matching-engine/src/settlement.rs`.
- **Failure:** `convert.rs` builds orders spanning up to 5 markets. `plan_admission` **defers** (does not reject) multi-market submissions into the batch (`sequencer.rs:1485-1490`, verified: `eligible = mm_constraint.is_none() && orders.len()==1 && orders[0].num_markets==1`; if not eligible it returns `Deferred`, not `Rejected`). The LP's single-market assumption is only a `debug_assert!` (verified at `lp_solver.rs:78`), and the workspace has **no `[profile.release]`** (verified — `Cargo.toml` contains only `[workspace]` + `[workspace.dependencies]`), so in release the assert is compiled out. The LP then reads only `orders[i].markets[0]` and `payoffs[0..2]`, pricing a bundle `[1,0,0,0]` over (A,B) as a plain BuyYes-on-A; the B leg never enters any balance constraint. Settlement, however, applies the full multi-market stride decomposition and credits/debits **both** markets' positions. The imbalance is absorbed by the MINT account and the conservation drift is only `error!`-logged.
- **Impact:** A user can submit a spread/bundle/custom order and receive positions the solver never priced or collateralized; naked shorts pass because `validation.rs` does not balance-check mixed-sign payoff vectors.
- **Fix:** Pick one direction (see [Theme 1](02-cross-cutting-themes.md)). Minimal: reject `num_markets != 1` and non-one-hot payoffs at admission **and** delete the Spread/Bundle/Custom `OrderSpec` variants until the solver supports them. Replace every `debug_assert` with a hard input check that returns an empty result or error. Do this together with [H1](#h1) so a mis-modeled order can never seal a block.

### C2 — Custom payoff magnitudes break fill-price semantics and leak value **[VERIFIED]**

- **Where:** `crates/sybil-api/src/convert.rs:379-418` (verified: `OrderSpec::Custom` validates market count, price, and state count, but performs **no one-hot or magnitude check** on `payoffs`); `crates/matching-solver/src/lp_solver.rs`; `crates/matching-engine/src/settlement.rs`; `crates/sybil-verifier/src/match_verifier.rs`.
- **Failure:** `Custom` accepts arbitrary `i8` payoffs. For `payoffs=[2,0]` the LP correctly consumes 2 YES shares of supply per unit (balance coefficient `c_y=2`) and fills when `limit >= 2·p_yes`, but `extract_result` sets `fill_price = p_yes` (flat), while settlement credits `payoff · fill_qty = 2·qty` YES shares and debits only `p_yes · qty`. The buyer pays `0.40q`, receives `2q` YES worth `0.80q` — the system loses `0.40q`. The ZK-side checks pass (UCP sees `yes_payoff>0, no_payoff==0` and expects `p_yes ≤ limit`), so the leak is invisible to the verifier; the only symptom is the `error!`-logged conservation mismatch.
- **Fix:** Validate payoff vectors at admission — require exactly one non-zero entry of value ±1 (one-hot binary) for the current engine. Longer term, make fill-price semantics explicit (charge `Σ state_price · payoff`, not a scalar) if magnitudes are ever supported.

### <a id="c3"></a>H2 — Verifier never binds `fill.account_id` to the order's account **[VERIFIED]**

- **Where:** `crates/sybil-verifier/src/settlement.rs:66-74` (verified) and `match_verifier.rs`.
- **Failure:** `derive_post_state` prefers `fill.account_id` when non-zero (verified: `let account_id = if fill.account_id != 0 { fill.account_id } else { order_account.get(...) }`), and no layer asserts `fill.account_id == witness_order[fill.order_id].account_id`. Layer 4 validates balance for the **order's** account; Layer 2 debits the **fill's** account. A malicious witness can have order O owned by funded account A pass Layer 4, with fill F carrying `account_id = B`; settlement then debits B, the claimed post-state matches, and all layers pass.
- **Impact:** Once proofs gate L1 custody, this makes theft attestable — B pays for A's order under a fully valid proof.
- **Fix:** Add a check in `verify_match`/settlement that every fill's `account_id` equals its order's `account_id` (or is 0), with a new `ViolationKind`. Add a divergent-id test.

### H3 — Unchecked verifier arithmetic can prove money into existence **[VERIFIED root cause]**

- **Where:** `crates/sybil-verifier/src/orders.rs:86`; `crates/matching-engine/src/settlement.rs` (i128→i64 casts); `crates/sybil-verifier/src/arithmetic.rs` (dead); `zk/openvm-guest` (no overflow profile).
- **Failure:** `max_cost = order.limit_price as i64 * order.max_fill as i64` with attacker-supplied `u64` values in a witness; no layer range-checks `limit_price ≤ NANOS_PER_DOLLAR`. With `limit_price ≈ 2^62, max_fill = 8`, `max_cost` wraps negative → balance check passes; settlement's `(price as i128 * qty as i128) as i64` truncates. The "overflow-safe" `arithmetic.rs` module that would prevent this has **zero callers** outside its own tests, and `SettlementOverflow`/`IntraBatchDoubleSpend` violation kinds are never emitted.
- **Fix:** Enforce range invariants in the verifier itself (`limit_price ≤ NANOS_PER_DOLLAR`, bounded `max_fill`); route all price×qty through checked i128 helpers that emit `SettlementOverflow`; enable `overflow-checks` in the guest profile; delete `arithmetic.rs` or make it the real arithmetic layer.

### H4 — Sidecar state transition is committed but never verified **[VERIFIED by absence]**

- **Where:** `crates/sybil-verifier/src/lib.rs`, `orders.rs`; `crates/sybil-zk/src/lib.rs`.
- **Failure:** The four layers + guest verify fills/prices, account balances/positions, root recomputation, order admission, and account-only system-event replay. Nothing derives the **post `state_sidecar`** (resting order book, account reservations, bridge withdrawals, deposit cursor, market statuses) from the pre-sidecar + block activity. `verify_orders` reads only the pre-sidecar; `block.rs` only hashes the post-sidecar into the root.
- **Impact:** A malicious sequencer can drop a user's resting order, zero their reservation, or delete a pending withdrawal leaf in the post-sidecar; the root commits the mutilated sidecar and every layer accepts.
- **Fix:** Add a fifth check (or extend Layer 2) that derives the expected post-sidecar and compares it to the witness before it is hashed. The clean version is the single `apply_block` STF in [Theme 2](02-cross-cutting-themes.md).

### H5 — ZK guest never verifies L1 deposit-leaf inclusion; deposit Merkle code is dead **[VERIFIED]**

- **Where:** `crates/sybil-zk/src/lib.rs:900-908`; `crates/matching-sequencer/src/bridge.rs`; `crates/sybil-l1-protocol/src/lib.rs`.
- **Failure:** `verify_public_input_binding` only checks `deposit_root`/`deposit_count` equal the sidecar values; the sequencer stores `deposit.deposit_root` **verbatim from the event** and never recomputes the tree. `deposit_leaf`/`deposit_tree_leaf`/`hash_node` have zero non-test callers. A malicious operator can credit arbitrary unbacked L1 deposits while setting the sidecar `(count, root)` to any on-chain-accepted pair, and the proof verifies. The doc claims "the proof verifies that every newly credited deposit is included" — not implemented.
- **Fix:** Have the guest reconstruct the deposit root from the `L1Deposit` witness events using the `sybil-l1-protocol` primitives and assert each credited `(amount, account, id)` hashes into `inputs.deposit_root`. This gives the dead Merkle code its intended caller.

### H6 — `SybilSettlement` deposit-root checkpoint is bypassable at any not-yet-reached count **[VERIFIED]**

- **Where:** `contracts/src/SybilSettlement.sol:105-108` (verified) and `contracts/src/SybilVault.sol`.
- **Failure:** `submitStateRoot` enforces `vault.depositRootByCount(inputs.depositCount) == inputs.depositRoot` (verified). `depositRootByCount` is a plain mapping returning `bytes32(0)` for any count the vault has not reached. There is no `inputs.depositCount <= vault.depositCount()` bound and no `depositRoot != 0` guard, so a state root with an unreached `depositCount` and `depositRoot = 0` passes (`0 == 0`). This is exactly the check meant to bind deposits to reality.
- **Fix:** Require `inputs.depositCount <= vault.depositCount()` and/or `inputs.depositRoot != bytes32(0)`, and enforce monotonic non-decreasing `depositCount` across accepted heights. Add the Foundry test that currently would catch it.

---

## Tier 2 — Crash loops, silent data loss, safety-net gaps

### <a id="h1"></a>H1 — Invariant checks and full verification are advisory; invalid blocks seal, persist, and broadcast **[VERIFIED]**

- **Where:** `crates/matching-sequencer/src/sequencer.rs` (`finalize_block_state_phase` ~2065-2095; `verify_full` ~2907).
- **Failure:** Post-settlement balance-delta mismatch and per-market `total_yes != total_no` are `error!`-logged, never rejected. `verify_full` runs inline and, on `!valid`, only logs — the block is still assembled, persisted, and broadcast. There is no fail-closed path for a settlement that violates conservation or fails verification.
- **Impact:** This is the runtime amplifier for C1, C2, H2–H4: a mis-solved or mis-modeled batch becomes a permanent block instead of being rejected.
- **Fix:** Make the hard invariants (no non-MINT negative balance, per-market position balance, `verify_full` validity) a **precondition of `commit_prepared_block`**: on failure, abort production and retain pre-block state (the prepare/commit split already supports this — a failed persist already discards the prepared clone). If soft-degradation is wanted for devnet, gate it behind config and default to fail-closed.

### H7 — Bridge-withdrawal WAL replay panics on restore → crash loop **[VERIFIED]**

- **Where:** `crates/matching-sequencer/src/sequencer.rs:858-867` (verified: `for request in wal.pending_bridge_withdrawals { restored.request_bridge_withdrawal(request).expect("...should be valid"); }`, and the L1 deposit loop just above uses `.expect` too).
- **Failure:** With `store_checkpoint_interval_blocks = 120` (prod), the restored `RESTING_ORDERS` snapshot can be up to 120 blocks stale. An order that expired in a skipped empty block (reservation released live) is resurrected with its reservation on restore, so a withdrawal that validated at accept time fails re-validation → `.expect` panics. The same restore path runs in main startup and in the supervisor restart, so the process crash-loops until the WAL row is manually deleted.
- **Fix:** Replace the `expect`s with the drop-with-warning+metric policy already used by `replay_admissions`, or re-order replay so expiry against the restored height runs first.

### H8 — Checkpoint-interval persistence silently discards read-model deltas **[VERIFIED]**

- **Where:** `crates/matching-sequencer/src/actor.rs` (`should_persist_block`, verified: skips blocks with no fills/system-events/bridge data unless `height - last_persisted >= interval`); `sequencer.rs` (`commit_prepared_block`, verified: calls `self.analytics.clear_offblock_pending()` **unconditionally**); `crates/sybil-api/src/config.rs` (interval default 120).
- **Failure:** With the deployed config (500ms blocks, interval 120, in-memory serving caps set to 0), a user places a resting order via direct admission; the next block has no fills → `persisted = false` → `commit` clears the pending "Placed" history event and equity points anyway. They are never written to redb, and since prod serves history/equity from redb with zero in-memory fallback, the account-history endpoint permanently omits the placement.
- **Fix:** Only clear pending deltas when persistence actually happened (thread the `persisted` bool into commit), or force persistence whenever pending deltas are non-empty. Add a test: direct-admit, produce N empty blocks with interval > 1, assert the event reaches redb.

### H9 — Any single arena bot exception tears down the whole arena and abandons all portfolios **[VERIFIED shape]**

- **Where:** `arena/live/trader.py:396-400` (unguarded `float(fv_match.group(1))` where the regex char class includes `.`, so `"FAIR_VALUE: 0.85."` → `float("0.85.")` raises); `arena/bots/base.py` (`on_block` called outside any try; `run()` re-raises); `arena/live/runner.py:269-289` (any task exit → stop all bots, re-raise → container restart → new accounts).
- **Failure:** One LLM completion with a trailing period kills that trader's task, which shuts down all 16 bots and restarts the container, minting fresh accounts and orphaning every open position.
- **Fix:** Wrap the parse in try/except returning `None`; strip trailing dots. Separately, catch `on_block` exceptions in `BaseAgent.run` and log-and-continue so one bad block can never kill a bot. Then move the runner from process-fatal to per-task supervision.

### H10 — Arena's Kelly-vs-Flat experiment is invalidated by a shared destructive news queue **[VERIFIED]**

- **Where:** `arena/live/runner.py:222-232` (verified: one `NewsFeed` wired into every trader); `arena/live/news_feed.py:507-511` (verified: `drain()` does `self._pending.pop(market_id, [])` — a destructive pop on shared state).
- **Failure:** Six LLM traders share one feed; each article is `pop`-ed by whichever trader drains first, so it reaches exactly one of six. The Kelly and Flat arms therefore see different inputs, making the entire A/B comparison scientifically invalid (not just imperfect — the comparison is the arena's whole purpose).
- **Fix:** Broadcast delivery (per-subscriber queues), or better, split analysis from trading: one analyst pass per persona produces a fair-value stream that both Kelly and Flat subscribe to (identical inputs, differing only in sizing). See [16-arena.md](16-arena.md).

### H11 — Deployed `/trade` page signs stale canonical bytes; every signed order is rejected **[VERIFIED shape]**

- **Where:** `crates/sybil-api/static/trade.html:549-558` (`ORDER_SCHEMA` omits `expires_at_block: Option<u64>`); `crates/sybil-signing/src/lib.rs:40` (Rust `CanonicalOrder` includes it — at minimum a trailing `0x00` byte); `crypto.rs`/`actor.rs:756` (verification reconstructs bytes with the field).
- **Failure:** The browser signs a message missing the trailing option byte; the server verifies a different message → `InvalidSignature` for every order. The page's embedded self-check vectors are equally stale, so its startup check passes while producing unverifiable signatures. This page is live (routed at `/trade`, iframed by the deployed console).
- **Fix:** Delete `static/trade.html` (superseded by the Next.js signing path) and the console's Trade tab; if a binary-embedded trade page must stay, regenerate its schema/vectors from the `sybil-signing` snapshots and add a CI check that greps embedded vectors against the `.snap` files.

### H12 — L1 indexer applies unconfirmed tip logs as irreversible credits **[REPORTED]**

- **Where:** `crates/sybil-l1-indexer/src/main.rs:152-157,208`.
- **Failure:** `to = latest.min(...)` where `latest` is `eth_blockNumber`, with no confirmation depth and no re-scan of prior ranges. On an L1 reorg that drops/replaces a tip deposit, the sequencer has already credited it irreversibly (`ingest_l1_deposit` mutates `deposit_cursor`/`deposit_root`); the `DepositGap` check only catches a missing lower id, not a same-id replacement.
- **Fix:** Scan only up to `latest - CONFIRMATIONS`; reconcile `deposit_root` per id against on-chain `depositRootByCount` before crediting; persist `next_from`.

### H13 — Resolving one market deletes its entire market group **[VERIFIED]**

- **Where:** `crates/matching-sequencer/src/sequencer.rs:1752-1753` and `1820-1821` (verified: both resolution paths do `self.market_groups.retain(|g| !g.markets.contains(&market_id));`).
- **Failure:** For a K-market mutually-exclusive group (e.g. election candidates), resolving one member removes the whole group, so the surviving members lose their complete-set/group-minting constraints and can trade/mint without the group relationship.
- **Fix:** Resolve/void all members atomically when one resolves, or remove only the resolved market from the group. Document the intended semantics in `Market Resolution.md` / `Binary Markets and Market Groups.md`.

### H14 — Escape mode is a decorative flag: no escape-claim path exists **[VERIFIED]**

- **Where:** `contracts/src/SybilVault.sol` (verified: `escapeModeActive` is read only in its own already-active guard at line 224; `CLAIM_KIND_ESCAPE` defined at line 19 but never used; no `escapeClaim`/`escapeWithdraw` function exists).
- **Failure:** `activateEscapeMode` sets a flag and emits an event, but there is no proof-backed cash-claim path, so the documented user-safety mechanism ("proof-backed withdrawals from the latest accepted root") is entirely absent. It also reverts while `latestRootVerifiedAt == 0`, so deposits taken before the first accepted root can never escape.
- **Fix:** Implement the escape-cash claim (verify a `claimKind == ESCAPE` proof against `latestStateRoot`, one nullifier per account/root) or delete the escape scaffolding and mark the safety mechanism explicitly unimplemented in the docs.

---

## Tier 3 — High-severity design/ops defects (not single-line bugs)

These are "high" because of blast radius, but the fix is structural, not a patch. Details in the per-subsystem docs.

| ID | Summary | Where | Doc |
|----|---------|-------|-----|
| D1 | Block production (LP solve + `verify_full` + full clone + fsync) runs inline on the single actor task; a slow solve stalls all reads and bursts blocks | `actor.rs`, `sequencer.rs` | [11](11-sequencer.md) |
| D2 | O(total state) per block: full sequencer clone every tick, ≥3 canonical scans, full qMDB leaf rewrite; caused the documented multi-GiB restart incident | `sequencer.rs`, `store.rs`, `qmdb_state.rs` | [11](11-sequencer.md) |
| D3 | Signed orders/cancels have no nonce/replay protection; re-POSTing a signed order creates a new resting order every time | `crypto.rs`, `actor.rs`, `request.rs` | [12](12-api.md) |
| D4 | Unbounded order quantity (no `max_fill` cap anywhere) feeds the i64 overflow in C1/H3 from the public signed endpoint | `convert.rs`, `validation.rs` | [12](12-api.md) |
| D5 | `SYBIL_DEV_MODE=true` in prod → public unauthenticated account-minting and arbitrary market resolution; all internal ports on 0.0.0.0; Grafana `admin/admin` | `docker-compose*.yml`, `routes/markets.rs`, `routes/accounts.rs` | [18](18-ops-deployment.md) |
| D6 | Live OpenRouter API key committed at `docs/api-keys.md`; key also passed via argv (visible in `docker inspect`, shell history) | `docs/api-keys.md`, `justfile`, `docker-compose.yml` | [18](18-ops-deployment.md) |
| D7 | No alert on `sybil_persistence_failures` (the most safety-critical ops event); no disk-space alert on a 73%-full single disk; monitoring stack co-located with no external heartbeat | `deploy/vmalert/rules.yml` | [18](18-ops-deployment.md) |
| D8 | `matching-solver` feature graph is broken (`milp` without `lp` fails to compile) and `matching-sim` force-enables `milp`, dragging bundled SCIP into every build | `matching-solver/Cargo.toml`, `matching-sim/Cargo.toml` | [19](19-workspace-consistency.md) |

---

## Suggested fix order

1. **Close the internet-reachable holes:** D5, D6 (config + delete, hours).
2. **Stop value leaks at the edge:** C1, C2, D4 — reject unsupported/unbounded orders at admission and `convert.rs` (small, mostly deletion).
3. **Make verification fail-closed:** H1 (this alone contains C1/C2/H2–H4 at runtime).
4. **Stop the crash loop and data loss:** H7, H8.
5. **Fix the arena's correctness and its purpose:** H9, H10.
6. **Delete the broken deployed surface:** H11.
7. **Harden the trust-critical checks** as the ZK path approaches custody: H2, H3, H4, H5, H6, H13, H14, H12.
8. **Then** the structural items (D1–D3, D7–D8) as part of the roadmap phases.
