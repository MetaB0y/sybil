# Executive Summary

## Verdict

Sybil is a strong idea executed at high velocity, and it shows in both directions. The **spine is excellent**: a prediction-market exchange built on frequent batch auctions whose matching coherence emerges from one joint Eisenberg–Gale/Fisher-market program rather than a pile of side-constraints; an all-integer settlement core shared verbatim between the sequencer and its verifier; a block-boundary persistence fence that is honest about not having cross-database atomicity; a four-layer verification design with a genuinely clever exact-keyspace qMDB proof; and an Obsidian architecture vault with a validated link graph that most funded teams never achieve. This is the work of someone who understands the problem deeply.

The **flesh around that spine is in the state the maintainer described**: bloated, inconsistent, and in several places buggy in ways that matter. The review found a small number of genuine correctness and safety defects — including two that leak value and several that make the verification layer attestable-but-wrong — sitting inside a large volume of dead experiments, half-finished migrations, and documentation that describes a different system than the one that runs. None of this is fatal. All of it is fixable, and the project's own stated rule — *elegance over backward compatibility, we are in early dev* — is exactly the license needed to fix it decisively.

The distance from here to "amazing, crispy clear and nice" is not a rewrite. It is: **fix ~10 bugs, delete ~15k lines, finish 2 migrations, and make 4 conventions mechanical instead of aspirational.**

## The five things that are true at once

1. **The matching core is sound but over-promised.** The LP solver family is well-factored and deterministic-enough, and settlement math is a clean pure module. But the *edges* — the API, the `Order` struct, the docs — advertise payoff vectors, multi-market spreads/bundles, conditional orders, and multi-outcome states, while every solver silently assumes single-market binary one-hot orders, enforced only by `debug_assert`s that vanish in release builds. This gap is the source of the two most serious bugs in the repo.

2. **The safety net is advisory.** The sequencer computes balance-conservation and position-balance invariants and runs full block verification (`verify_full`) on every block — then only `error!`-logs on failure and commits the block anyway. The verifier the docs call authoritative is, at runtime, a diagnostic. Combined with (1), the only online defense against a mis-solved batch is a log line.

3. **The conventions are prose, not constraints.** "No floating point anywhere" and "prefer actors over mutexes" are stated in AGENTS.md and violated in the code (f64 on a state-root-committed reservation path; three different actor idioms; shared `Arc<RwLock>` where an actor was intended). "All-integer" is undercut by unchecked `i64` multiplications with no `[profile.release] overflow-checks`. These are one clippy config and one newtype away from being enforced.

4. **There are two (or four) of almost everything.** Two block-stream transports (SSE + WebSocket). Three `Order` representations hand-mapped. Four frontends, of which the *worst* one is the only one deployed and its order-signing can never verify. A production node crate that also contains a full agent simulator. Consensus-critical hash and digest encodings copy-pasted across three crates and already diverging. A documentation estate in five layers at five different truth levels.

5. **The deployment posture is a demo with the doors open.** The public devnet runs `SYBIL_DEV_MODE=true`, which exposes unauthenticated account-minting and arbitrary market resolution to the internet; every internal port (unauthenticated metrics store, Grafana `admin/admin`, the raw API) is published on the public host; the entire alerting stack lives on the machine it monitors with no external heartbeat; and a live API key is committed to git. One `curl` can resolve every mirrored market.

## The through-lines (full treatment in [02-cross-cutting-themes.md](02-cross-cutting-themes.md))

The individual findings are symptoms of ten recurring patterns. The three that explain the most:

- **The aspirational/actual schism.** For every half-built capability — payoff vectors, conditional orders, the oracle's Propose/Challenge arms, the vault's escape-mode, the ZK deposit-inclusion proof, sidecar-transition verification — the code, types, and docs claim more than the runtime does. *The half-built state is worse than either finishing or deleting.* Resolving each schism in one direction is the single highest-leverage move, and it is squarely what "elegance over backcompat" authorizes.
- **Verify, don't log.** Turn the existing invariant checks and `verify_full` into a precondition of block commit. This one change converts the four-layer verification from decoration into an actual safety property and closes the runtime exposure from the money-leak bugs.
- **Make the conventions mechanical.** `Nanos`/`Qty` newtypes with checked i128-backed arithmetic, `overflow-checks = true` in release, and `clippy::disallowed-types` banning `f64` in core modules would turn an entire class of findings (silent overflow, f64 drift, lossy casts) into compile errors, and finally make "all-integer discipline" true.

## What to do, in order

This is the compressed roadmap; the sequenced version with effort estimates is in [30-roadmap.md](30-roadmap.md).

**Phase 0 — Stop the bleeding (days).** Reject non-one-hot / multi-market / unbounded-quantity orders at admission and in `convert.rs`; add `[profile.release] overflow-checks = true`; make `verify_full` + conservation checks fatal to block commit; bind `SYBIL_DEV_MODE` off in prod (or gate mint/resolve behind an admin token); rebind internal ports to localhost; rotate and un-commit the API key. These are small, mostly-deletions, and they close every internet-reachable and value-leaking hole.

**Phase 1 — Delete the sediment (1–2 weeks).** Remove `arena/nba/` (6.6k dead lines), the duplicate MM scripts and legacy traders, the fictional Mintlify pages and dead Kamal runbook, `matching-solver/verifier.rs`, `book.rs`, the `PipelineResult` fossils, one of the two block-stream transports, and two of the four frontends. Net: ~15k lines gone, zero behavior change, and "which X is real?" stops needing a review to answer.

**Phase 2 — Resolve the schisms (2–4 weeks).** For payoff vectors: choose honest-minimalism (a `BinaryOrder` solver input) or honest-generality (per-state balance constraints in the LP). Bind the ZK proof to what it commits (fill→account, sidecar transition, deposit inclusion, arithmetic range checks). Finish the hot/cold state split and delete the actor's read-model RPCs. Extract one `sybil-commitments` crate that owns every consensus byte layout.

**Phase 3 — Enforce and harden (ongoing).** Newtype the money types; land CI for arena, docs, frontend tests, the ZK guest, and a feature-lattice check; move to CI-built images on a registry so deploys stop depending on one laptop; add backups and an external heartbeat.

## The north star

Sybil's ambitious end-state is coherent and worth stating plainly, because most of the ambitious ideas in the per-subsystem reviews point at it:

> **One state-transition function, proven end-to-end, over integer money, with one canonical encoding, served by one API to one frontend, deployed from CI.**

Concretely: `apply_block(pre_state, inputs) -> post_state` is a single pure function the sequencer runs, the verifier re-runs, and the ZK guest proves — replacing the four partially-overlapping verification layers and closing the fill-binding, sidecar, and digest gaps in one move. Money is a checked integer newtype the compiler enforces. Consensus bytes live in one `no_std` crate. The API is the one type oracle, generating the frontend's types and the Python SDK. The frontend is the good one, deployed behind the one ingress. And the whole thing ships from a registry, not a `docker save | ssh` from someone's laptop.

Every deletion and every merge in this review is a step toward that sentence.
