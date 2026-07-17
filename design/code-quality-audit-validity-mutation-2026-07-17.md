---
tags: [audit, code-quality, testing, verification, zk, mutation]
layer: verification
status: current-audit
date: 2026-07-17
last_verified: 2026-07-17
---

# Validity-core mutation audit

Date: 2026-07-17  
Cluster: test-oracle effectiveness in validity-critical Rust  
Tooling: `cargo-mutants 27.1.0`, `cargo-nextest 0.9.140`

## Verdict

The targeted validity-core oracle is materially stronger after this audit.
Across the final campaigns, 235 generated mutants were classified:

- 220 were caught by tests;
- 7 were reviewed as behaviorally equivalent in the supported domain; and
- 8 were unviable return-value replacements that did not compile.

There are no unexplained survivors in the audited functions.

The audit found one concrete fail-open validity defect: a supported non-zero
fill could omit its market or outcome clearing-price entry and escape the
uniform-clearing-price check. It now produces a
`UniformClearingPriceViolation`. It also found a settlement defect on the
research/general-payoff path: a one-outcome payoff magnitude such as `+2` or
`-3` used the binary fast path but changed position by only one unit per fill.
The fast path now preserves payoff magnitude.

The largest result was not a score. Several guest-validity modules had almost
no direct negative oracle:

- 39 of 49 initial match-core mutants survived;
- 16 of 18 client-action binding mutants survived;
- 12 of 26 quarantine replay mutants survived;
- 15 of 48 settlement-verifier mutants survived; and
- 10 of 18 account-key commitment/encoding mutants survived.

Focused boundary and state-transition tests closed every meaningful gap in
those campaigns. The changed verifier was rebuilt into both affected OpenVM
guest closures. The main executable commitment moved; the escape commitment
reproduced unchanged. Repository validity pins now say `pending_redeploy` and
the validity boundary is explicitly `fresh_genesis`. No live deployment,
adapter update, or state reset was performed.

Two broader policy problems remain open:

- [GitHub #178](https://github.com/MetaB0y/sybil/issues/178) — separate
  consensus validity from diagnostic-quality policy; and
- [GitHub #179](https://github.com/MetaB0y/sybil/issues/179) — implement
  explicit action-domain prefixes for bare-Borsh signing builders and remove
  or classify duplicate key-operation shapes.

## Evidence boundary

This was a bounded mutation campaign, not a claim about the whole workspace.
The selected functions are deterministic, guest-safe, and directly involved
in transition acceptance, settlement, canonical commitments, or binding an
authorization to its effect:

- `crates/sybil-verifier/src/arithmetic.rs`
- match-core checks in `match_verifier.rs`
- `client_action.rs`
- `quarantine.rs`
- settlement derivation/comparison in `settlement.rs`
- account-key digests and canonical key-operation bytes in `account_keys.rs`
- `crates/matching-engine/src/settlement.rs`
- `crates/sybil-signing/src/lib.rs`

The campaign deliberately did not mutate all solver algorithms, storage
backends, API routes, generated clients, OpenVM dependencies, or every verifier
helper. Those belong to other audit clusters. No survivor was accepted merely
because a mutation score looked high: each was traced through the supported
input domain and classified.

## Architecture context read

The review used the root and focused crate guidance plus:

- `Four-Layer Verification`
- `Block Witness`
- `Canonical Serialization`
- `State Root Schema`
- `ZK Integration Path`
- `P256 Authentication`
- ADR-0007, domain-separated canonical bytes
- `Payoff Vectors`
- `Binary Markets and Market Groups`
- `Nanos and Integer Arithmetic`
- `Order Types`
- `Settlement`
- `L1 Settlement and Vault`

Important constraints carried into the review were:

- integer protocol truth;
- no separate lenient consensus mode;
- one native/guest-safe validity definition;
- fixed canonical field ordering and domains;
- deterministic checked settlement arithmetic; and
- deliberate guest commitment and fresh-genesis handling for validity changes.

## Research basis and method

Mutation testing was used as a meta-test: it asks whether the existing oracle
notices a small code change. It does not establish correctness by itself.

The method followed the maintained cargo-mutants guidance:

- start from a passing
  [unmutated baseline](https://mutants.rs/baseline.html);
- use explicit file/function
  [filters](https://mutants.rs/filter_mutants.html) rather than an unbounded
  workspace run;
- use a reviewed timeout rather than treating every timeout as a missed
  assertion ([timeout guidance](https://mutants.rs/timeouts.html)); and
- classify equivalent, unviable, and uncovered mutations instead of optimizing
  a percentage.

The broader review method remained repository-aware and validator-backed.
[RepoAudit](https://arxiv.org/abs/2501.18160) supports tracing findings through
repository context and validating feasible paths. The exact-wire cluster used
schema and cross-runtime oracles; this cluster used mutation because the
question was specifically whether validity tests reject defects.

Campaign procedure:

1. Run the unmutated package tests and a small arithmetic calibration.
2. Generate mutants for one pure function group at a time.
3. Inspect every survivor against admission invariants and actual call paths.
4. Add the narrowest negative, boundary, or known-layout oracle for meaningful
   survivors.
5. Refactor predicates only where the existing expression concealed protocol
   structure or contained a real semantic bug.
6. Rerun the same mutant subset and retain a survivor ledger.
7. Run default/no-default/feature, guest, lint, commitment, and consensus gates.

All cargo-mutants output directories were placed under `/tmp` after the
calibration runs. Generated `mutants.out` directories were removed from the
working copy.

## Campaign results

| Target | Initial signal | Final classification |
|---|---:|---:|
| Verifier checked arithmetic | Calibration already strong | 23 caught |
| Match validity: UCP, resolution, conditions, complementarity | 39 missed, 8 caught, 2 unviable | 25 caught |
| Engine settlement and minting | 24 misses across the first production/generic probes | 44 caught, 3 equivalent, 3 unviable |
| Client-action effect binding | 16 missed, 2 unviable | 13 caught, 3 equivalent, 2 unviable |
| `sybil-signing` canonical builders | No live survivor | 27 caught |
| Deposit-quarantine commitment/replay | 12 missed, 12 caught, 2 unviable | 24 caught, 2 unviable |
| Verifier settlement derivation/comparison | 15 missed, 32 caught, 1 unviable | 46 caught, 1 equivalent, 1 unviable |
| Account-key digest/key-operation bytes | 10 missed, 8 caught | 18 caught |
| **Total final set** |  | **220 caught, 7 equivalent, 8 unviable** |

The final sets are not always identical to the initial generated sets because
predicate refactoring removed redundant mutant sites and new code created
different sites. Counts are therefore evidence about each bounded campaign,
not a before/after percentage over one frozen mutation list.

## Survivor ledger

Every final live survivor was reviewed:

| Target | Surviving mutation | Classification |
|---|---|---|
| Engine settlement | Delete the positive-YES fast-path arm | Equivalent: the generic single-market path computes the same debit and scaled position. |
| Engine settlement | Delete the positive-NO fast-path arm | Equivalent for the same reason. |
| Engine minting | Change `diff > 0` to `diff >= 0` | Equivalent: `diff == 0` is continued immediately before the branch. |
| Client-action order cancellation scan | Change `index + 1` to `index * 1` | Equivalent: the wider slice adds only the current `ClientActionAuthorized` event, which cannot be `OrderCancelled`. |
| Client-action order resolution scan | Change `index + 1` to `index * 1` | Equivalent: the current authorization event cannot be `MarketResolved`. |
| Client-action cancel-effect scan | Change `index + 1` to `index * 1` | Equivalent: the current authorization event cannot be its own cancellation effect. |
| Verifier settlement | Invert the branch preferring a non-zero `fill.account_id` | Equivalent for valid fills because the fill and witness-order account IDs must be equal; mismatches already create a validity violation. |

The eight unviable mutants replaced functions with values that do not satisfy
their Rust return types or required trait bounds. They are tool classifications,
not test successes.

## Findings and disposition

| ID | Severity | Finding | Disposition |
|---|---|---|---|
| VM-1 | High | Uniform-clearing-price verification failed open when the market or expected outcome price was absent. | Fixed; missing entries now violate UCP. |
| VM-2 | High | UCP side combinations, resolved-market fills, exclusive condition boundaries, and missing condition prices had no direct negative tests. | Fixed with table/boundary tests; final match set 25/25 caught. |
| VM-3 | Medium | Verifier tests did not compile without default qMDB features because fixtures called a qMDB-only event-root helper. | Fixed with a feature-aware test helper; default and no-default tests pass. |
| VM-4 | Medium | The engine single-market fast path discarded payoff magnitude outside ±1. | Fixed; checked position deltas preserve `i8` magnitude. |
| VM-5 | High | Client-action bindings had no direct tests, allowing deletion of the entire binding pass and most exact-effect predicates. | Fixed with accepted/rejected/resting, duplicate, account/order mismatch, later-event, and cancellation tests. |
| VM-6 | High | Quarantine replay had no local tests, allowing deposit/claim handling, exact amount/key checks, and post-ledger comparison to be deleted. | Fixed; final campaign 24 caught, 2 unviable. |
| VM-7 | High | Settlement-verifier boundaries did not pin one-dollar validity, greater-than-one rejection, zero validity, missing accounts/positions, or account statistics. | Fixed with focused transition tests; one equivalent survivor remains. |
| VM-8 | High | Validity key-operation encoders were only exercised symmetrically by signing and verification, so constant replacement survived. | Fixed with independent domain/layout/field-sensitivity tests; final 18/18 caught. |
| VM-9 | High | A `diagnostics` flag currently changes validity for zero fills and market-group constraints, conflicting with the focused crate guidance. | Open in #178; requires one explicit validity policy and coordinated guest handling. |
| VM-10 | High | ADR-0007 requires explicit action separation, while several `sybil-signing` builders are bare Borsh and duplicate key-operation shapes exist. | Open in #179; coordinated Rust/Python/TypeScript/guest migration required. |

### VM-1 — missing clearing price failed open

The previous UCP path used `continue` when either the market clearing-price
vector or expected outcome entry was absent. A supported non-zero fill could
therefore evade the equality check by deleting the oracle value it was meant
to match.

The check now:

- recognizes the four admitted one-hot sides;
- requires the market price vector;
- requires the filled outcome entry; and
- compares the fill against that exact entry.

Malformed/general payoff vectors remain the order-validation layer's
responsibility. Tests cover buy/sell YES/NO, every mismatch, and missing
market/outcome entries.

### VM-2 — resolution and condition truth tables

Initial mutation results showed that deleting resolved-market rejection or
changing conditional boundaries did not fail a test. The new oracle pins:

- fills on resolved markets are invalid;
- `Above` means strictly greater than, and equality is inactive;
- `Below` means strictly less than, and equality is inactive;
- both directions reject the opposite side of the threshold; and
- a missing condition-market price is not treated as activation.

### VM-3 — feature-aware verifier fixtures

`cargo test -p sybil-verifier --no-default-features --lib --no-run` failed
because three test modules called the qMDB-backed event-root function. A
`cfg(test)` helper now returns the real qMDB root when enabled and the neutral
test root otherwise. This changes no production verifier behavior.

### VM-4 — payoff magnitude in settlement

The fast path selected any positive or negative one-hot payoff but always
applied `±fill_qty`. For research/general payoff vectors, `[2, 0]` should move
two position units per fill unit and `[-3, 0]` should move negative three.

The path now classifies outcome and buy/sell direction while applying the
checked signed payoff magnitude. Tests cover positive/negative YES/NO,
single-market mixed payoffs, both outcomes of two-market marginal settlement,
and checked minting overflows.

### VM-5 — exact action/effect binding

Tests now distinguish:

- accepted, rejected, and post-sidecar resting effects;
- exact account and exact order equality;
- duplicate result and duplicate authorization rejection;
- authenticated pre-existing resting orders;
- cancellation/resolution only after authorization; and
- exact later cancellation account/order identity.

The three surviving slice mutations are equivalent because including the
current authorization event cannot manufacture the required later event.

### VM-6 — quarantine replay

Tests independently pin:

- order-independent, field-sensitive ledger digesting;
- positive unique pre/post entries;
- positive checked accumulation;
- account-derived bridge keys;
- exact amount, presence, and double-claim behavior;
- deposit and claim replay; and
- exact post-ledger equality, including genesis emptiness.

### VM-7 — settlement comparison edges

The added tests cover protocol boundary values and asymmetric omissions:

- exactly `NANOS_PER_DOLLAR` is accepted;
- larger order limits and fill prices are rejected;
- zero balance and zero position are not negative;
- a claimed-only position is rejected;
- a derived-only position is rejected;
- an omitted post account is rejected for either non-zero balance or position;
- `accounts_checked` is reported; and
- missing clearing-price diagnostics name the correct outcome.

### VM-8 — independent key-operation layout oracle

End-to-end signing tests can be symmetric: if both signer and verifier call the
same broken byte builder, a constant-return mutation may still pass. The new
test independently checks the register/revoke domain, exact offsets and
little-endian fields, total length, and sensitivity to genesis, account, auth
scheme, public key, capability mask, bound key digest, and bound event digest.

## Implemented changes

- Made UCP reject missing market/outcome price entries.
- Added UCP, resolution, condition, action-binding, quarantine, settlement,
  account-key, and overflow regression tests.
- Added a feature-aware verifier test event-root helper.
- Preserved general single-outcome payoff magnitude in engine settlement.
- Simplified structurally non-zero multi-market marginal count predicates.
- Added independent canonical key-operation byte-layout checks.
- Rebuilt and repinned the main and escape OpenVM guest closures.
- Updated desired validity pins, generated protocol pins, and the explicit
  fresh-genesis validity boundary.

No live deployment, contract call, adapter rotation, state reset, or issue
closure was performed.

## Guest commitment and deployment boundary

The main state-transition executable commitment changed:

```text
old 0x007ed68957b0503461be92e3e35ca029819054b2742f815bce641f9664c5fd86
new 0x0078357d8232fb0ded2c529dc8b920e78a12a965ae46b5a4c93d9d03e8658210
```

The VM commitment remains:

```text
0x006185384dcac8a449ebcad26ce224c07145ad440e4739b237439a4318d3cd9d
```

The Form-L escape guest was rebuilt because shared verifier/engine source is in
its fingerprint closure. Its executable commitment reproduced unchanged:

```text
0x008c8f972e57f4b15163aa7bb8d5ca89c5532212c091bb85c1c19a1428a53ffd
```

`deploy/validity-pins.json` deliberately records `pending_redeploy`.
`deploy/validity-boundary.json` binds the new artifacts to `fresh_genesis` with
the reason “Fix fail-open clearing-price verification and strengthen
validity-core transition checks.” This is a repository deployment guardrail,
not evidence that a fresh genesis or adapter repin has happened.

## Verification

All final local gates below passed on 2026-07-17:

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | Pass |
| `cargo test -p matching-engine` | 67 unit tests plus doctests pass |
| `cargo test -p sybil-signing` | 14 tests pass |
| `cargo test -p sybil-verifier` | 149 tests pass |
| `cargo test -p sybil-verifier --no-default-features --lib` | 121 tests pass |
| `cargo hack check -p sybil-verifier --each-feature --no-dev-deps` | All four configurations pass |
| `cargo test -p sybil-zk` | 40 tests pass |
| Focused Clippy with `-D warnings` for engine/signing/verifier/zk | Pass |
| `just zk-rebuild-check` | Both guests rebuild from source and reproduce pins |
| `just check-consensus` | Goldens, fingerprints, desired pins, boundary, and protocol pins pass |

The exact-wire cluster's Rust, Arena, frontend, OpenAPI, and documentation
gates were also already green before this mutation cluster began.

## Open work and residual risk

1. Resolve #178 before treating diagnostic mode as settled consensus policy.
2. Resolve #179 through a coordinated canonical-signing migration; do not
   update only one client or the guest.
3. The new main guest commitment is not deployed. A future deployment must
   follow the fresh-genesis runbook, update the real verifier adapter, and
   produce deployment evidence.
4. Mutation operators do not generate every semantic defect. Stateful economic
   invariants, conservation/solvency, actor lifecycle, persistence recovery,
   API sequences, unsafe/panic policy, dependencies, performance, Arena,
   frontend accessibility, and Solidity differential semantics remain separate
   clusters in the program ledger.
5. Multi-market/general payoff settlement remains research-only and documents
   integer truncation. Production admission still requires one binary market
   with one ±1 payoff entry.

## Completion criteria

This cluster is complete when:

- every generated mutant in the stated function set is caught, unviable, or
  documented as equivalent;
- every accepted survivor gap has a regression test or bounded code fix;
- default, no-default, feature, lint, guest, and consensus gates pass;
- guest artifacts and the validity boundary are deliberately synchronized; and
- broader policy questions are present in GitHub Issues and Project 1.

Those criteria are met. Deployment remains intentionally pending and is not
part of a code-audit completion claim.
