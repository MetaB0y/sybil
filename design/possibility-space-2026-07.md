---
tags: [design, brainstorm, roadmap, vision]
layer: vision
status: brainstorm
date: 2026-07-07
---

# Possibility Space — features & design elements worth wanting

A forward brainstorm, deliberately *beyond* the finish-what's-started backlog in
`docs/review/30-roadmap.md`. The organizing principle is Ousterhout's: the
**deepest** opportunities exploit structure Sybil already has rather than bolt on
against it. Sybil's three structural assets are unusual in combination:

1. **Fisher-market / EG clearing** ([ADR-0001](../docs/adr/0001-eg-fisher-market-matching.md)) — one joint convex program prices *anything* coherently.
2. **A ZK-proven state transition** with a commodity-hardware escape path.
3. **Batch auctions** — discrete-time clearing with no intra-batch time priority.

Ideas are scored on **Leverage** (does it exploit an asset above, or fight it?),
**Validity impact** (does it move the guest commitment?), and **Horizon**.
The three flagged ⭐ are the ones this architecture makes *uniquely* good — where
a CLOB or an AMM competitor structurally cannot follow.

---

## I. Market types the EG clearing makes uniquely possible

The Fisher-market program prices a whole basket of instruments coherently *in one
solve*. A CLOB prices each book independently and bolts coherence on afterward;
an AMM can't do multi-outcome coherence at all. This is the moat — lean into it.

### ⭐ 1. Combinatorial & conditional markets ("X if Y")
`Order` already carries latent payoff-vector generality
(`matching-engine/src/order.rs`) that no solver honors yet
([ADR-0006](../docs/adr/0006-witness-v3-full-snapshot.md) notes changing it is a
validity move). The payoff: **conditional markets** — "Fed cuts *if* CPI < 3%" —
priced jointly with the unconditional markets so no arbitrage exists between
them. This is the headline capability of a Fisher-market exchange and nobody in
prediction markets does it well. Design exists in spirit
(canonical `~/github/prediction-markets-are-fisher-markets/decomposition.typ`,
`bundle-clearing.typ`; see `design/math-papers.md`); the work is real
(validity-critical, `Order` generality + solver support). **Leverage: maximal.
Validity: yes. Horizon: the flagship medium-term bet.**

### 2. Scalar / range markets
Outcomes as a number (e.g. "2026 US GDP growth") rather than a binary. The EG
framework prices a distribution over a range; LMSR-style scoring becomes the
per-market utility. Reuses the clearing; the new surface is the resolution
encoding (a number, not a winner) and the payoff shape. **Leverage: high.
Validity: moderate (new outcome/resolution type). Horizon: medium.**

### 3. Basket / index markets
A single tradable instrument over a weighted set of outcomes ("our top-10
election index"). Falls out of payoff vectors once #1 exists — mostly a
product/UX layer over the same clearing. **Leverage: high (free-riding on #1).
Validity: shares #1's. Horizon: after #1.**

### 4. Continuous / perpetual predictions with a funding rate
A market that never resolves — it tracks "current probability of X" and pays a
periodic funding rate between longs/shorts (perp-style). Fits batch clearing
naturally (funding accrues per block). New: funding mechanism + never-resolves
lifecycle. **Leverage: medium. Validity: moderate. Horizon: medium-long.**

### 5. Parimutuel pools for the long tail
For cold, illiquid markets, a parimutuel pool (bet against the pool, settle
pro-rata) sidesteps the need for two-sided liquidity. A *different* clearing rule
for a *different* liquidity regime — offer it per-market alongside EG. **Leverage:
medium. Validity: new settlement path. Horizon: opportunistic.**

---

## II. Privacy & fairness the batch structure makes uniquely possible

### ⭐ 6. Sealed-bid batch auctions (encrypted mempool + threshold decrypt)
This is the sleeper. A batch auction has **no intra-batch time priority**
([ADR-0001](../docs/adr/0001-eg-fisher-market-matching.md)) — so orders within a
batch can be **sealed** (encrypted) until the batch closes, then
threshold-decrypted and cleared together. Result: **no front-running, no
sandwiching, no order-flow leakage** — MEV-resistance as a *structural* property,
not a patch. A CLOB cannot do this (continuous matching needs plaintext now); an
AMM leaks to the mempool. Sybil already has the one precondition CLOBs lack: a
discrete clearing instant. Prerequisites: an encrypted-order envelope, a
threshold-decryption committee (or a delay/VDF variant for single-operator), and
the mempool already exists (`docs/architecture/Mempool.md`). **Leverage: maximal
and nearly free architecturally. Validity: adds a decrypt step before clearing.
Horizon: high-value medium-term; strong differentiator.**

### 7. ZK proof-of-reserves / solvency
The vault + state root already exist; a periodic proof that
`sum(account_balances) ≤ vault_collateral` is a small guest program over the
committed state. Turns "trust us" into "verify us" for the collateralization
invariant — a credibility feature for the same audience the escape path targets.
**Leverage: high (reuses the state commitment). Validity: new small guest.
Horizon: medium.**

---

## III. Agents & the arena as a first-class product

Sybil already runs an arena of trading bots (LLM traders, the OpenRouter loop).
That's not a demo — it's a **product surface** a prediction-market exchange is
unusually well-placed to own.

### ⭐ 8. The arena as a "prediction-market gym" for agents
A public, permissionless venue where third-party trading agents compete on real
(or play-money) markets with a leaderboard, standardized API, and reproducible
scoring. Prediction markets are the *ideal* agent benchmark — objective ground
truth (markets resolve), continuous scoring, and a clean API. This is a growth
engine and a moat: the exchange with the liveliest agent ecosystem wins the
liquidity. Prerequisites: capability-scoped keys (#9), a stable agent SDK
(`sybil-client` is the Rust seam), and public arena history endpoints (partly
built, SYB-115). **Leverage: high (arena exists). Validity: none. Horizon: a
distinguishing near-term product bet.**

### 9. Capability-scoped agent keys
Already anticipated (the ratification packet defers `KeyScope`→`capability_mask`
to post-v1). Delegated keys that can **trade but not withdraw/escape** make it
safe to hand an agent (or a third party) a key. This is the enabler for #8 and
for institutional/managed accounts. Small once `keys_digest`
([ADR-0008](../docs/adr/0008-in-guest-p256-openvm-ecc.md)) lands — it's one more
digested field. **Leverage: high. Validity: extends the keys_digest schema.
Horizon: right after SYB-225.**

---

## IV. Economic design

### 10. Welfare-share fees
Because EG computes the **welfare gain** of each clearing explicitly, fees can be
a function of realized welfare (charge a slice of the surplus a trader captured)
rather than a flat/notional bp. This is a fee model only a welfare-maximizing
exchange can express honestly. Needs care not to distort the clearing incentives.
**Leverage: high (EG-specific). Validity: touches settlement. Horizon: medium;
design-first.**

### 11. Cold-start liquidity: seeded LMSR bootstrap
Even with EG as the core ([ADR-0001](../docs/adr/0001-eg-fisher-market-matching.md)
keeps LMSR's *intuition*), a market with no orders is dead. Seed new markets with
a subsidized LMSR-style maker that the EG clearing treats as just another
participant, retiring it as real liquidity arrives. Bootstraps the two-sided
liquidity problem without abandoning the core. **Leverage: high. Validity: the
seed is a participant, so mostly off the proven path. Horizon: needed before real
launch of many markets.**

### 12. Insurance / backstop fund + its ZK attestation
A protocol-owned backstop for resolution disputes or bad debt, with its balance
provable via #7. Pairs with the oracle challenge path. **Leverage: medium.
Horizon: pre-mainnet.**

---

## V. Trust-minimization & scaling (longer horizon)

### 13. Oracle propose/challenge/AutomatedL0
Already reserved arms (survey; `docs/architecture/Oracle System.md`). The path
from operator-attested resolution to a **challengeable** resolution with an L0
automated tier is the credibility spine for resolution — the one thing users
must trust today. **Leverage: high. Validity: resolution path + `MarketStatus`
Voided/re-resolution. Horizon: the key trust-minimization milestone.**

### 14. Volition: per-position DA choice (validium ↔ rollup)
`docs/architecture/Data Availability.md` already points at validium. Let *users*
choose, per position, whether their data goes to full DA (costly, maximal
safety) or off-chain (cheap, operator-trust) — a volition model. Directly
addresses the full-snapshot payload cost from
[ADR-0006](../docs/adr/0006-witness-v3-full-snapshot.md). **Leverage: high.
Validity: DA commitment shape. Horizon: long.**

### 15. Multi-operator rotation (beyond single-operator replacement)
[ADR-0005](../docs/adr/0005-escape-via-operator-replacement.md) gives
*replacement*; the next rung is *rotation* — a small operator set with handoff, so
liveness doesn't depend on one machine. A stepping stone toward shared/based
sequencing without going straight to full decentralization. **Leverage: medium.
Validity: none directly (it's an ops/liveness layer). Horizon: long.**

### 16. Recursive proof aggregation
As proving cost grows (esp. after in-guest P-256,
[ADR-0008](../docs/adr/0008-in-guest-p256-openvm-ecc.md)), aggregate multiple
block proofs recursively to amortize L1 verification. OpenVM's aggregation layer
(`agg_prefix.pk`) already implies this direction. **Leverage: medium. Horizon:
when proof cost bites.**

### 17. Horizontal scaling by market-shard
[ADR-0010](../docs/adr/0010-acknowledged-write-wal.md)'s single-writer actor is a
throughput ceiling. Sharding the sequencer by disjoint market sets (each shard
its own actor + state subtree, roots combined) is the escape hatch — but only
once a single shard is provably saturated. **Leverage: low now (premature).
Validity: state-root composition. Horizon: only when measured as needed.**

---

## Recommended reading of this space

If forced to pick the bets that are both **differentiating** and **architecturally
cheap relative to their payoff**: **#6 sealed-bid batch auctions** (MEV-resistance
almost for free, structural moat), **#1 conditional/combinatorial markets** (the
Fisher-market headline capability), and **#8 the agent arena** (a growth engine
that already half-exists). #9 capability keys is the small unlock that makes #8
safe. Everything in §V is real but later, and #17 should not be built until
measured.

None of these should jump the current queue (god-split → keys_digest → redeploy →
shakedown). This is the *shape of the after* — a map, not a schedule.
