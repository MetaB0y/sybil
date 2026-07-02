# The Documentation Estate

**Scope:** `docs/architecture/` (Obsidian vault), `docs/` (Mintlify site + articles + ops), the AGENTS.md hierarchy, `design/`, `README.md`, `DEPLOY.md`

## Verdict

The Obsidian vault is genuinely good and worth protecting. The estate *around* it is five layers of sediment at five different truth levels, and the load-bearing numeric facts drift in triplicate. The meta-finding is that the vault claims to be the single canonical spec while four other layers restate overlapping content at different ages — and the facts that appear in multiple places (solver count, violation count, batch cadence, TTL) are exactly where the contradictions live. There is also a **live API key committed to git**.

## The five layers

1. **Obsidian vault (`docs/architecture/`)** — 48 interlinked notes with frontmatter (`layer`, `status`, `last_verified`), a validated link graph (zero broken wiki-links, zero orphans), and a `check-vault.sh` validator. Hot-path notes (Persistence, Mempool, System Diagram, Crate Dependency Map, REST API, WebSocket, Block Witness) are actively co-maintained with code. The **canonical, trustworthy layer** — but freshness is bimodal: 17 notes are frozen at `2026-03-15` (109 days), predating 148 sequencer and 87 arena commits.
2. **Mintlify site (`docs/*.mdx`, `docs/trading/`, `docs/technical/`, `docs/api-reference/`)** — marketing-aspirational, with **two nav configs** (docs.json + legacy mint.json). Documents fees (5 bps + slot auctions — zero fee logic exists), a `@sybil/sdk` TypeScript package and keccak header auth (the real auth is P256 canonical-payload signing), endpoints that don't exist (`GET /batches`, `DELETE /orders/{id}`, ULID ids), and a **deleted solver generation** (`LocalSolver → MultiMarketSolver → NegriskSolver → MMAllocator → MILP → MWIS`).
3. **`design/`** — theory papers (`.typ`) that remain canonical for the math; `architecture.md`/`architecture-diagrams.md` correctly carry "Superseded" banners; `problem-statement.md`/`solver-benchmarks.md`/`user-cli-plan.md` carry no status marker.
4. **AGENTS.md hierarchy** — root + arena + contracts + per-crate files in only **9 of 17 crates** (root AGENTS.md claims "each crate has its own"). Crate files duplicate vault content and are a major drift source.
5. **Operational + session artifacts** — `docs/deployment.md` (accurate) vs root `DEPLOY.md` (dead Kamal runbook); a dated ops report at root; `docs/superpowers/` (22 one-shot implementation plans embedding hardcoded line numbers that rot immediately); marketing articles; and **`docs/api-keys.md` with a live OpenRouter secret**.

## Strengths

- The vault is a real asset: 48 notes, zero broken links, zero orphans, consistent frontmatter, a validation script + justfile tooling — rare discipline.
- Hot-path notes are co-maintained with code (Persistence, Hot State, Mempool match the current design closely).
- System Diagram and Crate Dependency Map are honest, current, boundary-focused, and distinguish planned from built.
- `status: planned` frontmatter is used honestly where it exists (L1 Settlement, Proof Architecture, ZK Integration Path, Hot State).
- The superseded banners on `design/architecture.md` are the right pattern — just not applied everywhere.

## Findings

| ID | Kind | Sev | Summary |
|----|------|-----|---------|
| [D6](01-critical-bugs.md) | ops | high | Live OpenRouter API key committed in `docs/api-keys.md` |
| DOC-1 | doc-drift | high | Public Mintlify docs describe a fictional product: fees, SDK, auth scheme, and endpoints that don't exist |
| DOC-2 | doc-drift | high | `docs/technical/matching-engine.mdx` (+ `matching-solver/CLEARING.md`) documents the deleted pre-rewrite solver architecture with no superseded banner |
| DOC-3 | doc-drift | medium | Block cadence contradiction: vault says 1s, api defaults to 500ms, prod runs 10s |
| DOC-4 | doc-drift | medium | Verification check count differs across three sources: code has 36, MOC says 37, note says 38 |
| DOC-5 | doc-drift | medium | Root README is the worst onboarding doc: points at superseded design docs, omits the vault, mislabels the sequencer, omits 9 crates, pins stale benchmarks |
| DOC-6 | doc-drift | medium | Dead Kamal `DEPLOY.md` contradicts the real deploy path (also [OPS-2](18-ops-deployment.md)) |
| DOC-7 | doc-drift | medium | AGENTS.md repo map drift: 6 crates + 4 top-level dirs missing, sequencer mislabeled, phantom `deploy-dashboard` recipe, wrong note count, false per-crate-AGENTS claim |
| DOC-8 | doc-drift | medium | Sixth solver (IterLpSolver) undocumented in all doc layers except one crate AGENTS.md; five solver tables say "five" |
| DOC-9 | doc-drift | medium | `Block Lifecycle.md` contradicts `Mempool.md` and the current admission code (describes the deleted mempool drain model) |
| DOC-10 | doc-drift | medium | Arena doc layer rotted: `live/` undocumented everywhere, false 5-block TTL claim, all three arena vault notes frozen pre-rewrite |
| DOC-11 | bloat | medium | `docs/` root is a junk drawer mixing the Mintlify site, the vault, marketing articles, a secrets file, a runbook, an audit, and 22 superpowers plans |
| DOC-12 | test-gap | medium | `check-vault.sh` never runs in CI and can't see prose/table path claims or any numeric count; the 17 stale notes emit warnings (exit 0) even at 109 days |
| DOC-13 | doc-drift | low | Crate Dependency Map omits `sybil-polymarket`; no vault note exists for the mirror at all |
| DOC-14 | doc-drift | low | REST API note + `sybil-api/AGENTS.md` lag the actual 52-route surface; the indicative/open-batch subsystem has no vault note |
| DOC-15 | inconsistency | low | `design/` notes lack status markers where `architecture.md` has them |

## Ambitious ideas

1. **Collapse to three doc estates with hard rules:** (1) the `docs/architecture/` vault is the only prose spec; (2) generated reference — README repo-map from `cargo metadata`, API reference from the server's `/openapi.json`, the solver table from one source; (3) `docs/ops/` runbooks. Delete `DEPLOY.md`, `docs/README.md`'s ghost index, `mint.json`, `CLEARING.md`, the superpowers plans, and the marketing articles from the repo (move to a marketing repo). jj history preserves everything.
2. **Ban hardcoded volatile facts in prose:** no counts (38 violation types, five solvers, ~35 notes), no cadences (1-second), no benchmark numbers in README/AGENTS.md. Add a `check-vault.sh` rule that flags digit-adjacent keywords (`solvers`, `checks`, `violation types`, `second batch`) in `status: current` notes, forcing such facts to live in one note or be generated.
3. **Make the vault CI-enforced and code-anchored:** run `docs-check` in CI; extend it to verify every backticked `crates/...` path anywhere in a note, validate the `crate:` frontmatter field against the workspace, and hard-fail `status: current` notes whose crate has >N commits since `last_verified`. Turns `last_verified` from decoration into a contract.
4. **Replace the 9 per-crate AGENTS.md files with a single generated stanza each:** purpose (one line), "read these vault notes," and nothing else. All endpoint lists and type tables in crate AGENTS.md duplicate vault notes and are the second-biggest drift source after the Mintlify site.
5. **Write the four missing vault notes** for deployed-but-undocumented subsystems (Polymarket Mirror, Arena Live Runner, Indicative Prices/Open Batch, Iterative LP Solver), and delete/merge notes whose subject dissolved (fold the aging Mempool/Block Lifecycle pair into one "Admission and Block Production" note).
6. **Introduce an explicit aspirational tier for public docs:** every Mintlify page gets `status: built | partial | vision` rendered as a banner. The introduction's TEE/yield/ZK-reputation claims and the fees page are fine as *vision* documents — the failure mode is only that nothing distinguishes them from the accurate FBA description.
