---
tags: [zk, settlement, aggregation, hardware, syb-119, syb-126, openvm]
status: proposal-needs-revalidation
date: 2026-07-10
ticket: SYB-119
author: research lane (Fable), 2026-07-10 evening
sources: OpenVM v2.0.0 docs (tag v2.0.0), blog.openvm.dev, SWIRL paper, repo @ 2793d35c
---

# SYB-119: Aggregation vs monolithic settlement proofs under OpenVM 2.0.0 (SWIRL)

> **Status note (2026-07-11):** this is a dated research recommendation, not a
> statement that an epoch guest or aggregation pipeline exists in `main`.

**TL;DR.** SWIRL did change the economics, but not where we expected. The EVM leg is still
a Halo2/KZG wrapper (~316K gas to verify on-chain), and per-block L1 submission is still
dead on gas alone. What changed: (1) the **~256GB keygen box is dead** — v2.0.0 documents
~16 GB for `cargo openvm setup --evm`, and the Halo2 proving key + Solidity verifier are
**downloadable prebuilt** (`--download`, or the openvm-solidity-sdk repo), so no keygen-class
hardware is needed at all; (2) aggregation keys collapsed from ~16 GB (`agg.pk`, 1.x) to
~91 KB (`internal_recursive.pk`, measured on our box); (3) 2.0 adds a `verify-stark`
deferral path for in-guest recursive aggregation — real, but SDK-only (no CLI) and the
wrong tool for us today. **Recommendation: monolithic epoch guest** — one guest execution
verifies N consecutive block witnesses with chaining, continuations handle length, one
EVM proof + one `submitStateRoot` per **1-hour epoch** (360 blocks). Hardware: a 64 GB
CPU box, not 256 GB.

---

## 1. What SWIRL actually changes (documented)

### The proof system

SWIRL ("Stacked WHIR with Interaction Reductions via LogUp") is a new multilinear STARK
built on the WHIR polynomial commitment scheme, replacing the 1.x FRI/BabyBear-era
backend. From the paper's abstract: it stacks "multivariate polynomials over different
domains together prior to invoking any polynomial commitment scheme," uses LogUp-GKR for
interactions, and "yields a SNARK with fast verification, compact proofs, and provable
post-quantum security" — explicitly designed so that low verification cost "leads to
highly performant recursive proof aggregation" ([SWIRL paper](https://openvm.dev/swirl.pdf),
§1). 100-bit provable security, no trusted setup at the STARK level
([blog.openvm.dev/2.0](https://blog.openvm.dev/2.0)).

Headline performance (documented, GPU-centric): 11.4 MHz RISC-V proving on a single
RTX 5090, 139 MHz on 16×5090, proof sizes under 300 kB, Ethereum mainnet blocks proven
at p99 11.8 s on the 16-GPU cluster ([blog.openvm.dev/2.0](https://blog.openvm.dev/2.0),
[axiom.xyz/blog/openvm-2](https://www.axiom.xyz/blog/openvm-2)). No public CPU-only
throughput numbers for 2.0 (**gap — SYB-88 measures this for our guest**).
STARK Backend v2.0.0 is the production-recommended line and has an external audit
(zkSecurity) per the [stark-backend README](https://github.com/openvm-org/stark-backend).

### The pipeline: same shape, new internals

The [v2.0.0 continuations spec](https://docs.openvm.dev/specs/architecture/continuations)
keeps the layered aggregation tree, now as SWIRL circuits:

```
app (per-segment proofs) → leaf → internal-for-leaf → internal-recursive (repeat, VK-converged)
  → root → static (Halo2 wrapper) → EVM proof
```

- **App/STARK/EVM client-facing proofs** are unchanged in role: `ContinuationVmProof`
  (per-segment), `VmStarkProof` (one aggregated proof, ~large, off-chain verify),
  `EvmProof` (constant-size, on-chain verify).
- **VK convergence at internal-recursive** enables unbounded recursion — an
  internal-recursive proof verifies under its own circuit; "for fixed internal system
  parameters, the internal-recursive circuit is the same for any `app_vk`" (spec). This is
  why the heavy aggregation keys are now app-independent and shipped/downloadable.
- **The EVM leg is still Halo2/KZG.** The static layer re-encodes the root proof for a
  fixed on-chain verifier; `cargo openvm setup --evm` "downloads Halo2 parameters, and
  generates or downloads the Halo2 proving key and Solidity verifier artifacts"
  ([v2.0.0 generating-proofs docs](https://docs.openvm.dev/book/writing-apps/generating-proofs/)).
  On-chain verification costs **~316K gas** "via a Halo2-based recursive proof"
  ([blog.openvm.dev/2.0-beta](https://blog.openvm.dev/2.0-beta)). The EVM proof payload is
  small: `proof_data` = 12×32 B accumulator + 43×32 B proof = 1,760 B, plus 32 B public
  values and two 32 B commits ([verifying-proofs docs](https://docs.openvm.dev/book/writing-apps/verifying-proofs/)).
  So the ~300 kB SWIRL proofs never touch L1 — the wrapper compresses to ~2 kB calldata.

### Key material and memory: the big change

Documented lineage of the EVM-path setup requirement, same warning slot in the docs across
versions:

| Version | Documented requirement | Source |
|---|---|---|
| v1.0.0 | "requires very large amounts of computation and memory (**~200 GB**)" | [book/src/writing-apps/prove.md @ v1.0.0](https://github.com/openvm-org/openvm/blob/v1.0.0/book/src/writing-apps/prove.md) |
| v1.4.0 | "large amount of compute and memory (**~70 GB**) and can take ~7mins on a `m6a.16xlarge`" | docs @ v1.4.0, generating-proofs.mdx |
| **v2.0.0** | "requires a large amount of computation and memory (**~16 GB**)... Use `--download` to download the pre-built Halo2 proving key and verifier artifacts from S3 instead of generating them locally" | [docs @ v2.0.0, generating-proofs.mdx](https://docs.openvm.dev/book/writing-apps/generating-proofs/) |

The ~200 GB figure is what the "~256 GB box" plan (SYB-126) was based on. **It is obsolete.**
Additionally in 2.0.0:

- `cargo openvm setup --evm --download` fetches `halo2.pk`, the Solidity verifier contract,
  and bytecode prebuilt from S3 — no local generation, no `solc` (v2.0.0 docs). Our justfile
  already has this target (`openvm-setup-evm-download`, justfile:95).
- The [OpenVM Solidity SDK](https://docs.openvm.dev/book/writing-apps/solidity-sdk/) ships
  verifier contracts "generated at official release commits," built per minor release, in
  two variants: `v2.0-base` (no deferrals — what `setup --evm` produces) and
  `v2.0-deferral`. Deployable via `forge create` from
  [openvm-org/openvm-solidity-sdk](https://github.com/openvm-org/openvm-solidity-sdk).
- Aggregation keys are tiny now: our box's `internal_recursive.pk` is **~91 KB**
  (measured 2026-07-10 during the upgrade; app-independent). Under 1.x the equivalent
  `agg.pk` was ~16 GB (1.x SDK docs). App-specific keys (`app.pk`, `agg_prefix.pk`) are
  produced by `cargo openvm keygen`, which our 31 GB box handles (`--app-only` path is in
  the zk-smoke lane).

**Caveat (observed vs documented):** plain `cargo openvm setup` OOM'd our 31 GB shared box
on 2026-07-10 (with two heavy lanes resident; a partially-killed run still wrote a valid
`internal_recursive.pk`). Docs say ~16 GB. Most likely concurrent memory pressure, but we
have not measured peak RSS on a quiet box — see Open Questions.

### The genuinely new primitive: deferral / verify-stark

v2.0.0 adds "the continuation aggregation pipeline and deferral framework, including the
`verify-stark` deferral path for guest programs"
([v2.0.0 release notes](https://github.com/openvm-org/openvm/releases/tag/v2.0.0)). A guest
can call `verify_stark(input_commit, expected)` to establish — via a deferral circuit proven
during aggregation, not in the VM — that a child `VmStarkProof` verifies and revealed given
public values ([verify-stark docs](https://docs.openvm.dev/book/guest-libraries/verify-stark)).
This is the SWIRL-native way to aggregate proofs of *separate executions* (the continuation
tree itself only chains segments of one execution — spec, "segment adjacency").

Two practical limits (documented): "Deferrals require programmatic setup and can only be
configured through the SDK; **the OpenVM CLI does not currently support deferrals**"
([deferral docs](https://docs.openvm.dev/book/acceleration-using-extensions/deferral)), and
deferral-enabled proving needs the different `v2.0-deferral` on-chain verifier variant.

## 2. Our workload shape (repo @ 2793d35c, read 2026-07-10)

- **Blocks:** 10 s cadence on the deployed devnet (`docs/SPEC.md` §block lifecycle;
  `SequencerConfig::block_interval`) → 8,640 blocks/day, 360/hour. Fresh genesis today
  (2026-07-10, `e16c7655…`), so there is essentially **no historical backlog yet** — the
  cadence decision sets SYB-87's unit before the backlog grows.
- **Guest** (`zk/openvm-guest/`, rv32im + io + sha2 per `openvm.toml`): verifies one
  `StateTransitionGuestInput` — post-state qMDB exact-keyspace proofs, next_key ring,
  events root, deposit checkpoint, witness root, then the match/settlement/order layers via
  `sybil-verifier`. Reveals one 32-byte
  `keccak256(abi.encode("sybil/openvm/state-transition/v1", …))` public value. Pins:
  `app_exe_commit 0x000f896e…`, `app_vm_commit 0x007a02fc…`
  (`guest.commitment.lock.json`; deterministic, path-independent rebuild).
- **Prover pipeline** (`crates/sybil-prover`): witgen → prepare (runs the **native**
  `sybil-zk` transition verifier before emitting guest input) → file-based worker
  (per-block artifact dirs, DA payload/manifest, `status.json`) → `serve` (HTTP + metrics).
  Strictly **one job = one block**. Real app proofs work locally (`just zk-smoke true`);
  EVM path never stood up; devnet runs mock proving + `UnsafeAcceptAllVerifierAdapter`.
- **L1 leg today** (`contracts/src/OpenVmVerifierAdapter.sol`): dev adapter pinning
  exe/vm commits, checking the 32 public-value bytes against the settlement public-input
  hash, then calling `IOpenVmHalo2Verifier.verify` — ABI already aligned to the 2.0.0
  32-byte public-values shape (fixed in 2793d35c). `SybilSettlement.submitStateRoot`
  enforces `previousHeight == latestHeight` and `previousStateRoot == latestStateRoot` —
  i.e. **submissions must chain contiguously**; you cannot skip-submit every Nth
  single-block proof without a proof that spans the gap.
- **No cycle-count data exists** for the guest at any load (grep for proving
  time/cycles across docs/design: nothing recorded). SYB-88 is the vehicle.

## 3. Options analysis

Fixed background costs, all options: app-proving compute is proportional to total guest
cycles across all blocks regardless of batching; what batching changes is (i) how many
aggregation pipelines + Halo2 wraps we run, and (ii) how many L1 submissions we pay for.

### L1 gas model (mainnet-grade path; Sepolia is free but same shape)

Per `submitStateRoot` with a real EVM proof:
- Halo2 verify: **~316K gas** (documented, 2.0-beta blog).
- Calldata: ~2.2 kB (1,760 B proof_data + public inputs struct + ABI framing) ≈ ~35K gas.
- `SybilSettlement` storage + vault calls (3–4 SSTOREs, cold calls, events): ~60–100K gas (inferred from contract shape).
- **Total ≈ ~420–460K gas per submission.**

| Cadence | Submissions/day | Gas/day | ETH/day @1 gwei | $/day @ $3k/ETH, 1–5 gwei |
|---|---|---|---|---|
| (a) per block (10 s) | 8,640 | ~3.9 B | ~3.9 | **$11.7k–58k — dead** |
| (b/c) hourly epoch | 24 | ~10.8 M | ~0.011 | $32–162 |
| 6-hour epoch | 4 | ~1.8 M | ~0.0018 | $5–27 |

Per-block submission also means one Halo2 wrap per 10 s forever — the wrapper leg alone
(minutes-class on CPU, unmeasured for 2.0) can't keep that cadence on reasonable hardware.
**(a) monolithic per-block + submit-each is eliminated on both gas and wrapper-throughput.**

### (b) Per-block proofs + verify-stark recursive aggregation

Prove each block to a `VmStarkProof` (app → full agg pipeline per block), then an
aggregator guest verify-starks N of them, checks root chaining, reveals one epoch hash;
one EVM wrap per epoch. Honest assessment:

- **Pros:** per-block proof artifacts exist immediately (nice ops signal); blocks provable
  in parallel across machines; epoch failure isolates to one block's proof.
- **Cons:** N full aggregation pipelines per epoch *plus* the aggregator's own pipeline —
  strictly more proving work than (c); **SDK-only** (no CLI support for deferrals) — our
  entire toolchain (justfile, zk-rebuild CI gate, fingerprint lock, commit.json) is built
  on `cargo openvm`; needs the `v2.0-deferral` verifier variant on L1 and a second
  validity-pinned guest (the aggregator), i.e. two commitment sets to manage; the deferral
  key material (verify-stark circuit keys) is the one artifact class that is *not*
  prebuilt/downloadable.
- This is the right long-term shape **if** we later need parallel multi-box proving or
  proof-carrying interop (e.g., selective-reveal proof cluster SYB-89–94 verifying
  settlement proofs). Not now.

### (c) Monolithic epoch guest — recommended

Change the guest from "verify 1 block witness" to "verify N consecutive block witnesses":
loop the existing per-block verification, constrain `new_state_root[i] ==
previous_state_root[i+1]` and `height[i]+1 == height[i+1]`, accumulate the deposit
checkpoint and a digest over per-block `da_commitment`s, reveal one
`keccak256(abi.encode("sybil/openvm/state-transition/v2-epoch", first_prev_root,
last_new_root, first_height, last_height, deposit_root/count, epoch_da_digest))`.
Continuations make execution length a non-issue — the VM segments the run and the
aggregation tree collapses it to one root proof exactly as today (spec §aggregation
pipelines). Memory in-guest stays bounded by processing block-by-block (witness data is
streamed via stdin; drop each block's data after verifying).

- **Pros:** one aggregation pipeline + one Halo2 wrap + one L1 submission per epoch —
  minimum total proving overhead of all options; stays 100% on the CLI toolchain
  (`prove evm` just works); one guest, one commitment pair, existing fingerprint/zk-rebuild
  machinery unchanged in kind; `SybilSettlement`'s contiguous-chaining check is satisfied
  by construction; per-block guest remains available for spot-checks.
- **Cons:** proof latency = epoch length + proving time (withdrawals/deposit-finality wait
  for epoch close — irrelevant next to the 14-day challenge window in
  `matching-sequencer/src/bridge.rs`); a mid-epoch unprovable block is discovered at epoch
  proving time — mitigated because `prover prepare` already runs the native `sybil-zk`
  transition verifier per block, so semantic failures surface within seconds of block
  production; only guest-target-specific drift (the SYB-208 class) escapes native checks,
  covered by the weekly zk-rebuild gate + an optional per-day spot app-proof of one block.
- New guest = **new commitments** → the epoch guest must land *before* the Sepolia adapter
  deploy, or we eat an immediate repin (procedure exists, `zk/openvm-guest/README.md`).

### Recommendation

**Adopt (c): time-based epochs, one monolithic epoch-guest proof per epoch, submitted as
one EVM proof per `submitStateRoot`.**

- **Cadence: 1-hour epochs (360 blocks) on Sepolia.** Rationale: 24 submissions/day is a
  meaningful pipeline exercise without being noise; hourly bounds the re-prove blast radius
  and the "unprovable block discovered late" window; gas at mainnet prices would be
  $30–160/day — already viable, with an easy dial to 4–6-hour epochs for mainnet. Make
  epoch length a config value (blocks-per-epoch with a wall-clock cap), not a constant.
  Devnet-idle optimization (later): skip submission when the epoch's state root is
  unchanged and no deposits were checkpointed.
- Keep the per-block job pipeline exactly as-is up through `prepare` (native verification
  per block, DA artifacts per block); the worker gains an epoch assembler that
  concatenates N prepared guest inputs into one epoch input.
- Revisit verify-stark aggregation when either (i) proving throughput requires multi-box
  parallelism, or (ii) the selective-reveal cluster needs proof composition.

## 4. SYB-126 hardware verdict

**The ~256 GB requirement is dead.** It derived from v1.0.0's documented ~200 GB
`cargo openvm setup`; v2.0.0 documents **~16 GB** for `setup --evm` (7 min on an
m6a.16xlarge when generating locally) and makes even that optional via
`--download` (prebuilt `halo2.pk` + verifier from S3). The on-chain verifier itself needs
**zero** local keygen — deploy `v2.0-base` from openvm-org/openvm-solidity-sdk. SYB-30 is
therefore no longer hardware-gated at all.

What the recommended path actually needs:

| Workload | Documented/measured | Verdict |
|---|---|---|
| App proving only (current devnet + spot checks) | runs on the existing 31 GB box today (app keygen `--app-only` + `prove app` in zk-smoke) | **existing box, no purchase** |
| Full EVM path (epoch proving for Sepolia) | heaviest documented stage ~16 GB (`setup --evm`); agg keys ~91 KB; `prove evm` peak RSS **undocumented** | **64 GB / 16–32 core CPU box** (e.g., Hetzner AX52-class, ~€100–130/mo, or cloud spot for the trial) — 4× documented headroom |
| Throughput upgrade (only if SYB-88 shows CPU can't hold cadence) | 11.4 MHz app proving on one RTX 5090 (documented, GPU) | single consumer-GPU box or rented GPU spot; decide **after** SYB-88 |

Do **not** sign a long lease before one measurement: peak RSS of the full
`setup --evm --download` → `prove stark` → `prove evm` sequence on a real exported block
job (`/usr/bin/time -v`, quiet 64 GB spot instance, ~2–4 hours, ~$10–20). This also
resolves the discrepancy between the documented ~16 GB and our 31 GB box OOMing on plain
`setup` (concurrent-lane pressure is the likely explanation; the box also carries the
"restore-not-regenerate" key-material policy, so we never rerun setup there anyway).

## 5. Sequencing implications

| Ticket | Implication of this note |
|---|---|
| **SYB-126** (proving box) | Re-scope: 256 GB rental → one-off 64 GB spot-instance experiment (peak RSS + wall time), then a 64 GB CPU box if Sepolia goes live. GPU decision deferred to SYB-88 data. |
| **new: epoch guest** | The one implementation item this note adds: N-witness epoch guest + epoch public-input hash + epoch assembler in `sybil-prover` worker + `SybilTypes.StateTransitionPublicInputs` unchanged (first/last roots slot into previous/new). Must land **before** the Sepolia adapter pin. |
| **SYB-95/31** (Sepolia deploy) | Unblocked from hardware entirely: deploy `OpenVmHalo2Verifier` (`v2.0-base`, prebuilt bytecode, solc 0.8.19) + `OpenVmVerifierAdapter` + vault/settlement. Sequence the adapter deploy after the epoch guest's commitments exist; still gated on Valery's funded-key go. |
| **SYB-30** (real on-chain verification) | No longer needs SYB-126 hardware — verifier artifacts are prebuilt. Needs: epoch guest pins + one real `prove evm` output to run through forge tests against the SDK bytecode (golden-vector style). |
| **SYB-87** (historical backfill) | Unit of backfill = epoch. Genesis is one day old — decide now, backlog stays trivial. Backfill = sequential epoch proving from genesis; requires sustained proving ≥ ~2× real time to converge; at hourly epochs the job count stays small (24/day). Blocked only on epoch guest + prover stability on v6. |
| **SYB-88** (benchmark suite) | Design to the recommended path: (i) guest cycles/block at 100/1k/10k orders (execution only, runs on current box — no proving needed); (ii) CPU app-proving MHz on our guest (the missing number for the GPU decision); (iii) epoch-path end-to-end wall time + peak RSS (`prove app/stark/evm` split); (iv) regression guard on cycles/block. The recruiting number "10k trades proven in X" is (ii) applied to (i). |

## 6. Open questions → cheapest closing experiment

1. **Guest cycles per block at realistic load?** (Feeds everything: epoch sizing, CPU/GPU
   call, SYB-88 targets.) → `cargo openvm run` over exported jobs at 100/1k/10k orders on
   the current box; execution only, no proving, no new hardware. Cheapest, do first.
2. **Peak RSS + wall time of the full EVM path on our guest?** (Docs say ~16 GB for the
   heaviest stage; our box OOM'd plain `setup` under lane pressure; `prove evm` peak is
   undocumented.) → 64 GB spot instance, `setup --evm --download`, then time/measure
   `prove app` → `prove stark` → `prove evm` on one real block job. ~$10–20.
3. **Does the S3-prebuilt `halo2.pk`/verifier match our adapter ABI end-to-end?**
   → same spot instance: `cargo openvm verify evm` on the produced proof, then a forge test
   feeding the proof through `OpenVmVerifierAdapter` against the SDK `verifier.bytecode.json`.
4. **CPU proving throughput (MHz) for our extension set (rv32im+sha2)?** → falls out of
   experiment 2's `prove app` timing ÷ experiment 1's cycle count.
5. **Epoch-guest overhead** (witness decode + chaining across N blocks vs N single runs)?
   → prototype the loop, `cargo openvm run` an N=10 epoch input; compare instret sums.
   No proving required.
6. **Halo2 wrap wall time per submission on CPU** (sets the floor on epoch cadence if CPU-only)?
   → experiment 2's `prove evm` − `prove stark` split.

## Citations

- [Announcing OpenVM 2.0, Powered by SWIRL](https://blog.openvm.dev/2.0) — throughput, proof size, security claims.
- [OpenVM 2.0 Beta announcement](https://blog.openvm.dev/2.0-beta) — **316K gas** EVM verification via Halo2-based recursive proof.
- [SWIRL paper](https://openvm.dev/swirl.pdf) — proof system design, recursion-friendliness.
- [v2.0.0 generating-proofs docs](https://docs.openvm.dev/book/writing-apps/generating-proofs/) (source: `docs/vocs/docs/pages/book/writing-apps/generating-proofs.mdx` @ tag v2.0.0) — **~16 GB** setup, `--download`, artifact list.
- [v1.0.0 book, prove.md](https://github.com/openvm-org/openvm/blob/v1.0.0/book/src/writing-apps/prove.md) — the obsolete **~200 GB** requirement (basis of the 256 GB plan); v1.4.0 docs — interim ~70 GB.
- [Continuations spec @ v2.0.0](https://docs.openvm.dev/specs/architecture/continuations) — aggregation tree, VK convergence, deferral integration, segment-adjacency (why cross-execution batching needs verify-stark or a batch guest).
- [Deferral](https://docs.openvm.dev/book/acceleration-using-extensions/deferral) / [Verify STARK](https://docs.openvm.dev/book/guest-libraries/verify-stark) docs — SDK-only, no CLI support.
- [Solidity SDK docs](https://docs.openvm.dev/book/writing-apps/solidity-sdk/) / [openvm-solidity-sdk](https://github.com/openvm-org/openvm-solidity-sdk) — prebuilt `v2.0-base` / `v2.0-deferral` verifier variants.
- [v2.0.0 release notes](https://github.com/openvm-org/openvm/releases/tag/v2.0.0) — SWIRL via STARK Backend v2.0.0, recursive verifier circuit, continuation aggregation pipeline + deferral framework (verify-stark path).
- [stark-backend](https://github.com/openvm-org/stark-backend) — v2.0.0 production-recommended, zkSecurity audit (README claim).
- Repo evidence @ 2793d35c: `zk/openvm-guest/{README.md,openvm.toml,guest.commitment.lock.json}`, `docs/architecture/ZK Integration Path.md`, `contracts/src/{OpenVmVerifierAdapter,SybilSettlement}.sol`, `crates/sybil-prover/src/`, `justfile:72–203`, `docs/SPEC.md:184`, `crates/matching-sequencer/src/bridge.rs:13`; measured `internal_recursive.pk` ~91 KB (2026-07-10 session memory, devnet baseline).

**Documented vs inferred:** all RAM/gas/throughput figures above marked "documented" trace
to the linked sources at pinned versions. Inferred items are flagged inline: the
~60–100K gas settlement-contract overhead (from contract shape, not measured), CPU proving
throughput (no public 2.0 CPU numbers), `prove evm` peak RSS (undocumented), and the
explanation for our 31 GB OOM (concurrent lane pressure — plausible, unverified).
