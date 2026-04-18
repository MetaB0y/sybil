---
tags: [concept, architecture]
layer: oracle
crate: sybil-oracle
status: current
last_verified: 2026-04-18
---

# Oracle System

## Rationale

One primitive, one idea: **a resolution is a signed attestation from a registered `DataFeed`, evaluated by the market's declared `ResolutionPolicy`.** Everything else — challenges, bonds, liveness, predicates, UMA/Kleros bridges, LLM resolvers — is either a new enum arm on `ResolutionPolicy` or a new field on `DataFeed`. No new traits, no per-source bespoke oracle implementations, no call-graph surgery to add a feature.

The alternative — adding `PolymarketOracle`, `UmaOracle`, `LlmOracle`, ... as parallel implementations of an `Oracle` trait — was rejected because each additional source forks settlement logic, bond accounting, and the state machine. That style optimizes for the first feature and punishes every subsequent one.

## Inside-TEE vs outside-TEE

The sequencer (enclave) runs pure state-machine logic: verify signatures, look up registered identities, run policy arms. All external I/O — fetching Polymarket Gamma, polling UMA, asking an LLM — happens in untrusted off-chain signers whose **only** channel into the enclave is a signed attestation. This makes the split TEE-correct by construction: nothing inside the enclave depends on untrusted data except through a signature check against a pre-registered pubkey.

For production TEE deployment, admin / polymarket-mirror signing keys must be sealed to the enclave. SYB-23 stores them on disk for dev convenience — see the migration notes below.

## DataFeed primitive

```rust
pub struct FeedId(pub u64);
pub struct DataFeed {
    pub id: FeedId,
    pub pubkey: FeedPubkey,   // compressed SEC1 P256, 33 bytes
    pub name: String,         // "admin", "polymarket_mirror", ...
    pub created_at_ms: u64,
}
```

**Reserved (designed-in, not built):**

- **Reputation counters**: `successful_attestations: u64`, `challenged_attestations: u64`, `slashed_bonds_nanos: u64`. Drop-in once challenges ship; meanwhile the fields are `#[serde(default)]` so rmp-serde forward-compat holds.
- **Revenue-share bps**: `fee_share_bps: u16`. A feed can be paid a cut of the market's trading fees as a signing incentive.
- **Authorization scoping**: `authorized_categories: Vec<String>`, `authorized_market_ids: Vec<MarketId>`, `authorized_template_ids: Vec<TemplateId>`. Lets an operator declare "this feed can only resolve sports markets" without building a new policy variant.

All additive. None in use today. `FeedRegistry` persists via the `DATA_FEEDS` redb table.

## ResolutionPolicy variants

```rust
pub enum ResolutionPolicy {
    Immediate { feed_id: FeedId },   // SYB-23 (shipped)
    Optimistic { ... },              // designed
    Quorum { ... },                  // designed
    Predicate { ... },               // designed
    External { ... },                // designed
}
```

### `Immediate { feed_id }` (shipped)

One attestation from the named feed settles the market with no challenge window. Used for `admin_immediate` (dev) and `polymarket_mirror` (SYB-23 end-to-end).

### `Optimistic { proposer_feed, challenge_window_ms, bond_schedule, terminal_authority, max_tiers }`

Anyone registered as a feed can propose. Challenges escalate via geometric bond doubling (`bond × 2` per tier) up to `max_tiers`. Exact-match bond resolution: when the market ultimately settles, every bond from the *losing* side is fully slashed and split across the winning side proportional to each tier's stake. `terminal_authority` is either `Admin` or a pre-named panel; when the last tier is posted, they break the tie.

State transitions: `Active → Proposed → (Challenged →)* Resolved`. Trading freezes in `Proposed` via a single-line change to `MarketStatus::is_tradeable()`.

### `Quorum { feeds: Vec<FeedId>, threshold: u16 }`

Requires k-of-n attestations, each with matching `market_id` / `payout_nanos`. Order-independent (attestations can arrive in any order). Mechanically the same as `Immediate` once threshold is reached — the sequencer accumulates attestations into a small table keyed by `(market_id, payout_nanos)` and fires once it meets `threshold`.

### `Predicate { feed_id: FeedId, predicate: Predicate }`

Thick feeds publish *typed values* (`Price(Nanos)`, `Outcome(u8)`, `Bool`) rather than per-market payouts. Markets declare predicates over those values and the evaluator derives the payout. One `ETH/USD` feed resolves thousands of markets.

`Predicate` is a deliberately small, non-Turing-complete enum: `>`, `<`, `==`, `AtTime(ms)`, `And`, `Or`, `Not`. No VM, no dynamic dispatch. The predicate itself lives in `ResolutionConfig::predicate` — same storage path as `resolution_config.template`.

### `External { bridge_feed_id: FeedId }`

Mechanically identical to `Immediate` — the bridge process (UMA subscriber, Kleros subscriber, ...) translates an external system's verdict into a signed attestation and submits it. The distinction from `Immediate` is semantic: the UI can show "awaiting UMA" vs "awaiting admin", and reputation / fee-share can be partitioned by source type.

## Thick feeds and predicates

The "thick feed" model lets one `ETH/USD` feed resolve "Will ETH reach $3500 by end of Q2?", "Did ETH close above $4000 on 2026-06-15?", "Was ETH > BTC×10 at any point in April?" — without registering a separate feed per market. The feed publishes:

```rust
pub enum FeedValue {
    Price { market: String, nanos: u64 },   // "ETH-USD" → 3_450_000_000_000
    Outcome { key: String, outcome: u8 },   // "NFL-game-123" → winner idx
    Bool { key: String, value: bool },
}
```

Markets declare a predicate that evaluates against the feed's published values, plus observation windows (`AtTime`, `BetweenTimes`). The enclave stays pure — the evaluator is a small, deterministic expression interpreter, not a VM.

## Escrow / programmable balance lock

Typed primitive on accounts:

```rust
pub struct BalanceLock {
    pub amount: Nanos,
    pub release: ReleaseCondition,
}
pub enum ReleaseCondition {
    ChallengeResolved(ChallengeId),
    Timeout(u64),   // ms since epoch
    Always,
}
```

Same primitive later backs MM capital locks, vault deposits, bounty escrow. **Not built until a policy variant (Optimistic, External with bonds) actually needs it** — avoids a sequencer rewrite that only pays off when challenges ship.

## Liveness flow

Per-market fields on `ResolutionConfig`:

- `primary_deadline_ms`: when the primary policy (e.g. `Optimistic`) must have produced a resolution.
- `fallback_mode: { Admin | Permissionless | None }`: if `primary_deadline_ms` passes without resolution, either admin can settle, anyone with a bond can propose, or the market voids.
- `void_after_ms`: hard deadline. After this, the market voids regardless.
- `void_mode: { FiftyFifty | LastTradePrice }`: how the void pays out.

`Permissionless` fallback requires a proposal bond — same `BalanceLock` primitive — to deter spam.

## Trading freeze on Proposed

Single-line change when `Optimistic` ships:

```rust
// sybil-oracle/src/types.rs
pub fn is_tradeable(&self) -> bool {
    matches!(self, MarketStatus::Active)   // drop the `| Proposed { .. }`
}
```

Current `Immediate` policy never transits through `Proposed`, so SYB-23 keeps the old `Active | Proposed` rule without observable effect.

## Rich-attacker defense

The concern: a well-capitalized attacker repeatedly challenges bad-faith to force the honest side to keep doubling their bond until the honest side runs out of capital. Mitigations baked into the `Optimistic` design:

- **`max_tiers`**: caps bond escalation. After N tiers, control passes to `terminal_authority` regardless of who posted last.
- **Full slashing of the losing side**: an attacker who loses forfeits every tier they posted, split proportionally among honest bonders at each tier. Makes repeated bad-faith challenges asymptotically unprofitable.
- **Configurable terminal authority**: for high-stakes markets, `terminal_authority` can be a multisig panel rather than admin.
- **Transparency events**: every proposal, challenge, bond post, and slash emits a `system_events` entry on the block stream. Observable, replayable, diffable.

## UMA / Kleros as bridge feeds

Neither the enclave nor the main sequencer code needs to know UMA exists. A bridge process:

1. Subscribes to UMA's chain events.
2. When UMA resolves a market that maps to a Sybil market id, signs a `ResolutionAttestation`.
3. POSTs it to `/v1/markets/:id/resolve` via the `External` policy path.

The bridge is an ordinary feed from the enclave's point of view. Same pattern for Kleros, Reality.eth, anything else.

## Migration path per deferred feature

| Feature | Change required |
|---|---|
| Optimistic / challenges | New `ResolutionPolicy::Optimistic` arm. New `BalanceLock` on `Account`. New `SequencerMsg::ChallengeMarket`. Flip `is_tradeable()` to drop `Proposed`. |
| Quorum attestations | New `ResolutionPolicy::Quorum` arm. Small accumulation table `(market_id, payout_nanos) -> HashSet<FeedId>` on `MarketLifecycle`. |
| Thick feeds / predicates | New `ResolutionPolicy::Predicate` arm. New `FeedValue` publish pathway: `POST /v1/feeds/:id/values` with attested payloads. Predicate evaluator in `sybil-oracle`. |
| UMA bridge | No enclave changes. New `sybil-uma-bridge` binary that wraps UMA events as attestations. Register as a normal feed. |
| LLM resolver | Same shape as the UMA bridge: new out-of-tree binary that produces attestations. The existing `Immediate` arm suffices. Reputation counters on `DataFeed` let operators track accuracy. |
| TEE key sealing | Replace `load_or_generate_admin_pubkey` in sybil-api main.rs with an enclave-sealed key path. Feed registry and policy code stay unchanged. |
| Per-category authorization | New `DataFeed::authorized_categories` field (`#[serde(default)]`). Policy evaluators add a category check. |
| Revenue share / fees | New `DataFeed::fee_share_bps` field. Fee routing in `settlement::resolve_market`. |

**Goal: SYB-23's groundwork does not paint anything into a corner.** Every future feature is a new arm or a new row field, never a call-graph refactor.

## See Also

- [[Oracle Lifecycle]] — the current (minimal) resolution state machine
- [[Market Resolution]] — payout semantics
- [[Settlement]] — sequencer-side execution
- [[P256 Authentication]] — the signing/verification primitive shared with orders
