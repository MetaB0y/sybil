# Escape-Hatch Data Reconstruction (SYB-80)

Status: **DRAFT — for ratification**
Author: Claude (Fable 5), 2026-07-06, grounded in a full code survey at main `46819606`.
Feeds: SYB-32 (escape claim), SYB-116 (operator replacement), SYB-120 (encrypted DA),
SYB-222 (witness/DA API exposure — implementation increment R0 of this design).

## 0. One-paragraph summary

Since witness v3, **every block's DA payload is a complete state snapshot**, not a
delta: the canonical witness carries the full `pre_state_sidecar`, the full account
sections, and the deposit frontier. Reconstruction therefore needs exactly **one**
witness payload whose `da_commitment` matches an accepted on-chain root — no replay
chain, no periodic-snapshot schedule, no per-user data hoarding. The design below
turns that accident of the v3 schema into a load-bearing guarantee, specifies the
client-side verification chain end-to-end, defines what (little) a distrustful user
should keep locally, and draws precise boundaries to the escape-claim guest (SYB-32)
and the encryption layer (SYB-120) so neither is precluded.

## 1. Threat model and goal

The sequencer (single operator today) can: go offline permanently, refuse service to
a user, or serve stale data. It can NOT forge state — every accepted root on
`SybilSettlement` is ZK-verified, and `da_commitment` is a bound public input
(`crates/sybil-zk/src/lib.rs:54,339`; stored in `RootRecord`,
`contracts/src/SybilSettlement.sol:131`).

Goal of this protocol: **any user (or replacement operator) holding only public L1
state + one DA payload can reconstruct the complete typed exchange state at an
accepted root, verify it independently, and derive their own conservative
withdrawable balance** — the input to the SYB-32 escape claim and the SYB-116
operator handoff.

Non-goals here: the escape-claim ZK program itself (SYB-32), the encryption/
disclosure scheme (SYB-120 — but its interface is fixed in §6), position unwinding
on L1 (rejected previously; positions are recovered by operator replacement, not
forced L1 settlement — `docs/architecture/L1 Settlement and Vault.md` §emergency
escape).

## 2. What v3 already guarantees (the foundation)

Facts, verified in code:

1. `da_commitment = BLAKE3("sybil/da-commitment/v1" || height || state_root ||
   witness_root || payload_root || payload_len || provider_refs_hash)`
   (`crates/sybil-zk/src/lib.rs:449-466`). The guest recomputes it and fails closed
   on mismatch (`lib.rs:524-530`). L1 stores it per accepted root.
2. The payload is the canonical witness bytes (`lib.rs:369-371`), and the v3 witness
   contains: full `pre_state_sidecar` (complete `StateSidecarSnapshot`: bridge,
   markets, groups, resting orders, reservations —
   `crates/sybil-verifier/src/snapshot_schema.rs:244-252`), full `state_sidecar`
   (post), full account sections (`pre_state`, `post_system_state`, `post_state`),
   and the deposit frontier + delta.
3. `state_root` is a qMDB root over typed leaves (`acct/`, `acct_resv/`, `market/`,
   `market_group/`, `order/`, `withdrawal/`, `sys/` —
   `crates/sybil-verifier/src/state_schema.rs`), and
   `compute_state_root_with_sidecar(post_state, state_sidecar)` is pure, public
   Rust that anyone can run.

**Consequence (the design's keystone): one payload = one full state.** Decoding the
witness at height H yields every leaf family; recomputing the root and comparing to
the L1-accepted `state_root` at H proves the reconstruction is exact. There is no
"data set that must be DA-posted" question left to answer — the proven pipeline
already posts it. What is missing is *access*, *retention*, and *procedure*, which
is the rest of this document.

> **Guard rail:** witness-schema Increment 2 (delta/openings witness, deferred in
> `design/witness-schema-v2.md` §7) would destroy the one-payload property. If
> Increment 2 ever lands, this protocol requires reinstating periodic full-snapshot
> DA payloads (every N blocks) as a *separate* DA artifact. This dependency must be
> stated in Increment 2's design gate. A test should pin it (§7, R0 gates).

## 3. The reconstruction procedure (normative)

Inputs: an Ethereum RPC (public), the `SybilSettlement` + `SybilVault` addresses,
and at least one DA payload retrieval path (§5).

1. **Pick the target root.** Read the latest `RootRecord` from `SybilSettlement`
   (or a specific height for audit). Extract `height`, `state_root`,
   `da_commitment`, `verified_at`.
2. **Fetch the payload** for `height` from any retention location (§5). Also fetch
   the DA manifest (payload_len, provider refs).
3. **Verify the binding chain**, fail closed at each step:
   a. `payload_root = BLAKE3("sybil/da/witness-payload/v1" || len || bytes)`
      matches the manifest.
   b. `witness_root = BLAKE3("sybil/witness" || bytes)` recomputed.
   c. `da_commitment` recomputed from parts == the on-chain value. This transitively
      authenticates the payload against the ZK proof that produced the root.
4. **Decode** the canonical witness (version byte must be 3; unknown version =
   abort, no fallback decoding — consistent with the no-dual-decoder policy).
5. **Recompute** `compute_state_root_with_sidecar(post_state, state_sidecar)` and
   require equality with the on-chain `state_root`. At this point the FULL typed
   state at H is locally held and independently proven.
6. **Extract per-user facts:** `acct/{id}` (balance, positions, total_deposited),
   `acct_resv/{id}` (open reservations), pending `withdrawal/` leaves.
   `withdrawable_cash = max(0, balance − open_cash_reservations)` — deliberately
   conservative and identical to the planned escape-guest formula
   (`L1 Settlement and Vault.md:453-471`), so the number a user computes offline is
   the number the escape claim will prove.
7. **(Operator replacement, SYB-116 path)** the same decoded state seeds a fresh
   sequencer: genesis-from-snapshot at H with the deposit frontier restored — the
   store already persists/restores the frontier, so the loader shape exists.

Failure handling:
- **Payload for latest root unavailable** → walk back to the newest height whose
  payload IS retrievable and whose root is accepted. Everything after that height
  is the *at-risk delta*; its size is bounded by the retention SLO (§5). This walk-
  back is legitimate because every payload is a full snapshot.
- **No payload retrievable at all** → the system has failed its availability
  promise; escape-mode activation (already timeout-gated on root staleness,
  `SybilVault.sol:240-259`) is the recourse, and users fall back to their local
  self-insurance snapshot (§4) for the escape claim.

## 4. What a user must store (answer: almost nothing, optionally two small files)

Mandatory: **only their P256 key** (which they hold anyway). Reconstruction and the
future escape claim key off the account leaf, which is recoverable from any payload.

Recommended self-insurance ("custody snapshot", automated by the SDK/CLI in R1):
1. **Own-leaf proof file**: `acct/{id}` + `acct_resv/{id}` leaves + their qMDB
   inclusion proofs against the latest `state_root` (`GET /v1/proofs/state/…`,
   already live) + the `RootRecord` reference. A few KB; verifiable forever against
   L1 with no third party. This is the user's floor if DA fails entirely *and* the
   escape guest accepts direct leaf proofs (it must — §SYB-32 note below).
2. **Latest DA manifest** (not the payload): height, roots, provider refs, ~1 KB.
   Lets the user prove *what* should have been available when disputing
   availability.

The SDK should refresh both on a timer and after every withdrawal/deposit. Users
who skip this lose nothing while DA holds; they lose the *unilateral* escape floor
if DA fails — that tradeoff is stated in user docs, not silently absorbed.

## 5. Retention and where payloads live

Devnet (now, R0): payloads already exist as prover-local files
(`sybil-file://witness/{payload_root}.witness.bin`, `crates/sybil-prover/src/da.rs`).
R0 exposes them through the API (SYB-222: manifest by default, bytes on request,
SYB-139-style caps) — that makes the *sequencer itself* retrieval path #1 and lets
anyone mirror.

Testnet (R2): add one non-operator location — an object store (S3/R2) written by
the prover at publish time, `provider_ref` kind `https`. Retention SLO: **every
accepted root's payload retained ≥ 90 days; latest 1,000 heights always; heights
that are the newest-payload-under-an-accepted-root: forever** (cheap — one payload
per fresh-genesis era suffices for the floor; keep more for audit). Witness
payloads are KB–MB scale at current book sizes; cost is negligible until Increment
2 economics change the calculus.

The vault/settlement deliberately never judge availability (`Data
Availability.md:146-155`) — this design keeps that: availability is enforced
socially/operationally + by the user-held floor (§4), not by L1 consensus. An
availability *challenge* game is explicitly out of scope until there is more than
one operator.

## 6. Interface reservations for SYB-120 (encryption) and SYB-32 (escape claim)

**Encryption hook (do not implement now, do not preclude):** `da_commitment` binds
the PLAINTEXT payload bytes (via `payload_root`) — keep it that way. Encrypted DA
means: ciphertext stored at the provider, `provider_ref` gains kind
`encrypted:{scheme}`, and the manifest adds `ciphertext_hash` + key-custody
metadata. Disclosure on escape activation = revealing the content key; verifiers
then re-run the §3 chain on the decrypted bytes unchanged. This keeps SYB-120
purely additive: no re-hashing, no guest change, no protocol fork. The R0 API
must therefore serve the manifest as a typed object (not a bare byte blob) so new
fields slot in.

**Escape-claim boundary (SYB-32):** the second guest program should accept EITHER
(a) a full payload + account extraction per §3 — the common case, or (b) a bare
qMDB own-leaf proof against an accepted root — the user-floor case from §4. Both
prove the same statement: `withdrawable_cash(acct, acct_resv) = X at accepted root
R`. The vault side is already shaped for it: `claimKind` dispatch exists and fails
closed on non-normal kinds (`SybilVault.sol:180`), escape activation is live, and
the claim entrypoint is the only missing contract piece. Nullifier domain for
escape claims must differ from normal withdrawals (new domain string), and an
escape claim must consume the SAME per-account value space as normal withdrawals
at root R (an account that withdrew normally after R must not double-spend via
escape — the claim binds to the root's leaf values, and the vault tracks a
per-account escape nullifier).

## 7. Implementation increments

- **R0 — Access (SYB-222, next):** witness/DA manifest + payload over the API with
  `witness_root`/`payload_root`/`da_commitment` fields; SDK wrappers for it and for
  `GET /v1/proofs/state/…`; a `verify-reconstruction` test binary that runs §3
  steps 3–5 against a live block and is wired into the E2E smoke (SYB-223).
  **Gate:** a red test that fails if the witness ever stops being a full snapshot
  (pin: decoded leaf families reproduce `state_root` with no external inputs) —
  this is the Increment-2 tripwire from §2.
- **R1 — Custody CLI:** `sybil custody snapshot` (own-leaf proofs + manifest,
  §4) and `sybil custody reconstruct --height H` (full §3, prints the account
  summary + withdrawable_cash). Python thin layer gets the same two verbs.
- **R2 — Off-operator retention:** object-store publisher + retention SLO
  monitoring (alert when newest retrievable payload lags the newest accepted root
  by > N blocks — plugs into SYB-223 alerting).
- **R3 — Escape claim (SYB-32):** the second guest + `escapeClaim` vault entry
  per §6. Separate design addendum once R0/R1 exist to build against.
- **R4 — Encryption (SYB-120):** per §6 hook; needs the key-custody decision
  (operator-held with timelocked disclosure vs threshold escrow) — a product/trust
  decision for Valery, parked until then.

## 8. Open questions for ratification

1. **Retention SLO numbers** (§5: 90 days / 1,000 heights / floor-forever) — fine,
   or pick different constants? They only bind at R2.
2. **Escape-claim dual input** (§6: payload-based AND bare-leaf-proof-based) — the
   bare-leaf path doubles the guest surface but is what makes the user floor real
   with zero DA. I recommend keeping both; confirm.
3. **Availability alerting ownership**: R2 puts "payload lag" on the ops alert
   channel (SYB-223). OK?
4. Anything in §4's "users lose the unilateral floor if they skip snapshots"
   tradeoff you want moved to mandatory (e.g. the frontend auto-downloading the
   snapshot file periodically)?
