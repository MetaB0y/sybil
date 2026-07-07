---
tags: [moc, math, reference]
layer: core
status: current
date: 2026-07-07
---

# Math papers — canonical location

The authoritative math papers now live in a dedicated repo:
`~/github/prediction-markets-are-fisher-markets/`. The in-repo `design/` copies
were **removed 2026-07-07** to avoid drift; update the canonical repo instead.

## Old → new mapping

| Old (removed) `design/…`  | Canonical `~/github/prediction-markets-are-fisher-markets/…` |
| ------------------------- | ----------------------------------------------------------- |
| `lmsr-proof.typ`          | `paper.typ` — *Prediction Markets Are Fisher Markets*       |
| `math-primer.typ`         | `primer.typ`                                                |
| `decomposition.typ`       | `decomposition.typ` — **corrected July 2026**               |
| `bundle-clearing.typ`     | `bundle-clearing.typ` — **corrected July 2026**             |

The July 2026 corrections to `decomposition.typ` and `bundle-clearing.typ`
supersede the earlier drafts; see those files for the revised bundle/coupling
factorization.

## Sybil-local derivations (no canonical counterpart)

`design/eg-conic.typ` and `design/mint-pnl.typ` are sybil-specific derivations
with no counterpart in the canonical repo and remain here.
