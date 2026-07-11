# General Advice — Follow-up to the 2026-07 Review

*Follow-up notes from the Fable xhigh analysis session (2026-07-02), answering "what else is worth doing that nobody asked about?" Companion to `design/architecture-review-2026-07.md` (proposals P1–P10, tracked as SYB-165…175) and `docs/SPEC.md`.*

---

## Found during a final sweep

1. **There is no backup story.** The justfile has `deploy-reset-state CONFIRM` (destructive volume wipe) but zero backup/restore tooling, and nothing in `scripts/` or `deploy/`. The `sybil-data` volume holds redb + both qMDB stores — every account, block header, and witness. One disk failure on the single Linode and the devnet chain history is gone. The escape-hatch design protects *users*; nothing protects the *operator*. A nightly `docker run --volumes-from … tar | ssh` cron or a `just deploy-backup` recipe is an afternoon of work and the highest ratio of protection-to-effort available right now.

2. **No LICENSE file anywhere** (README says "[TBD]", no `license` field in any Cargo.toml). The repo is public on GitHub — legally that means "all rights reserved," which is fine if intentional, but worth deciding deliberately before more people look at it.

## Worth doing immediately

**Fact-check the spec before trusting it.** `docs/SPEC.md` was written from six parallel agent explorations. The load-bearing claims were spot-checked against code (payoff indexing against the order-builder tests, dev-mode in compose, solver file inventory, `produce_block_in_place`), but ~200 factual claims went in, and a spec that's 3% wrong quietly poisons trust in the other 97%. A fresh-context adversarial pass is cheap insurance. Prompt to paste:

> Adversarially fact-check docs/SPEC.md against the actual code. Spawn parallel agents per section; for every checkable claim (constants, defaults, file paths, struct fields, endpoint lists, invariants), verify it with file:line evidence. Report only claims that are wrong, stale, or unverifiable — then fix them in place.

**Land the docs PR soon.** Docs rot fastest between writing and merging; the review found five documents that lied precisely because they sat while the code moved.

## Audits worth commissioning (things nobody has done yet)

- **Economic attack surface.** The 2026-07 review covered architecture, not adversarial economics. Nobody has tried to *break the mechanism*: rounding-direction exploits at the floor/ceil boundary, minting arbitrage across groups, self-trade patterns that survive STP, MM budget gaming via the single-pass SLP weakness, wash-trading welfare inflation. Prompt:

  > Attack Sybil's clearing mechanism as an adversarial trader. Construct concrete order sets that extract value or violate intent without tripping the verifier. Turn each candidate into a proptest.

  This is the study most likely to find something that matters before real money is at stake.

- **Mutation-test the verifier.** The 4-layer verifier is the soundness core — but do its tests actually *catch* bugs, or just execute code? `cargo-mutants` on `sybil-verifier` + `matching-engine/src/settlement.rs` answers that empirically. Surviving mutants in a ZK reference implementation are exactly the bugs a prover would happily prove.

- **Block-time budget under load.** The devnet runs 10 s blocks on 1 vCPU; the design says 1 s. Nobody has measured where the budget goes at realistic scale (solve vs settle vs qMDB root vs redb commit vs inline `verify_full`). The blocking-redb issue (SYB-169) makes this more than academic.

## Meta recommendations

- **Use a deep multi-agent review (`/code-review ultra`) on the P1 refactor PR** when it happens. The kernel/views split (SYB-166) touches the consensus path; that's the one change where it pays for itself.
- **Docs process:** the vault's `last_verified` + `check-vault.sh` discipline works — the per-crate AGENTS.md files rot because they're *outside* it. Either shrink each AGENTS.md to a few lines of pointers (preferred; covered in SYB-174), or add them to the staleness checker. Don't leave them as unchecked parallel prose.
- **Make drift mechanical where possible:** the OpenAPI-completeness CI test (SYB-171) is the pattern — anywhere a doc can be checked against code by a script, write the script instead of scheduling vigilance.

## Open items nobody has asked about

- **`witness_root` is not in the block header.** The Block Witness vault note flags this itself: the sequencer can currently equivocate about non-event witness sections without changing the header hash. A protocol-level gap with a written proposal sitting in a note — deserves a ticket before mainnet-shaped decisions, since changing the header format later touches everything.
- **Attestation replay.** Oracle attestations carry a `nonce`, but nothing tracks used nonces — replay is prevented only by the `AlreadyResolved` check. Fine today (resolution is one-shot); becomes a real question the moment `Voided`/re-resolution or fractional updates ship.
- **The 2 GB box is the real single point of failure.** ~11 containers, alerts firing at 650 MiB API RSS, swap alerts configured — the monitoring is already saying the host is at its ceiling. Before adding workload (real proving is coming), either trim services or upsize. Cheaper than debugging OOM-kills mid-demo.

---

**Single next action if picking one:** merge the docs PR, then run the spec fact-check prompt. Everything else is queued in SYB-165 or listed above.
