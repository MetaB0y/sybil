---
tags: [sybil, roadmap, backlog-triage]
status: current (reviewed by orchestrator 2026-07-10)
date: 2026-07-10
supersedes: the 2026-07-10 morning roadmap (session artifact, archived in ~/sybil-handoff-artifacts-2026-07-10/)
sources: full Linear sweep (76 open → 71 after flips), repo @ main d2b987d5, ADR set 0001–0015
---

# Sybil roadmap — 2026-07-10 evening (post-landing-wave)

## 1. Current state

Main moved `060dec4f` → `d2b987d5` today: OpenVM 2.0.0 (SWIRL) with path-independent guest builds (canonical pin `app_exe_commit 0x000f896e…`), fresh-genesis devnet redeploy (genesis `e16c7655…`, healthy), the whole SYB-252 money-path punch-list landed (12/13 children), golden-vector single source, compose integration harness + deterministic seeder, mobile frontend deployed, backup/restore drill + synthetic monitoring live, restart-resilience gate. The 2026-07 review debt is nearly retired: of the SYB-269 adversarial findings only ZK-1 (270), L1-1 (272) — both mid-lane — and API-2 (275, rides 237) remain. Tonight's triage flipped five stale-but-landed tickets Done (225, 216, 28, 29, 76): the M4 "ZK settlement" milestone and the witness-schema/DA design arcs are, in substance, finished. Open count is now ~71, of which ~40 are deliberately parked strategic tracks (TEE, Sepolia, selective-reveal proofs, sponsors, M6 growth). The near-term game: finish the in-flight soundness/security lanes, then pivot from "fix the reviewed" to the two structural arcs the ADRs already ratified — escape-claim implementation (custody) and encrypted DA (privacy) — while keeping the devnet demo sharp for the Polymarket-refugee-quant wedge.

## 2. In flight (landing today/tomorrow — do not re-plan)

- **SYB-270 witness-v6** — witnessed key events + guest keys_digest transition constraint; wire v6; fresh genesis + fingerprint refresh at end of session. Closes the last HIGH soundness finding.
- **SYB-237 Phase 1b** read-auth (option A ratified) + **SYB-271** atomic create-with-initial-key (271 already flipped Done; 1b in lane).
- **SYB-272 deposit-quarantine** (option 1 ratified; ADR-0015 already committed `b3041997`; implementation in lane).
- **Dependency-update pass** — queued behind the heavy lanes.
- After 270+272 land: close SYB-269 umbrella (only 275 residual remains, re-scoped per comment).

## 3. Prioritized lanes — next ~2 weeks

Ordering reflects: soundness > custody/escape > privacy > product polish > ops. Demo-for-recruiting bumps a few product items into the tail.

| # | Lane | Size | Depends on | Who |
|---|------|------|-----------|-----|
| 1 | **Post-v6 verification + closeout** — verify fresh genesis, fingerprint refresh, golden vectors extended to keys_digest transitions; close SYB-269/270/272; refresh SYB-218 description (stale v3 commitments) | S | in-flight lanes land | autonomous |
| 2 | **Soundness guardrails while CI is off** — SYB-233 re-scoped: local from-source guest rebuild-and-compare gate as a `just` target wired into the land ritual (path-filtered); plus SYB-246 economic property catalog (value conservation incl. bridge refunds, no-arb, crossing-book non-triviality, order-independence; resolve the `#[ignore]`d `iter_lp_solver_conformance`) | M | none | autonomous |
| 3 | **Money-path residue** — SYB-265 items 3–4 (reserved_balance restore validation, release_reservations tripwire); then close SYB-265 → SYB-252 umbrella | S | none | autonomous |
| 4 | **Custody arc opener: escape-claim guest (SYB-32)** — keys_digest is now committed (225 Done) and, after v6, transition-constrained; `design/escape-claim-guest.md` + ADR-0013 valuation rule are ratified. Write the implementation brief, then staged implementation (guest + vault escape mode + claim flow). This is the single biggest credibility item for real-money custody | L | lane 1 (v6 landed) | autonomous (design ratified) |
| 5 | **Privacy arc: SYB-237 residual + encrypted-DA increments (SYB-120)** — after 1b lands: per-endpoint decision on remaining structured reads (SYB-275 residual — public fill history deanonymization; coordinate arena); start ADR-0012 implementation in landable increments: view-key derivation from passkey-PRF, per-account HPKE blob writer behind a flag, blinded-slot layout | L | 237 1b landed | autonomous (ADR ratified); staged |
| 6 | **SYB-119 SWIRL-era aggregation note** — recursive vs monolithic batch proofs re-evaluated under OpenVM 2.0.0; output = settlement cadence recommendation + what hardware SYB-126 actually needs now; sequences SYB-87/95/126 | M | none | autonomous (research) |
| 7 | **SYB-88 proving benchmark suite** — 100/1k/10k orders per block on SWIRL; produces the recruiting-pitch numbers ("10k trades proven in X") and a proving-time regression guard | M | lane 6 helps | autonomous |
| 8 | **Demo polish: SYB-101 residual** — /arena consumer-grade restyle (still 1:1 `dev/primitives` operator tables); Lighthouse + physical-device pass stay with Valery | M | none | autonomous (restyle part) |
| 9 | **SYB-238 close-out** — measure + document native build baseline on the next routine deploy, then flip Done | S | next deploy | autonomous |
| 10 | **Demo liveliness: SYB-44 (auto market creation)** — the devnet demo goes stale without fresh markets; even a curated-cadence version (Poly mirror + trending) keeps it alive for visitors | M/L | none | autonomous, needs Valery taste-check on catalog policy |
| 11 | **Recruiting surface (Valery-gated go): SYB-36 landing page + SYB-35 copy-trade simulator spec** — the wedge story ("your alpha stays yours") aimed at Polymarket-refugee quants; landing repo is separate (`sybil-landing`) | M | Valery go on messaging/timing | mixed |
| 12 | **SYB-114 bot-quality research** — read terminator2-agent, draft concrete arena changes; implementation waits for calibration windows (Valery runs `calibration.py` after ~a day of post-genesis trading) | M | calibration data | autonomous (research half) |

Notes on sequencing: lanes 2–3 are immediate fillers behind the in-flight heavies; lanes 4 and 5 are the two structural arcs and should each own a workspace once 270/272 free them; 6–7 are Fable-research-shaped and parallelize freely; 8–10 keep the demo credible while the heavy arcs run.

## 4. Valery decision queue (each ≤5 min unless noted)

1. **Linear workspace** — at the free-issue cap; tonight's triage freed 5 slots by closing, but filing is still effectively blocked. Upgrade one seat (~$8/mo) or bulk-archive pre-M2 issues. (Standing from morning roadmap; still open.)
2. **GitHub Actions billing** — $10–20 cap re-enables CI → unblocks the CI half of SYB-248 and makes SYB-233's gate a required check instead of a ritual. Local gates cover us meanwhile.
3. **SYB-56 / SYB-101 device pass** — iOS/Android/macOS/Windows passkey journey + Lighthouse mobile run on the deployed origin; flips SYB-56 (and most of 101) Done. Needs his hands, ~30 min.
4. **Telegram alert live-fire test** (optional) — synthetic monitoring is live; one deliberate failure confirms the pager path.
5. **SYB-44 catalog policy** (when lane 10 starts) — auto-created markets: fully automatic vs curated-approve queue.
6. **Landing page go** (lane 11) — messaging + timing for the recruiting push; blocked only on his call, not on engineering.
7. **Sepolia go/no-go** stays parked by his standing decision (funded deployer key + RPC when ready).

## 5. Parked / later — with unpark conditions

- **TEE track** (SYB-25, 26, 43, 77, 78, 82, 83, 84, 85): standing "not now". Unpark: AWS account + Nitro instance decision. Note ADR-0012/0013 reduced how much of the trust story depends on TEE.
- **Sepolia / real L1** (SYB-95, 31, 30): unpark on funded Sepolia key. SYB-30's real on-chain verification additionally needs the Halo2/EVM artifact path (SYB-126).
- **SYB-126 proving box** (~256 GB RAM): unpark when lane 6 (SWIRL note) says what's actually required now — SWIRL may have changed the wrapper-path economics; do the research before renting hardware.
- **SYB-87 historical backfill**: unpark once settlement cadence is decided (lane 6 output) and prover is stable on v6.
- **SYB-111 verifier-side STP**: explicit "consider" ticket; the full-snapshot witness (ADR-0006) has made the fix much cheaper than when filed, but the reopen triggers (trust-model change, actionable rejections) haven't fired. Revisit inside the escape/custody review (lane 4).
- **SYB-105 order-book persistence at scale**: unpark on >5k resting orders or redb I/O dominance; cheap interim = add redb I/O to synthetic monitoring (idea #6).
- **Selective-reveal proofs** (SYB-79, 88→active, 89–94): after privacy arc; note this cluster is the long-term differentiator for the quant audience — good next-quarter arc after encrypted DA.
- **Opportunity markets** (SYB-67–73): post-shakedown, needs product pull.
- **Agents foundation** (SYB-61, 62, 65, 66): post-shakedown; arguably the second wedge (quants configuring server-side agents) — candidate to unpark right after the custody/privacy arcs.
- **Growth/M6** (SYB-39, 57, 98–100, 102–104, 47, 49): post-open-doors; SYB-102 (token doc) is Valery's pen.
- **Content** (SYB-16, 37): anytime; pairs with lane 11 recruiting push.
- **SYB-218**: not parked — living checklist, needs a description refresh after v6 genesis (stale v3 commitments/witness references).

## 6. New-ticket-worthy items found during triage (cannot file — issue cap; record here)

1. **Stale trust TODO in l1-indexer**: `crates/sybil-l1-indexer/src/main.rs:662` — `TODO(SYB-188/SYB-178): this dev indexer trusts eth_getLogs from its RPC`; both referenced tickets are closed. Either file a real "verify RPC responses / multi-provider cross-check" ticket for the Sepolia era or delete the stale TODO so grep-for-TODO stays honest.
2. **`/tmp/sybil-*` default paths in justfile**: `witgen-smoke-job` defaults to `store="/tmp/sybil-smoke.redb" job="/tmp/sybil-proof-job.msgpack"` (justfile:232). Shared-box policy says never create `/tmp` files named `sybil-*` (collision/permission incident class). Move defaults to `$XDG_CACHE_HOME` or the repo `target/` dir. One-line fix.
3. **Canonical witness-version constant**: there is no greppable `WITNESS_VERSION` symbol; version knowledge lives in ADRs/docs and encoding code. Add a single canonical schema-version constant in `sybil-verifier` (cited by guest + docs) so version audits are mechanical — matters more now that v-bumps are routine (v3→v6 in one week).
4. **Proof-lag probe in synthetic monitoring**: monitoring covers core flows but nothing asserts `GET /proofs/latest` freshness vs block height. A wedged prover (the openvm pk bitcode-error class from this morning) would be invisible until someone looks. Add "proof height within N of block height" to the probe.
5. **Bridge-inclusive value-conservation property**: SYB-246's catalog should explicitly include deposits + withdrawal refunds (new since SYB-253) in the conservation sum — the refund path is exactly where conservation bugs would now hide. (Fold into lane 2 scope rather than a new ticket if timing works.)
6. **redb I/O measurement hook** (SYB-105 interim): emit redb write-bytes/block as a metric so the SYB-105 "when we hit the wall" trigger is observable instead of vibes.
7. **Quarantine UX follow-up** (after SYB-272 lands): frontend/docs surface for "deposit quarantined — register this key to claim" + L1 refund path instructions; also add the quarantine flow to the post-deploy smoke.
8. **Golden vectors for keys_digest transitions** (after v6): extend the single-source golden-vector generator (SYB-234/249 machinery) with key-event/keys_digest cases so Solidity/Rust parity covers the new constraint surface. (Belongs to lane 1.)
9. **Devnet verification-depth honesty note**: document exactly what the devnet L1 adapter verifies today (`contracts/src/OpenVmVerifierAdapter.sol` + `contracts/src/dev/`) vs what mainnet-grade verification requires (Halo2 wrapper, SYB-126) — one page in docs/architecture, so demo claims to recruits stay precise.
10. **SYB-80 merge**: escape-hatch *data-reconstruction* ticket is design-complete (`design/escape-hatch-reconstruction.md`, ADR-0005/0006/0013); its implementation remainder duplicates SYB-116 (operator replacement) + SYB-32 (escape claim). When a slot frees, merge 80 into 116 and close it.

*Added during the evening landing wave (statuses: #2 fixed `78987a21`; #8 done in the v6 landing `9f578dbf`):*

11. **commonware 2026.5 migration**: the deps pass (`f693bdc7`) pinned all nine commonware crates back to 2026.4.0 — 2026.5's authenticated-storage rework (journaled API removal, explicit parallel strategies, changed proof hashers, removed range-proof fields) crosses our qMDB proof boundary. This is a real M-sized migration with soundness review, not a bump; schedule it deliberately before the pin rots (upstream moves fast, and staying ≥2 releases behind on the storage layer we settle on is its own risk).
12. **WAL-atomic account creation**: `POST /v1/accounts` (atomic create-with-initial-key, `2461f317`) is API-atomic via a bootstrap mutex but persists as two sequencer/WAL operations; a crash between them strands an inert, unclaimable account (no security hole — bootstrap is service-only). Small matching-sequencer follow-up: a combined create-and-register actor command, single WAL entry.
