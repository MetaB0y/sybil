---
tags: [sybil, plan, escape, custody, zk, syb-32]
status: accepted plan (orchestrator review 2026-07-10; stage 0 next, S1a/1b/2/3 dispatchable after it)
date: 2026-07-10
tickets: [SYB-32, SYB-80, SYB-116]
author: planning lane (Fable), from ratified designs — no new design decisions here
sources: design/escape-claim-guest.md, design/keys-and-escape-ratification.md (D2–D10), ADR-0005/0008/0009/0011/0012/0013/0014/0015, design/witness-v6-keys-transition.md + landed key_transition.rs, design/settlement-aggregation-swirl.md, design/escape-hatch-reconstruction.md, docs/architecture/Operator Replacement.md, repo @ main (v6 landed, 9f578dbf)
---

# SYB-32 staged implementation plan — escape-claim guest + L1 escape path

The custody-arc opener (roadmap 2026-07-10-evening, lane 4). Everything below
sequences **already-ratified** designs: input forms **P + L, L first** (D7),
**newest-accepted-root-only** freshness (D8), **escape bypasses withdrawal delay
AND pause** (D9), one claim per account per root (D10), positions valued at
**last clearing price** (ADR-0013), WebAuthn verified **in-guest** (ADR-0014 —
the "raw backup key" requirement is dropped).

---

## 1. Ground truth (verified in code, 2026-07-10)

### Guest / verifier layer
- **One guest program** exists: `zk/openvm-guest` (extensions: `rv32i, rv32m,
  io, sha2` only — `zk/openvm-guest/openvm.toml`; **no ECC**), entrypoint
  `verify_state_transition_input` in `crates/sybil-zk/src/lib.rs`. Since v6 it
  verifies **both** post-state and pre-state qMDB root proofs
  (`pre_state_root_proof`, `lib.rs:66,325-334`).
- **Witness v6 is landed** (`WITNESS_FORMAT_VERSION = 6`,
  `crates/sybil-verifier/src/witness_schema.rs:28`; commit `9f578dbf`).
  `crates/sybil-verifier/src/key_transition.rs` enforces the full keys_digest
  transition: post/pre weld, reverse+forward fold, global pubkey uniqueness,
  key-op ordering, caps. **Deviation that matters here:** `verify_keyop_auth`'s
  cryptographic half is **deferred** — `validate_authorization`
  (`key_transition.rs:353-382`) checks only that the declared signer is an
  active scheme-matching key; it never verifies the signature. So a malicious
  *sequencer* (who owns admission) can still inject a `KeyRegistered` event
  with a garbage signature and mint its own key into any account — the guest
  accepts it. **Until in-guest signature verification lands in the main guest,
  committed key sets are only admission-honest, and any escape claim
  authorized against them is forgeable by the operator.** This is the single
  hard soundness prerequisite (ADR-0008) this plan must retire.
- **Single-leaf proof machinery already exists in-guest**:
  `verify_qmdb_key_value_proof` + `QmdbStateExclusionProof` (inclusion and
  exclusion), `crates/sybil-zk/src/guest_commitments.rs:24-56,97-108`. Form L
  reuses these; nothing new to invent.
- **Valuation input is NOT committed**: `MarketSnapshot`
  (`crates/sybil-verifier/src/types.rs:358-365`) carries no price field;
  clearing prices exist only per-block in the witness
  (`BlockWitness.clearing_prices: HashMap<MarketId, Vec<Nanos>>`, `types.rs:36`)
  and in unproven sequencer `price_tracker` state. ADR-0013's follow-up
  ("commit `last_clearing_price` per market leaf") has not landed — a state
  schema + wire-version move.
- Account leaf: `AccountSnapshot { id, balance: i64, total_deposited,
  positions: Vec<(MarketId, u8, i64)>, events_digest, keys_digest }`
  (`types.rs:336-346`). Reservations: `acct_resv/{id}` leaf
  (`state_schema.rs:127`). Both leaf families the escape claim needs exist.
- **Fingerprint/commitment discipline**: `scripts/zk-guest-fingerprint.sh`
  (closure = sybil-zk, sybil-verifier, matching-engine, sybil-l1-protocol +
  guest crate), `just openvm-commit` (path-independent build via rustc
  wrapper), `just zk-rebuild-check` (CI-off local gate). Lock currently pins
  `app_exe_commit 0x000f896e…` / `app_vm_commit 0x007a02fc…`
  (`zk/openvm-guest/guest.commitment.lock.json`); the v6 repin + fresh genesis
  is roadmap lane 1 (in flight, orchestrator).
- **In-guest P-256 feasibility is CONFIRMED** on the pinned OpenVM v2.0.0 tag:
  first-class secp256r1 acceleration, drop-in `p256` guest crate,
  `verify_prehash` recipe (`design/openvm-p256-integration.md`; ADR-0008).
  Enabling `modular`+`ecc` moves `app_vm_commit` **of whichever guest enables
  it** — crucially, a *separate* escape guest has its own VM config, so its
  ECC extension does not touch the main guest's pins.

### L1 surface
- `SybilVault.sol`: `escapeModeActive` + `activateEscapeMode()` are deployed
  and live — timeout from `settlement.latestRootVerifiedAt()` (fallback
  `deployedAt`), callable by anyone (`SybilVault.sol:264-283`). The
  `CLAIM_KIND_ESCAPE` constant was deliberately removed; `requestWithdrawal`
  fails closed on any `claimKind != CLAIM_KIND_NORMAL` (`:180`). `paused`
  currently gates deposit/request/finalize (`:154,175,224`) — the D9 carve-out
  (escape checks only `escapeModeActive`) does not exist yet. Nullifier map +
  withdrawal queue exist; `SybilSettlement` exposes `latestStateRoot`,
  `latestHeight`, `isAcceptedRoot`, `rootAt` — everything the newest-root rule
  needs.
- `OpenVmVerifierAdapter.sol` pins exactly **one** `(app_exe_commit,
  app_vm_commit)` pair, immutable at construction. Multi-guest coexistence
  (settlement guest → future epoch guest per SYB-119; escape guest) is
  naturally handled by **one adapter instance per guest**: settlement keeps its
  `verifier` (timelocked swap already exists), the vault gains a second,
  independently-pinned `escapeVerifier`. No adapter redesign needed.
- **No claim prover exists for ANY claim kind**: the domain
  `"sybil/openvm/withdrawal/v1"` appears only in Solidity
  (`SybilVault.sol:374-389`); nothing in Rust produces withdrawal claim
  proofs. Devnet runs `contracts/src/dev/UnsafeAcceptAllVerifierAdapter.sol`;
  the only `requestWithdrawal` caller is
  `contracts/script/UnsafeAnvilSmoke.s.sol:80`. The escape-claim pipeline will
  be the **first** user-side proving path in the system — stage 4 builds that
  muscle from scratch.

### Data access / recovery layer (SYB-80/116 status)
- **R0 landed** (SYB-222, commit `6cd59162`): DA manifest + witness payload per
  height (`crates/sybil-api/src/routes/da.rs`) and qMDB state leaf proofs
  (`routes/proofs.rs::get_state_proof`). The two-file custody snapshot is
  fetchable today.
- **OR-1 landed**: genesis-from-witness importer (`--import-witness`,
  `crates/sybil-api/src/main.rs`, `matching-sequencer/src/store/import.rs`),
  incl. the v6 `account_keys` read-back. **OR-2 landed**: order/cancel
  canonical bytes are `genesis_hash`-domain-separated
  (`matching-sequencer/src/crypto.rs:320-335`) — cross-genesis replay is dead.
- **R1 custody CLI does NOT exist** (no `custody` verbs anywhere in
  `crates/sybil-client`); R2 off-operator retention does not exist.
- **SYB-272 quarantine (ADR-0015) is in flight, not landed** (no
  `DepositQuarantined`/`QuarantineClaimed` tags in `types.rs` /
  `event_schema.rs`). It will take wire v7 + event tags 12/13 and its own
  fresh genesis. Its L1 refund hook is explicitly deferred *onto this
  machinery* (ADR-0015 decision 6).

### What the deferred in-guest signature verification blocks, exactly
1. **Trustless escape-claim authorization transitively** — not the claim's own
   signature (the escape guest verifies that itself, in its own VM), but the
   *integrity of the key set it authorizes against*: main-guest key-op events
   currently prove membership-of-declared-signer, not possession. Operator can
   self-insert keys → operator can escape-claim anyone's funds. Retired in
   stage 1b.
2. Nothing else in this plan: the escape guest carries its own ECC extension
   independent of the main guest's pins.

---

## 2. The staged plan

Shape: 6 stages. 1a/1b/2/3 can run as parallel codex lanes after a half-day
shape-freeze; 4 integrates; 5 deploys+drills; 6 is the deferrable tail.
"Fresh-genesis window" is cheap (ADR-0009/0011) but each window costs a repin +
redeploy + smoke, so stages state which window they ride.

### Stage 0 (half-day, orchestrator): implementation brief + shape freeze
Not a lane — a written brief pinning the frozen interfaces the parallel lanes
build against: `EscapeClaimPublicInputs {stateRoot, height, accountId,
recipient, amount, nullifier}` and its hash domain
`"sybil/openvm/escape-claim/v1"`; the claim canonical-bytes layout
(`"sybil/escape-claim/v1" || genesis_hash[32] || R || H || A || D || X`); the
nullifier formula (`keccak256("sybil/escape-nullifier/v1", chain_id, vault, A,
R)`); the market-leaf price field encoding; and two details the ratified docs
leave open (resolve in the brief, both small): (a) the ADR-0013 "capped at the
account's backing" clause — recommend interpreting as *no per-account cap
beyond floor-0 + checked arithmetic* (price coherence + vault balance is the
systemic cap; document the dust-bend per ADR-0013), (b) never-cleared markets
value at **0** (ADR-0013's stated bend), encoded as an empty price vector.
**Gate:** brief committed to `design/`; Valery eyeballs the two (a)/(b) calls
(5 min, they're ADR-0013-conformant readings, not new decisions).

### Stage 1a — Commit last clearing prices to the market leaf (M)
The ADR-0013 follow-up; without it, positions cannot be valued at rest.
- **Scope:** `crates/sybil-verifier` (`types.rs::MarketSnapshot` gains
  `last_clearing_prices: Vec<Nanos>` (per outcome, empty = never cleared),
  `snapshot_schema.rs` encoders, `state_schema.rs` leaf bytes,
  `witness_schema.rs` wire bump, **transition constraint**: post-leaf prices ==
  witnessed `clearing_prices[m]` for markets that cleared this block, ==
  pre-leaf prices otherwise — same pattern as the landed `verify_sidecar`
  market checks, both ends authenticated since v6 §2e);
  `crates/matching-sequencer` (populate from `price_tracker` at block build;
  genesis snapshots); golden vectors regenerated.
- **Gates:** golden vectors; red→green forgery test (post-leaf price ≠
  witnessed clearing price ⇒ violation; price mutated with no clearing ⇒
  violation); `fingerprint --write`; `just zk-rebuild-check`.
- **Ordering:** moves the wire version + main-guest commitment ⇒ rides a fresh
  genesis. **Batch with SYB-272's wire v7 if quarantine hasn't shipped its
  genesis when this lands** (one window, per ADR-0015's own batching note);
  otherwise it's v8 — fine, genesis is free. Coordinate tag/version allocation
  with the quarantine lane either way.
- **Unblocks:** stage 2's position-valuation arm; ADR-0013 provable at rest.
- **Who:** codex-autonomous (pattern-following; the landed sidecar transition
  checks are the template).

### Stage 1b — Main-guest key-op signature verification (ADR-0008 in the MAIN guest) (M–L)
The v6 §3a normative follow-up; retires the key-forgery hole that would
otherwise let the operator escape-claim user funds.
- **Scope:** `zk/openvm-guest/openvm.toml` gains `modular` + `ecc` (P-256
  constants per `design/openvm-p256-integration.md`) + `openvm::init!`;
  `crates/sybil-verifier` gains the real `verify_keyop_auth`: RawP256
  `verify_prehash` over `SHA256(canonical_keyop_bytes)`, WebAuthn arm per v6
  §3a items 1–5 (escape-safe RFC 8259 challenge extraction, `type`, rpIdHash
  genesis-pinned constant, UP+UV flags, caps re-asserted) — crypto callable
  behind a guest/host feature split per ADR-0003; host-side fuzz corpus for the
  clientDataJSON scanner (browser samples + adversarial escapes). **The
  atomically-coupled API migration** from nonce-bearing admission messages to
  the state-bound canonical key-op bytes (v6 §2a) lands here too — the guest
  can't verify signatures over messages users never signed.
- **Gates:** guest/host parity on shared signature vectors; fuzz run; e2e
  register+revoke through the API with real WebAuthn fixtures;
  `fingerprint --write`.
- **Ordering:** moves the main guest's **`app_vm_commit`** ⇒ same fresh-genesis
  window as stage 1a (one repin, one genesis — exactly the batching the
  escape design §3 called for). Independent of 1a code-wise; parallel lane.
- **Unblocks:** trust-worthy committed key sets — the authorization root of
  every escape claim. Also completes v6's deferred item 1 and de-facto
  unblocks the D4 funded-key floor.
- **Who:** codex-autonomous for the code; the WebAuthn envelope parser
  deserves an adversarial review pass (Fable lane) before the repin.

### Stage 2 — Escape-claim guest, Form L (L)
The second guest program, deliberately tiny.
- **Scope:** new pure verifier lib (recommend `crates/sybil-escape-claim`,
  guest-safe, depending on sybil-verifier for leaf-byte encoders + digest
  code and on the qMDB proof verifiers — reuse
  `verify_qmdb_key_value_proof`/`QmdbStateExclusionProof`, do not fork); new
  guest wrapper `zk/openvm-escape-guest` (own `openvm.toml`: `rv32i, rv32m,
  io, sha2, modular, ecc` — shares stage 1b's ECC recipe and, ideally, the
  same `verify_keyop_auth`-style signature module). Checks, fail closed:
  1. Leaf proofs: `acct/{A}` inclusion, `acct_resv/{A}` inclusion **or
     exclusion** (absence must be proven), `market/{m}` inclusion for every
     market with a nonzero position — all against claimed root R.
  2. Key binding: witness carries A's claimed active key set; guest recomputes
     `keys_digest` and welds it to the acct leaf (same weld as
     `key_transition.rs::weld_post_keys`).
  3. Authorization: P-256 or WebAuthn assertion (both arms, ADR-0014) over the
     canonical claim bytes, signer ∈ welded key set. Zero-key / MINT accounts
     fail closed (D4).
  4. `X = max(0, balance + Σ_(m,o) qty·last_clearing_prices[m][o] −
     open_cash_reservations)`, checked i128 accumulation, never-cleared ⇒ 0.
  5. Reveal `keccak256(abi.encode("sybil/openvm/escape-claim/v1", R, H, A, D,
     X, nullifier))` — same 32-byte reveal shape the adapter already checks.
- **Gates:** unit + property tests (valuation edges: negative positions,
  missing resv leaf, never-cleared markets, price-vector/outcome-count
  mismatch, overflow, wrong signer, cross-account key, wrong genesis_hash
  domain); guest executes via `cargo openvm run` on a real exported devnet
  state; **fingerprint script gains the second closure + lock entries and
  `zk-rebuild-check` covers both guests**; own `commit.json`.
- **Ordering:** cash-only arm can start right after stage 0; the valuation arm
  needs stage 1a's types (compile-time dep, land 1a first in-tree). Does NOT
  gate on stage 1b landing (separate VM configs) — but do not *deploy* escape
  before 1b's genesis (soundness ordering above).
- **Unblocks:** stages 3–6. **Who:** codex-autonomous (design ratified); the
  statement/valuation code gets a Fable adversarial review before pinning.

### Stage 3 — Vault `escapeClaim` + second verifier pin (M) — L1-only, parallel with stage 2
- **Scope:** `contracts/src/SybilTypes.sol` (`EscapeClaimPublicInputs`),
  `SybilVault.sol`: immutable-or-timelocked `escapeVerifier` (second
  `OpenVmVerifierAdapter` instance pinned to the escape guest — keep the
  timelocked-setter parity with `verifier`), `escapeClaim(inputs, proof)`:
  requires `escapeModeActive`, **explicitly does NOT check `paused`** (D9
  carve-out, comment it loudly — this is the SYB-96 exception), enforces
  newest-root-only (`inputs.stateRoot == settlement.latestStateRoot()`),
  recomputes + consumes the escape nullifier (own domain; may share
  `nullifierUsed` since domains are disjoint keccak spaces — pick in the
  brief), verifies against `escapeVerifier`, pays X to D **immediately** (no
  delay, D9). Reinstate the escape claim-kind constant only if the input hash
  carries `claimKind`; a dedicated entrypoint + dedicated domain string is the
  cleaner fail-closed dispatch (recommend that).
- **Gates:** forge tests — pause-on/escape-active pays; escape-inactive
  reverts; stale root reverts; double claim reverts; mock-adapter proof
  plumbing; **golden-vector parity for the escape input hash** (extend the
  single-source `golden/golden-vectors.json` + `SybilGoldenVectors.t.sol`,
  the OL-4 pattern); nullifier cross-domain non-collision test.
- **Ordering:** needs only stage 0's frozen shapes. No fingerprint/genesis
  coupling (contracts redeploy with whatever genesis comes next).
- **Who:** codex-autonomous for devnet contracts + tests. **Any real-network
  deploy of this code is Valery's** (funds-bearing surface).

### Stage 4 — Anyone-can-prove pipeline + custody CLI (SYB-80 R1) (M)
An escape path that needs the operator's prover is not an escape path.
- **Scope:** `sybil custody snapshot` (own-leaf proofs via
  `GET /v1/proofs/state/…` + DA manifest → two small local files, SYB-80 §4),
  `sybil custody reconstruct --height H` (full §3 chain, prints account
  summary + withdrawable, sharing the decode/verify lib with stage 6), and
  `sybil custody escape-claim` (assemble Form-L guest input from snapshot
  or live API → `cargo openvm prove` → encode the adapter ABI proof blob →
  print/submit the `escapeClaim` calldata). Wire an e2e anvil test:
  seed → snapshot → activate escape (short timeout) → prove → claim → assert
  payout (extends the `UnsafeAnvilSmoke` pattern; real proof verified locally
  via `cargo openvm verify` even while the dev adapter is accept-all).
  **Measure and document proving cost on a commodity box** — the guest is
  tiny; record the number (it is a recruiting/credibility datum).
- **Gates:** e2e anvil test green in the compose/itest harness; the SYB-80 §7
  full-snapshot tripwire test (red test if the witness ever stops being a full
  snapshot) — verify it landed with R0, add it here if not.
- **Ordering:** after stage 2 (guest exists) + stage 3 (entrypoint exists).
- **Who:** codex-autonomous.

### Stage 5 — Deploy + escape drill (S orchestration + Valery gates)
- **Scope:** ride the stage-1 fresh-genesis window if the lanes converge
  (one window: wire bump + main-guest repin + new vault + both adapter
  instances), else a second window — both are routine per the
  `devnet-redeploy` runbook, now recording **two** commitment pairs (and a
  third when SYB-119's epoch guest arrives — the runbook + lock files are the
  commitment-set registry; note the epoch guest swaps the *settlement*
  adapter only and never touches `escapeVerifier`). Then the drill (design §6
  item 4, SYB-223 culture): throwaway deployment, short `escapeTimeout`,
  activate escape, run a real user-side Form-L claim end to end from a custody
  snapshot, assert payout; document the user-facing escape runbook; add
  payload-lag alerting + retention SLO monitoring (SYB-80 R2's ops half) to
  the synthetic-monitoring stack. Sanity-check `escapeTimeout` vs future epoch
  cadence (hourly epochs ⇒ timeout must comfortably exceed epoch+proving lag).
- **Gates:** drill transcript committed; smoke extended with escape-path
  probe (activation + claim on the throwaway stack only).
- **Who:** orchestrator + codex for mechanics; **Valery** for: deploy keys,
  the drill go, and sign-off that the pause-bypass semantics ship (he ratified
  D9, but this is the moment it becomes deployed behavior on a funds-bearing
  contract).

### Stage 6 — Form P (payload path) (M, deferrable tail)
- **Scope:** escape guest gains the payload arm: re-run the §3 binding chain
  (`payload_root` → `witness_root` → `da_commitment` vs the claimed
  `RootRecord`), decode the canonical witness (current version only, no
  fallback), extract the same leaves, same downstream checks. Shares the
  reconstruction library with stage 4's `custody reconstruct`. Document the
  encrypted-DA interplay (ADR-0012 / OR-3): once payloads are encrypted, Form
  P operates on *disclosed* plaintext (Shamir release is gated on
  `escapeModeActive` — same trigger, no protocol change; `da_commitment`
  keeps binding plaintext bytes, per SYB-80 §6).
- **Ordering:** any time after stage 2; guest repin of the *escape* guest only.
  Deliberately last — Form L already delivers the zero-DA user floor (D7's
  rationale).
- **Who:** codex-autonomous.

### Hard ordering constraints, summarized
1. **1b before any real escape deploy** (stage 5): claims authorize against
   key sets; key sets are forgeable-by-operator until 1b's genesis. 2 and 3
   may *land in-tree* earlier; they must not be *live* earlier.
2. **1a before 2's valuation arm compiles**; 1a+1b share one fresh-genesis
   window; batch with SYB-272's v7 if its genesis hasn't happened.
3. **Escape guest ECC ≠ main guest ECC**: separate VM configs; stage 2 never
   moves the main pins, stages 1a/1b never move the escape pins. The
   fingerprint lock + runbook track both pairs (three with the epoch guest).
4. **Stage 3 is L1-only** (no genesis coupling); stage 4 needs 2+3; stage 6
   needs only 2.
5. **Newest-root-only (D8) needs zero vault bookkeeping** — do not add any;
   that is the point of the ratified rule.

---

## 3. Risk register (top 5, each with its retiring stage)

1. **Forged valuations via unconstrained prices** (soundness, theft-grade).
   If `last_clearing_prices` lands as a leaf field without the transition
   constraint, the sequencer commits arbitrary prices and inflates escape
   payouts — the exact ZK-1 laundering pattern v6 just killed for keys.
   *Retired: stage 1a's red→green forgery tests (constraint lands with the
   field, same commit).*
2. **Operator key-insertion → operator escape-claims user funds** (key-auth
   gap). `validate_authorization` proves membership, not possession; the
   operator can register its own key on any account today and the v6 guest
   accepts it. Deploying escape before this is closed converts a liveness
   failure into a theft primitive. *Retired: stage 1b (in-guest signature
   verification, both arms) + ordering constraint 1.*
3. **Valuation edge cases at last-clearing-price**: never-traded markets
   (⇒ 0, ratified bend), YES+NO rounding dust (floor-0 + coherence bound it),
   negative/short positions, outcome-count vs price-vector mismatch, i64
   overflow on qty·price, markets resolved-but-position-carrying at R. Any
   one of these mis-summed is an over- or under-payment. *Retired: stage 0
   pins the two open readings; stage 2's property suite enumerates each case;
   stage 5's drill exercises a real portfolio.*
4. **Quarantined-deposit blind spot** (ADR-0015 interplay). Quarantined value
   is committed system state, not account balance — the escape formula never
   sees it, so a depositor whose key never resolved has *no* escape path
   until the deferred L1 refund hook exists; and that hook needs the
   quarantine ledger commitment to be **membership-provable** (or a guest
   that opens the full ledger). If the in-flight SYB-272 lane commits only an
   opaque running digest, the refund hook later forces another schema move.
   *Retired: coordination note to the SYB-272 lane during stage 1a's shared
   window (make the ledger digest openable); stage 3 keeps claim dispatch
   extensible (a future `QuarantineRefund` arm is a new entrypoint + guest
   statement, precluded by nothing); the refund arm itself stays explicitly
   deferred per ADR-0015 decision 6 — document it in the stage-5 runbook.*
5. **Escape-window griefing / double-recovery at the vault edge**: root churn
   (an operator resubmitting roots after activation continuously invalidates
   in-flight newest-root claims — decide in stage 3 whether latest-at-claim
   semantics is accepted (recommended: yes, a root-submitting operator is
   alive and each new root carries everyone's balances) or the activation
   root is recorded as a floor); pause-carve-out regressions (a future
   `paused` refactor silently re-gating `escapeClaim`); nullifier-domain
   collision with normal withdrawals; queued-withdrawal + escape overlap
   (consistent by construction — leaf creation debits balance — but must be
   tested, not assumed). *Retired: stage 3's forge suite (each listed case is
   a named test) + stage 5's drill.*

Watchlist (not top-5): commitment-set drift across three guests (fingerprint
second closure + runbook, stages 2/5); prover accessibility regression (stage
4's measured commodity-box number becomes a periodic check); SYB-111
verifier-side STP revisit is due "inside the escape/custody review" per the
roadmap — fold a yes/no note into stage 2's review.

---

## 4. What the SYB-80 merge means concretely

SYB-80 (escape-hatch data reconstruction) is design-complete; its increments
disperse as follows, then the ticket closes into SYB-116/SYB-32 (roadmap
triage item 10):

| SYB-80 item | Status | Lands in |
|---|---|---|
| R0 — witness/DA API access (SYB-222) | **done** (`6cd59162`; `routes/da.rs`, `routes/proofs.rs`) | — |
| R0 gate — full-snapshot tripwire red test | verify; add if absent | stage 4 |
| R1 — `sybil custody snapshot` / `reconstruct` CLI | not built | stage 4 |
| R2 — off-operator retention + payload-lag alerting | not built | stage 5 (ops half); object-store publisher may trail as an ops follow-up |
| R3 — escape claim (guest + vault) | this plan | stages 1–5 |
| R4 — encryption hook | reserved, untouched | privacy arc (SYB-120 / OR-3), out of scope here; stage 6 documents the interplay |
| §6 escape boundary (dual input, nullifier domains, per-account/root spend) | ratified D7/D10 | stages 2/3/6 |
| OR-1 importer, OR-2 genesis_hash domains (via SYB-116) | **landed** (import.rs + crypto.rs) | — |

Ticket mechanics (when a Linear slot frees): close SYB-80 with a comment
mapping the table above; SYB-116 keeps only OR-3 (disclosure/Shamir,
Valery-gated people-decision) and OR-4 (R-B appointment, parked); SYB-32
carries stages 0–6.

---

## 5. Stage/size/dependency summary

| Stage | What | Size | Depends on | Genesis/pins | Who |
|---|---|---|---|---|---|
| 0 | Brief + shape freeze | XS (½ day) | — | — | orchestrator (+5 min Valery) |
| 1a | Market-leaf prices + constraint | M | 0 | main-guest wire bump + repin, fresh genesis (batch w/ SYB-272) | codex |
| 1b | Main-guest key-op crypto (ADR-0008) | M–L | 0 | same window as 1a (`app_vm_commit` move) | codex + Fable review |
| 2 | Escape guest, Form L | L | 0 (cash), 1a (valuation types) | own commitment pair; fingerprint 2nd closure | codex + Fable review |
| 3 | Vault `escapeClaim` + 2nd pin | M | 0 | none (L1-only) | codex; deploys Valery |
| 4 | Prover pipeline + custody CLI (R1) | M | 2, 3 | none | codex |
| 5 | Deploy + drill + retention ops | S–M | 1a, 1b, 2, 3, 4 | vault + adapters deploy; rides 1's window if converged | orchestrator + **Valery** |
| 6 | Form P payload arm | M | 2 | escape-guest repin only | codex |

Critical path: 0 → {1a ∥ 1b ∥ 2 ∥ 3} → 4 → 5, with 6 trailing. Stage 0 can
start tonight; stages 1a/3 are immediately codex-dispatchable behind it.
