# Escape-Claim Guest Program (SYB-32) — Spec Draft

Status: **DRAFT — for ratification** (companion addendum promised in
`design/escape-hatch-reconstruction.md` §6/R3)
Author: Claude (Fable 5), 2026-07-06. Parents:
`design/escape-hatch-reconstruction.md` (SYB-80),
`docs/architecture/Operator Replacement.md` (SYB-116),
`docs/architecture/L1 Settlement and Vault.md` (H14 record).

## 1. Statement to prove

> At accepted root R (height H), account A (identified by its registered P256
> key) had `withdrawable_cash = max(0, balance − open_cash_reservations) = X`,
> and this claim is bound to recipient address D.

Deliberately conservative and cash-only, per the standing rejection of L1
position unwinding: positions are recovered by operator replacement (R-A),
never force-settled on L1.

## 2. Why a SECOND guest program

The state-transition guest proves block transitions; it cannot prove
"account-level fact at rest" without dragging in a whole block's witness and
it pins a different public-input shape. A minimal separate guest keeps the
escape path: (a) tiny and auditable, (b) independent of witness-schema
evolution (it consumes qMDB leaf proofs, not the witness format), (c) provable
by ANYONE on commodity hardware — an escape path that needs the operator's
prover is not an escape path. Its own `app_exe_commit` is pinned in the
adapter/vault alongside the main guest's (both read from a second commit.json;
same fingerprint discipline, `scripts/zk-guest-fingerprint.sh` extended with a
second closure).

## 3. Guest inputs and checks

Private inputs (two accepted forms, per SYB-80 §6 — ratification question):

- **Form P (payload)**: the full DA payload for height H. Guest re-runs the
  §3 binding chain (payload_root → witness_root → da_commitment) and extracts
  `acct/{A}` + `acct_resv/{A}` from the decoded post-state.
- **Form L (leaf proofs)**: qMDB inclusion proofs for `acct/{A}` and
  `acct_resv/{A}` against R (plus an exclusion proof if the reservation leaf
  is absent — absence means zero reservations and MUST be proven, not
  assumed). This is the user-floor path: it works from the two-file custody
  snapshot with zero DA. The qMDB verification code already exists in-guest
  (`verify_qmdb_key_value_proof` etc. in sybil-zk) — reuse, do not fork.

Common checks, fail closed:
1. Key binding: the claimed account id ↔ registered pubkey linkage.
   **VERIFIED GAP (code-checked): `AccountSnapshot` carries id, balance,
   total_deposited, positions, events_digest — NO key commitment
   (`crates/sybil-verifier/src/types.rs:198-207`). The registered-key
   registry lives only in unproven sequencer state. Without a key
   commitment in the `acct/` leaf, NO trustless escape claim is possible —
   the guest cannot bind an account to a signer. A key-registry digest must
   be added to account leaves at the NEXT batched schema move (with
   SYB-224). Ticketed as a hard prerequisite.** The
   claim itself is authorized by a P256 signature over the claim's canonical
   bytes (domain `"sybil/escape-claim/v1"`, includes R, H, A, D, X, and
   `genesis_hash` per SYB-224's domain discipline).
2. `X = max(0, balance − open_cash_reservations)` recomputed in-guest with
   the same overflow-checked integer arithmetic conventions as the main guest.
3. Public inputs out: `{R, H, A, D, X, escape_nullifier}` hashed under domain
   `"sybil/openvm/escape-claim/v1"` (parallel to the withdrawal input hash).

## 4. Nullifier and double-spend rules

`escape_nullifier = keccak256("sybil/escape-nullifier/v1", chain_id, vault,
A, R)` — **one claim per account per root**, deliberately NOT per-amount:
partial escape claims complicate accounting for no benefit (X is already the
maximum safe amount).

Interaction with normal withdrawals (the §6 concern in the reconstruction
doc): an account that finalized a normal withdrawal AFTER root R could
double-recover via an escape claim computed AT R. Rule: the vault tracks, per
account, the sum of normal-withdrawal amounts finalized at roots ≥ the escape
activation reference root, and deducts it from any escape payout (floor 0).
Simpler alternative if that bookkeeping is unwelcome: escape claims only
accepted against roots ≥ the newest root at escape activation — i.e. claims
must use the freshest accepted state, which post-dates any finalized
withdrawal already reflected in balances. **Recommendation: the simpler rule**
(claims bind to the newest accepted root only); it needs zero new vault
bookkeeping and the freshest root is exactly what escape mode certifies as
stale-but-final. Ratification question.

## 5. Vault entrypoint

`escapeClaim(EscapeClaimPublicInputs inputs, bytes proof)`:
- requires `escapeModeActive` (already deployed) and NOT paused-for-escape
  (pause must not block escape — escape exists for when governance is the
  problem; **pause() therefore must NOT gate escapeClaim** — needs a carve-out
  from the SYB-96 single `paused` flag: escape claims check only
  `escapeModeActive`). Flag this in the contract change.
- `claimKind = keccak256("sybil/claim-kind/escape-cash/v1")` — reinstates the
  removed constant; the existing fail-closed dispatch gets a second arm.
- verifies the escape-guest proof against the SECOND pinned commitment,
  checks `R` per §4's freshness rule, consumes `escape_nullifier`, pays `X`
  to D immediately — **no withdrawal delay in escape mode** (the delay exists
  to let the operator react; in escape mode the operator is the failure).
  Ratification question: confirm no-delay.

## 6. Implementation plan (post-ratification)

1. Guest crate `sybil-escape-guest` (own openvm.toml, own commit.json, own
   fingerprint closure entry). Form L first — it is smaller and unblocks the
   user floor; Form P second (shares the reconstruction library with the
   custody CLI, R1).
2. Vault: `escapeClaim` + second verifier pin + claim-kind arm + pause
   carve-out; forge tests incl. golden vectors for the escape input hash
   (extend OL-4 pattern).
3. `sybil custody escape-claim` CLI verb (R1 tool grows the prover call).
4. Drill: testnet exercise — activate escape on a throwaway deployment, run a
   real claim end-to-end. Rides SYB-223's drill culture.

## 7. Ratification questions (Valery)

1. Both input forms (P + L), or L only? (Doc recommends both; L-only halves
   the surface but makes DA the single point of failure again.)
2. Freshness rule for R: newest-accepted-root-only (recommended, zero
   bookkeeping) vs deduction bookkeeping?
3. Confirm: escape claims bypass the withdrawal delay and the pause flag.
4. The key-binding open item in §3.1 — if keys aren't in the account leaf,
   approve adding the key-registry commitment to `acct/` leaves at the next
   batched schema move (with SYB-224).
