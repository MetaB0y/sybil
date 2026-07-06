# Path to First Real Users

*Internal strategy memo — for Valery. Grounded in the repo and Linear state as of 2026-07-06. Not marketing.*

---

## 1. Honest state of the product

Sybil is a working prediction-market exchange with a real spine and no front door. The matching engine is genuinely good: frequent batch auctions cleared by a single joint Eisenberg–Gale/Fisher-market program, all-integer settlement math shared verbatim between sequencer and verifier, a four-layer verification design, and an architecture vault most funded teams never write. The backend is deployed and live on a 2GB Linode (`172-104-31-54.nip.io`), running a Polymarket mirror, a live LLM bot arena with calibration scoring, and — as of today — an LLM auto-resolver (default OFF, behind a 24h challenge window and operator veto).

What a user could do today **if the frontend were public**: browse markets (real ones mirrored from Polymarket plus a handful of native research-backed markets), watch batch auctions clear at a uniform price, watch the bot arena forecast and get scored against a market-price baseline, and — with a P256 key — place signed orders through the API. What they **could not** do: use real money (this is a devnet, balances are minted), trust settlement to anyone but us (single operator, no trustless recovery), rely on private order flow (encrypted DA / TEE is parked, so flow is not actually confidential yet), or find the app at all — **the frontend runs only via local `pnpm dev`; there is no public URL** (SYB-219, still Todo). Every "demo" today ends at "clone the repo." That single gap is the first thing between here and a first user.

## 2. The real differentiator, stated crisply

Three claimed edges. Only two are real *today*.

- **Batch-auction fairness — REAL NOW.** This is the shipped, working, defensible core. The Sniper's Tax thesis is not a slide; the solver runs, clears a window at one uniform price, and structurally removes the latency race that lets a fast trader pick off a market maker's stale quote. On a "will Israel strike today" contract, being 50ms faster buys you nothing. This is a genuine mechanism advantage over Polymarket/Kalshi's inherited CLOB, and it exists in production.
- **Agent-native design — REAL NOW, as a thesis with running proof.** The arena is live: split analyst/sizer bots, freshness decay, Kelly-vs-flat A/B, Brier-vs-market-baseline calibration, per-decision auditability. Batch auctions plus (eventual) private flow are exactly the structure where a good agent's alpha survives instead of training its own copiers. The mechanism is real; the *market* of agent builders wanting this is still a hypothesis (see §7).
- **Private validium — SOMEDAY.** This is the one to stop over-claiming internally. Privacy is not real until encrypted-DA/TEE ships (parked). Trustless settlement is not real at one operator — the honest posture is R-A operator-replacement/disaster-recovery on paper, escape cash-claim unimplemented, one set of keys (`admin`, mirror, resolver) tracing to one party. The validium is ZK-*ready*, not ZK-*private-and-trustless*. Say "someday" and mean it.

**The crisp version:** *Sybil is the prediction market where the auction is fair and agents are first-class — today — and where settlement will become private and trustless later.* Lead with the two nouns you can defend under scrutiny. The privacy/trustlessness story is a roadmap promise, and pitching it as present is how you lose your most technical early user the first time they read the code.

## 3. The wedge first user

**Pick prediction-market quant/degens frustrated by Polymarket MEV and sniping.** One beachhead, chosen because it is the only candidate whose acute pain maps directly onto the one thing Sybil already does better than anyone.

Why them:

- Their pain is **specific, current, and documented** — dudukos clearing books from 10c to 80c before MMs can cancel is a real event on a real market. Batch auctions are the direct, legible fix. You are selling aspirin, not vitamins.
- They are **sophisticated enough to perceive a mechanism advantage** without a polished UI or a brand. A degen quant will read "uniform-price batch auction, no latency game" and immediately understand what it's worth. That lowers the bar on everything Sybil is weakest at (design polish, trust, liquidity depth).
- They **already trade prediction markets**, so there is no category-education cost. You are not convincing someone that forecasting is fun; you are offering a better venue for a thing they already do.
- They **talk to each other** in tight communities (Polymarket Discord/X, MM circles). One MM who stops getting sniped is a testimonial the next ten will hear.

Why the others are worse *first*:

- **AI-agent builders** are the most strategically aligned with the long-term thesis and the eventual C·Agents Foundation milestone — but the audience barely exists yet, the value ("your alpha survives") only fully lands once private flow ships (a *someday* feature), and you'd be betting the beachhead on an unproven market. Great second wedge, wrong first wedge.
- **Forecasting hobbyists** want calibration scoreboards, which Sybil has — but they are price-insensitive, low-intent, and mostly happy on Manifold with play money. They won't push on the one mechanism that is Sybil's actual moat, and they won't generate the flow that seeds liquidity. Nice for arena PR, not a wedge.

The quant/degen is the only user for whom Sybil's *real-today* advantage is their *real-today* pain.

## 4. The gap between here and 10 real users

Ordered by what actually blocks the wedge user — a quant who wants to place a real order and not get sniped:

1. **Public frontend (SYB-219, Todo, milestone A).** Absolute blocker. No URL, no user. Everything else is moot until this ships. Note the frontend is also desktop-only (SYB-101, zero media queries, In Progress) — acceptable for a degen-on-a-laptop wedge, so don't let mobile block launch.
2. **The real-money decision.** The wedge user's pain (MEV) only bites with real money at stake; a devnet with minted balances demonstrates the mechanism but doesn't *retain* anyone. This is the fork in the road: (a) stay on minted devnet and win on "watch fair auctions + arena," acquiring users as spectators/testers, or (b) go real-money, which drags in the entire custody road (Sepolia bridge SYB-95 is still Backlog, real mainnet money invites the regulatory problem in §7). **Recommendation: devnet-with-real-markets first**, real money gated behind D·Custody Road. You can get 10 users doing real *trades* on real *questions* with fake balances if the questions are live and the arena is compelling.
3. **Security hardening for a public, authenticated surface.** The R1 "stop the bleeding" family is largely done — the dev-mode/prod boundary is real (service tier fails closed even with no token), ops surfaces are locked down, and the Grafana default-creds / permissive-CORS holes are shut. A security verification pass on 2026-07-06 confirmed those, but surfaced **two live findings** that genuinely gate a public launch, not just scaling: (a) `POST /v1/accounts/{id}/keys` is public and unauthenticated — anyone can bind a signing key to any account id and trade against the victim's balance (an account-takeover primitive; fix in flight, SYB-229); and (b) a live OpenRouter key was committed in git history (removed from tree, rotation + history-purge pending, SYB-230). **Item (a) must close before the frontend faces anyone.** Beyond these, signed-write replay protection is already in (per-account nonces + the genesis-hash domain binding landed 2026-07-06), and the remaining CI/mechanical hardening blocks scaling, not the first user.
4. **Market liquidity cold-start (§5).** The wedge user needs something to trade against. Partially answered by mirroring + house bots, but the honest gap is real.
5. **Resolution trust.** Lowest blocker for *this* wedge. A quant trading mirrored markets inherits Polymarket's resolution wholesale (honest inheritance, never worse than the source). Native markets carry criteria-as-code. Single-operator resolution is a real ceiling (§7) but not what stops the first ten sophisticated users who understand they're on a devnet.

## 5. Cold-start liquidity

The hardest problem for any new venue, and the place to be most honest.

**What Sybil actually has:**

- **Polymarket mirroring.** This is the strongest cold-start asset in the building. A mirror market inherits Polymarket's price *and* resolution. So the naive pitch — "never worse than Polymarket's price" — is *directionally* true for price discovery: the reference price is always available, so a market never has to bootstrap a number from zero.
- **House bots / the arena.** Live LLM traders and seeded noise-traders already put continuous flow into markets, so a book is never empty.

**Is "never worse than Polymarket's price" the cold-start answer?** Partly — and it's important to be precise about the limit. Mirroring solves the *price-discovery* half of cold-start (there's always a fair reference number) but not the *counterparty-depth* half. A displayed price you can't get filled at in size is not liquidity. If Sybil's book is house bots quoting around the Polymarket mid, a real quant can trade *small* against it, but the moment they want size, they're really trading against the operator's bots — which is fine for a first trade and a demo, and not fine as a business. Mirroring makes the market *legible* from block zero; it does not make it *deep*.

**Honest plan:** Use mirroring + arena bots to guarantee every market is always quotable and legible — that's enough to get the first user's first real trade. Seed depth deliberately by running house market-makers (budget-constrained, which the solver already models) tight around the mirrored mid on a chosen set of ~10–20 high-interest markets rather than spreading thin. Accept that early depth is subsidized and treat the subsidy as customer-acquisition cost, not liquidity. The batch-auction structure is a genuine tailwind here: it's the market design that makes providing that liquidity *less* toxic, so the subsidy is cheaper to sustain than it would be on a CLOB. That is the honest, self-consistent story — you're not claiming organic depth you don't have; you're claiming a structure where seeded depth survives longer.

## 6. Three concrete next moves

Ranked. Bias: the smallest thing that gets **one external user doing one real trade.**

1. **Ship the public frontend (SYB-219). Effort: days.** This is the single highest-leverage move and it's already scoped (a `frontend` service in docker-compose behind the existing Caddy on the same box is the stated simplest path). Desktop-only is fine for the wedge; do *not* block on SYB-101 mobile polish. Exit criteria: a stranger can open a URL, see live markets and the arena, and place a signed order. Until this exists, nothing in this memo can start. If you do one thing this week, this is it.
2. **Hand-recruit 3–5 wedge users and instrument the one flow. Effort: days of your time, ~S of eng.** Once the URL exists, go directly to Polymarket MM/quant communities with the Sniper's Tax article as the opener and a "come get un-snipeable" pitch. Sit each person through placing one order. Add whatever minimal onboarding/keygen friction-removal that reveals (key registration UX is the likely wall). This is founder-led sales, not growth — 5 hand-held users beat 500 waitlist signups. Sequence the landing-page-with-waitlist (SYB-36, Backlog, milestone B) *after* this, not before; a waitlist without a live product is a way to look busy.
3. **Seed 10–20 flagship markets with subsidized house depth + arena presence. Effort: ~M, mostly config.** Pick markets where the wedge user has an active view (geopolitics, crypto, AI-model races — the native "which provider is #1" markets already show the criteria-as-code discipline). Point house MMs and arena bots at them tightly (§5). Goal: when the recruited user arrives, every flagship market is quotable in modest size and the arena is visibly forecasting. This is what converts "neat demo" into "I placed a trade and it filled."

Everything past these three (real money, Sepolia bridge SYB-95, mobile, mechanical-conventions CI) is milestone B/C/D work that should wait until one real user has done one real trade and told you what actually hurt.

## 7. Risks and unknowns

- **Regulatory (flagged honestly, not as legal advice).** Real-money prediction markets are legally fraught in many jurisdictions — the CFTC/Kalshi history and Polymarket's US posture are cautionary. I am not a lawyer and this memo is not legal analysis. The strategic implication: the real-money decision in §4 is not just an engineering fork, it's a legal one, and it argues *for* the devnet-first path as a way to build product and users while the money/jurisdiction question is answered deliberately rather than by accident. **Open question: what is the minimum-viable legal posture that lets a first cohort trade real value, and in which jurisdiction?** This may gate the wedge harder than any code.
- **Single-operator trust ceiling.** Everything traces to one party's keys. For 10 sophisticated devnet users who understand this, it's fine. It becomes a hard ceiling the moment real money and non-technical users arrive — and the trustless-recovery/escape-hatch machinery (D·Custody Road) is designed-not-built. **Open question: how many real-money users can you honestly serve before "trust us" stops being acceptable, and does that number arrive before or after the custody road is walkable?**
- **Is agent-native a market or a thesis?** The arena proves the *mechanism* works. It does not prove there is a population of agent builders who will *pay to trade* on a venue chosen for alpha-survival. This is the biggest unknown behind the long-term positioning. The wedge choice in §3 deliberately hedges it: win the quant/degen beachhead on batch-auction fairness *first*, and let agent-native be the expansion once you've observed whether real agent builders show up in the arena unprompted. **Open question: do any external agent builders engage with the arena without being asked? That signal, more than any argument, tells you whether C·Agents Foundation is a business or a beautiful demo.**
- **Execution concentration.** Solo founder/operator, deploys from one laptop via `docker save | ssh`, alerting stack lives on the box it monitors. Not a strategy risk to first users directly, but it caps how fast you can respond when the first real user hits a real bug at an inconvenient hour.

---

**The one tension to hold in view:** *the users who most value what Sybil is best at (fair auctions, no MEV) feel that value most sharply with real money — which is exactly what Sybil is furthest from shipping safely (single operator, no trustless recovery, unresolved legal posture). The wedge that most wants the product can only be fully served by the thing you're least ready to do. The devnet-first path in this memo is the bet that you can earn a real cohort on the mechanism alone, and buy the time to make real money safe, before that tension forces the issue.*

---

*This is an AI-drafted strategic memo prepared for the founder's judgment. It is grounded in the repository and Linear state but reflects analysis, not decision — treat it as one structured input, not a recommendation to act on unexamined.*
