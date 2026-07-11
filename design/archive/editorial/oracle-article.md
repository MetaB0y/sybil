# What Did You Actually Pay For?

In June 2025, Polymarket ran a market on a question a child could adjudicate: *would Volodymyr Zelenskyy wear a suit before July?* He appeared at a NATO summit in a dark, tailored, suit-like jacket. Photographs everywhere. And the market resolved **No** [VERIFY: confirm the exact resolution outcome and date before publishing]. The dispute wasn't about the facts on the ground — it was about who got to say what the facts meant, and the people who got to say it did not agree with the people looking at the pictures.

That is the whole problem in one market. A prediction market is a machine for turning disagreement about the future into a single number. But the number only means something if, at the end, somebody says what happened — and everybody believes them. **You can build the fairest matching engine in the world, and it settles to zero if the oracle lies, stalls, or gets captured.**

We've written twice about the front of the exchange. [The Sniper's Tax](https://sybilpm.substack.com/p/the-snipers-tax) was about *how* you trade — continuous order books hand the fastest bot a tax on every market maker's stale quote, and batch auctions take it back. The [agents piece](#) was about *whom* you trust to forecast — an agent that can't beat "just quote the market price" is negative alpha with extra steps, and only calibration, not narrative, tells you which is which. This one is about the back of the exchange: *what you actually paid for.* Resolution. The part that decides whether your winning ticket is worth a dollar or worth nothing.

## The trust problem

Every prediction market inherits one unavoidable dependency: a fact about the world that lives outside the market. Did the strike happen. Who won the election. Was the model above the benchmark. The market cannot observe this itself. Somebody has to report it, and the instant somebody reports it, you have to ask the uncomfortable question — *why should I believe them?*

The design space for answering that has a few known shapes, and it's worth being fair to each.

**Augur** (2018) pioneered the decentralized version: REP token-holders stake on outcomes and report them, with escalating dispute rounds that can fork the whole token if a resolution is contested hard enough. It proved decentralized resolution was *possible*. It also proved it was slow, and that forcing every honest holder to babysit disputes is its own kind of tax.

**UMA's optimistic oracle** — the one Polymarket uses — is the dominant modern answer. Someone proposes an outcome with a bond. If nobody disputes it inside a liveness window, it's accepted; cheap and fast in the common case. If someone disputes, it escalates to a vote of UMA token-holders, who are paid to vote with the majority. **The optimistic model is brilliant when the truth is boring and nobody's motivated to lie — and exactly then it's least needed.** On the loud, contested, high-stakes questions — the ones with real money and real narrative pressure — the same mechanism that makes it cheap becomes a governance surface a large holder can lean on. The suit market is the canonical example, but it isn't the only one; the pattern is that a market whose *facts* are trivial can still resolve wrong because the *voting* isn't.

None of these are stupid. They are honest answers to a genuinely hard question, each optimizing for something — decentralization, cost, speed — and paying for it somewhere else. The point isn't that Sybil has escaped the trade-off. It's that we've been explicit about which trade we're making, and built the machinery so we can change our mind later without a rewrite.

## Sybil's rails: a resolution is a signed attestation

Here is the entire primitive, and it fits in a sentence: **a resolution is a signed claim — "at time T, market M pays out P" — from a registered identity, and the exchange accepts it only if the signature verifies against a key it already knows.**

Concretely, an attestation carries three fields: the market id, a `payout_nanos`, and a nonce. It's signed with a P256 key belonging to a registered *data feed* — an on-file `(pubkey, name)` identity like `admin` or `polymarket_mirror`. The sequencer, which runs inside the trusted core of the system, does no I/O of its own. It fetches nothing, polls nothing, asks no API and no model anything. Its only job is state-machine logic: look up the feed the signature belongs to, check that this feed is the one the market's policy names, check the payout is in range, check the market isn't already resolved — and settle. **Every path to changing what a market paid out is a signature check against a pre-registered key. There is no other door.**

That `payout_nanos` field is quietly doing more work than it looks. It's an integer in `[0, 1_000_000_000]` — zero to one dollar, measured in nanodollars. A binary market is just the two endpoints: `0` for NO, `1_000_000_000` for YES. But nothing in the machinery insists on the endpoints. A payout of `650_000_000` — 65 cents on the YES share, 35 on the NO — is equally legal. **Every market on Sybil is scalar-capable for free; "binary" is just the special case where the answer landed on a corner.** That matters for resolution because plenty of real questions ("what share of the vote," "which of five models leads") don't have a clean yes/no, and the settlement layer shouldn't force one.

Feeds compose. A **mirror** market — one that shadows a Polymarket contract — inherits Polymarket's resolution wholesale: a resolution actor watches Polymarket's Gamma API, and when a mirrored market reports a *clean binary settlement*, it signs an attestation with the `polymarket_mirror` feed key and submits it through the same front door as everyone else. Anything ambiguous — non-binary, UMA-disputed, voided — is deliberately skipped with a log line rather than guessed. **Mirrors don't re-adjudicate Polymarket; they faithfully repeat it, and refuse to invent an answer Polymarket hasn't given.** That's an honest inheritance: a mirror market is exactly as trustworthy as its source, and never pretends to be more.

## Criteria as code

Before any of that machinery runs, there's a discipline that costs nothing and prevents most disasters: **write the resolution criteria before you list the market, and write them the way you'd write a function — naming the exact source, the read time, the tie-break, and what happens when the world doesn't cooperate.**

Sybil's native (non-mirror) markets carry their full resolution criteria as text, and the checked-in examples are deliberately, almost tediously precise. One reads:

> *Resolves to the AI provider whose model occupies rank #1 on the Artificial Analysis 'Intelligence Index' leaderboard at [url] as read at 2026-12-31 23:59 UTC ... Tie-break: if two models share the highest integer index score, the one listed higher in AA's own page ordering wins. If Artificial Analysis discontinues the leaderboard, renames the metric, or ships a new index methodology before the read date, resolution uses the highest-numbered 'Intelligence Index' variant still displayed on that page; if no such leaderboard exists, the market voids.*

Every clause there is a bug that didn't happen. What if there's a tie? Named. What if the methodology changes mid-year — a real risk for a benchmark? Named. What if the source disappears entirely? The market voids, cleanly, instead of turning into an argument. **A resolution criterion that doesn't say what happens when the source disappears isn't a criterion — it's a promise to fight about it later.**

The strongest evidence this discipline is real is what it forbids. The catalog ships one template — a market on the #1 app by token usage on a public leaderboard — *disabled*, dark, uncreated. Not because the market is uninteresting, but because its outcome set couldn't be pinned down crisply enough to resolve without ambiguity. **The most disciplined thing an exchange can do with a question it can't resolve cleanly is refuse to list it.** A market you can't settle honestly is worse than a market you never opened.

## The LLM evaluator, and its cage

That brings us to what landed today, and I want to describe it exactly as it is: version one, off by default, and built with more suspicion of itself than enthusiasm.

The feature is automated resolution by a language model. For a native market whose source is a pollable API and whose end time has passed, a resolver fetches the source content, hands the model the market's *full* resolution criteria plus that content, and asks for a strict-JSON verdict: a payout fraction in `[0,1]`, a confidence, a reasoning string, and verbatim evidence excerpts. Then — and this is the whole design — the confidence decides the model's *authority*, not its *conclusion*:

- **≥ 0.9**: the resolver signs an attestation and *proposes* it, opening a 24-hour challenge window. The proposal is posted to an operator review board. The operator can veto it (durable — a vetoed market never gets a fresh signed proposal, only a downgrade to review-only) or approve it early. If the window elapses untouched, the *signed attestation replays through the exact same money path as a human resolution.*
- **0.7–0.9**: review queue only. No signature, nothing auto-finalizes. A human decides.
- **below 0.7, or any parse failure, fetch failure, or non-finite number**: escalate. Fail closed. The model returning prose instead of JSON, or an out-of-range payout, is treated as *no answer*, never a coin-flip.

Notice what the model is and isn't allowed to be. **The LLM is an evidence evaluator inside tight rails, not an oracle of truth.** It never resolves anything. At most it *proposes*, and a proposal is a signed attestation held behind a challenge window and an operator veto — subject to precisely the same guards as a human's, because it flows through precisely the same signed-attestation front door. There is no bypass. The model can be wrong, and the machinery around it is built on the assumption that it will be.

This is the deliberate inversion of the failure we wrote about in the agents piece. There, the danger was a model trusted with sizing — conviction compounding into a margin call. Here, the model is trusted only with *reading*, and even that verdict has to survive a day-long window and a human who can kill it with one click. **A model that resolves in hours is a bug; a model that proposes in hours behind a window you can veto is a feature. The guardrails are the difference between "fast" and "fast and wrong."**

## Honest limits

Now the part where I tell you what this isn't.

Today there is one operator. The feed identity *is* the operator — `admin` and the mirror and resolver keys all trace back to one party, and that party could, in principle, resolve a market however they liked. The signed-attestation rail makes every resolution *attributable and replayable* — every settlement is an event on the block stream, diffable after the fact — but attribution is not the same as decentralization. Right now, "who says what happened?" has one answer, and it's us.

What changes with real users is designed but not built. The oracle system has room, on paper, for an `Optimistic` policy — anyone can propose, challenges escalate through doubling bonds, a losing challenger's stake is slashed to the honest side — and for bridge feeds that turn a UMA or Kleros verdict into an attestation with no change to the core at all. The 24-hour challenge window in the LLM resolver is the first real piece of that future wearing its adult clothes early: today only the operator can veto inside it; the same window is where public disputes will eventually live. **We built the challenge window before we built the challengers, on purpose — because retrofitting a dispute period onto a market that resolved instantly is how you get the suit market.**

Which is the honest way to read the whole thing. "Resolves in hours" is only a feature if the hours buy you something. Strip the challenge window, the operator veto, the fail-closed escalation, the criteria written in advance — and "resolves in hours" just means "wrong in hours, at speed." The guardrails aren't friction bolted onto a fast oracle. They *are* the oracle. The speed is what's left over after you've made it safe to be fast.

## The through-line

Three articles, one argument. Batch auctions fix **how** you trade — they take the latency tax off the market maker's stale quote. Calibration fixes **whom** you trust to forecast — it crowns the well-measured, not the well-narrated. Resolution design fixes **what** you actually paid for — it makes settlement a signed, attributable, disputable claim instead of a decree.

A market is a chain, and it's only as strong as its weakest link. The most elegant auction in the world clears to a number that means nothing if the oracle behind it can be leaned on. We've spent a lot of words on the front of the exchange. This is the back, and it's load-bearing: **the price is a forecast, but the resolution is the truth you're forecasting toward — and if that truth is for sale, everything upstream of it was theater.**

---

*Draft for review. Facts flagged inline with [VERIFY] — chiefly the Zelenskyy-suit market's exact resolution and date — need a second pair of eyes before this ships. The oracle machinery described here is as-landed today; the `Optimistic`/bridge-feed roadmap items are designed, not built, and are called out as such. Not for publication as-is.*
