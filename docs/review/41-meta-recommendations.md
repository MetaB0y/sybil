# Meta-Recommendations

The things worth doing that sit *around* the review rather than inside it — process, scope, and sequencing advice for whoever picks this up. Ordered by value.

## 1. Prove the two money-leak bugs before "fixing" them

C1 and C2 ([SYB-181](https://linear.app/sybilmarket/issue/SYB-181)) are the highest-stakes claims in the review, and they are **verified by reading, not by reproduction**. The code paths were confirmed (the multi-market defer-gate, the missing `Custom` validation, the absent release profile, settlement crediting both legs), but no test actually drives a spread/bundle or a `[2,0]` order through solve → settle and observes value disappear.

Close that gap first, both directions: if it reproduces, the ticket is ironclad and you have a permanent regression guard; if it *doesn't*, you've avoided deleting order types over a phantom. Write the failing conservation test **before** the fix, commit it red, then fix to green.

## 2. Treat the security items as "today," not backlog

Filing [SYB-173](https://linear.app/sybilmarket/issue/SYB-173) as a ticket makes exposure look like queue work. It isn't:

- **The OpenRouter key in `docs/api-keys.md` is live in git history now.** Deleting the file from `HEAD` does not remove it from history — **rotate the key today**, then scrub/deprecate the old one, then delete the file.
- **`SYBIL_DEV_MODE=true` + open ports mean the public devnet currently accepts unauthenticated account-minting and arbitrary market resolution.** If anything is exposed to anyone, the port rebind (to `127.0.0.1`) and a mint/resolve admin-token gate are same-day changes, independent of the larger dev/prod auth-tier work.

## 3. Reconcile with the parallel pass's SPEC

The SYB-165 pass produced `docs/SPEC.md` (it references an **invariants table §17** and a **known-drift §19**) and `design/architecture-review-2026-07.md`, both on PR #8's branch — **this review was written without seeing them.** Two consequences worth a short reconciliation:

- That invariants table is the natural anchor for R4's "make conventions mechanical" work ([SYB-196](https://linear.app/sybilmarket/issue/SYB-196)) and overlaps [40-do-not-break.md](40-do-not-break.md) — align them so there's one invariants list, not two.
- The two passes may agree, sharpen, or *contradict* each other on specific claims. Diff them for genuine duplicates and disagreements before starting work; where they disagree, the code is the tiebreaker.

## 4. Scope caveats to hold in mind

- **This is an engineering review, not a security audit or a mechanism-design audit.** The trust-model holes (H2–H6, H12–H14) surfaced as a *byproduct* of reading for correctness — that is not the same as a systematic adversarial pass. Before testnet/custody, run a dedicated security review focused on contracts + bridge + auth (the `/security-review` skill, or a real engagement). The economic soundness of welfare-maximization, the MM-budget model, and minting lives in `design/*.typ` + `lean/` and was **not** audited here.
- **These review docs will drift like every other doc the review criticizes.** The file:line anchors were captured against a working copy that includes unmerged devnet fixes, so a few are already slightly off on `main`. Treat `docs/review/` as a **dated snapshot** and the **Linear tickets (SYB-176 tree) as the living source of truth**. When a fix lands, update the ticket, not the doc.
- **Attribution note:** every Linear issue/comment from this pass is authored under the connected account, not "Fable" — the model attribution lives in PR #9's description and the umbrella ticket body.

## 5. Process & prompt suggestions for future work

- **Make the review a repeatable drift-check, not a one-shot.** A cheap recurring prompt keeps it honest as code moves:
  > *"For each open child of SYB-176, re-read the cited file:line and report whether the finding still holds, has moved, or is fixed. Flag stale anchors and anything now contradicted by the code."*
  Runs in the main loop, no fan-out.
- **Failing-test-first for every bug ticket.**
  > *"Before fixing SYB-18x, write the test that reproduces it (red), commit that, then fix to green."*
  Turns each fix into a permanent guard — and is exactly what CI ([SYB-197](https://linear.app/sybilmarket/issue/SYB-197)) should enforce.
- **Scope future fan-outs to changed surface.** The 13-agent sweep cost ~3M tokens (~60% of a limit window) because it read *everything*. A follow-up should target `jj diff`-touched subsystems, or run 4–5 larger agents instead of 13 — same coverage of the active area at a fraction of the spend.
- **Reusable deep-read prompt** (what worked here), per subsystem:
  > *"Deep-read <subsystem> from the code, not the docs. Produce an architecture description as-built; every finding needs a file:line and a concrete failure scenario. Hunt bugs, bloat/dead code, inconsistency, boundary/coupling problems, test gaps, and doc drift. Propose ambitious restructurings consistent with elegance-over-backcompat. Output structured for synthesis, not for a human reader."*

## 6. Two more that didn't make the cut but are worth a glance

- **Confirm the currently-red things** the survey flagged but I didn't run: the arena date test (`test_selection_skips_expired_markets`, AR-2 — should be failing *today* since it's date-dependent), the `zk/openvm-tools` compile break (ZK-2), and the `milp`-without-`lp` feature break (D8). If any is green, the corresponding ticket needs a second look; if red, that's CI debt you're carrying right now.
- **Give the R1 tickets an owner and a cycle.** They're in Backlog. R1 (stop-the-bleeding) is the one group where "sitting in backlog" has a real cost — it's the value-leak and exposure set.
