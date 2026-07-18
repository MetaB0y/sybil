"""Persona analyst: the LLM half of the live arena (SYB-210).

Extracted from ``LiveLlmTrader``. One ``PersonaAnalyst`` per persona subscribes
to the shared :class:`~live.news_feed.NewsFeed`, runs the analysis LLM once per
drained article batch, and publishes a :class:`~live.fair_value_bus.FairValueUpdate`
onto its persona's :class:`~live.fair_value_bus.FairValueBus`. The persona's two
sizing arms (Kelly, Flat) both consume that single update, so the analysis LLM
is called N times per batch instead of 2N.

The analyst holds no account and places no orders: its fair value is a pure
probability estimate, so the prompt is deliberately portfolio-agnostic (both
sizing arms have different portfolios but must share one fair value).
"""

from __future__ import annotations

import logging
import re
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from hashlib import sha256
from typing import TYPE_CHECKING, Callable, Literal

import openai

from sybil_client import Block

from .costs import cost_of_call
from .fair_value_bus import FairValueBus, FairValueUpdate
from .news_feed import LiveArticle, NewsFeed, NewsSubscription, PairedNewsBatchView
from .pricing import market_price
from .provider_health import ProviderCircuit
from .strategy import RESOLVED_HIGH, RESOLVED_LOW

if TYPE_CHECKING:
    from sybil_client import SybilClient
    from sybil_client.types import Market

    from .db import DecisionDB
    from .metrics import ArenaMetrics

log = logging.getLogger(__name__)

DEFAULT_PARSE_CONFIDENCE = 0.5
DEFAULT_COUNTERCASE = "No countercase supplied; treat this estimate cautiously."
DEFAULT_RESTATE = ""
DEDUP_SIMILARITY_THRESHOLD = 0.82
LLM_TEMPERATURE = 0.3
LLM_MAX_TOKENS = 2048
LLM_REASONING_MAX_TOKENS = 1024


def llm_generation_parameters() -> dict:
    """Return the actual immutable generation treatment used for every call."""
    return {
        "temperature": LLM_TEMPERATURE,
        "max_tokens": LLM_MAX_TOKENS,
        "reasoning": {"max_tokens": LLM_REASONING_MAX_TOKENS},
    }


# System prompt: unchanged probability-analysis framing from the pre-split
# trader. The only change vs. the old prompt is that portfolio state is no
# longer injected (the analyst has no portfolio; see _build_prompt).
SYSTEM_PROMPT = """\
You are analyzing news articles for a prediction market. Your job is to estimate the probability
of the event occurring, given the evidence.

You will be given:
- A market question
- Current market price (from Polymarket)
- Your previous fair value estimate (if any)
- One or more news articles

Respond with your probability estimate and brief reasoning. Be concise.

Key principles:
- Base your estimate on the article evidence + prior fair value, not just the market price
- If the article contains no NEW information, keep your estimate near your prior fair value
- Only revise significantly for DIRECT evidence — tangential news warrants at most 1-2 cent
  adjustment
- Official actions > direct quotes > analysis > speculation > rumors
- Prefer original primary sources or wire reporting; discount aggregator and SEO-driven summaries
- Most events have genuine uncertainty — avoid extreme probabilities unless evidence is
  extraordinary

Always respond in English regardless of article language."""

# Concurrent SYB-114 Stage 1 experiments need the actual pre-Stage-1 prompt as
# their control. Keep this as an explicit contract instead of editing the
# current prompt at runtime: the two variants must remain auditable, and the
# ordinary live path must continue to use ``SYSTEM_PROMPT`` verbatim.
PRE_STAGE1_SYSTEM_PROMPT = """\
You are analyzing news articles for a prediction market. Your job is to estimate the probability
of the event occurring, given the evidence.

You will be given:
- A market question
- Current market price (from Polymarket)
- Your previous fair value estimate (if any)
- One or more news articles

Respond with your probability estimate and brief reasoning. Be concise.

Key principles:
- Base your estimate on the article evidence + prior fair value, not just the market price
- If the article contains no NEW information, keep your estimate near your prior fair value
- Only revise significantly for DIRECT evidence — tangential news warrants at most 1-2 cent
  adjustment
- Official actions > direct quotes > analysis > speculation > rumors
- Most events have genuine uncertainty — avoid extreme probabilities unless evidence is
  extraordinary

Always respond in English regardless of article language."""

PromptContract = Literal["stage1", "pre_stage1_control"]

PRE_STAGE1_RESPONSE_CONTRACT = """FAIR_VALUE: [Your probability estimate, 0.01-0.99]
COUNTERCASE: [1 sentence — strongest reason this estimate could be wrong]
CONFIDENCE: [0.0-1.0 confidence in this fair value; lower for indirect, stale, duplicated, or conflicting evidence]
MOTIVATION: [1 sentence — why this fair value]
ANALYSIS: [2-3 sentences max — key evidence from the article(s)]"""

STAGE1_RESPONSE_CONTRACT = """RESTATE: [1 sentence — paraphrase exactly what must happen for this market to resolve YES]
FAIR_VALUE: [Your probability estimate, 0.01-0.99]
COUNTERCASE: [1 sentence — strongest reason this estimate could be wrong]
CONFIDENCE: [0.0-1.0 confidence in this fair value; lower for indirect, stale, duplicated, or conflicting evidence]
MOTIVATION: [1 sentence — why this fair value]
ANALYSIS: [2-3 sentences max — key evidence from the article(s)]"""

PROMPT_TEMPLATE = """{system_prompt}

{persona}

Market: "{market_name}"{context}

Current state:
{price_line}{last_fv_line}

Recent estimates:
{recent_fair_values}

{article_section}

Analyze and respond in this EXACT format:

{response_contract}"""


def prompt_contract_fingerprint(prompt_contract: PromptContract) -> str:
    """SHA-256 the complete static scaffold + selected system/output contract."""
    if prompt_contract == "pre_stage1_control":
        system_prompt = PRE_STAGE1_SYSTEM_PROMPT
        response_contract = PRE_STAGE1_RESPONSE_CONTRACT
    elif prompt_contract == "stage1":
        system_prompt = SYSTEM_PROMPT
        response_contract = STAGE1_RESPONSE_CONTRACT
    else:
        raise ValueError(f"unknown analyst prompt contract: {prompt_contract}")
    material = "\0".join((PROMPT_TEMPLATE, system_prompt, response_contract))
    return sha256(material.encode("utf-8")).hexdigest()


@dataclass(frozen=True)
class ParsedFairValue:
    fair_value: float
    restate: str
    motivation: str
    analysis: str
    countercase: str
    confidence: float


@dataclass(frozen=True)
class ArticleCluster:
    representative: LiveArticle
    articles: list[LiveArticle]


_ARTICLE_STOPWORDS = {
    "about",
    "after",
    "against",
    "also",
    "and",
    "are",
    "but",
    "for",
    "from",
    "has",
    "have",
    "into",
    "its",
    "not",
    "over",
    "said",
    "says",
    "that",
    "the",
    "their",
    "this",
    "was",
    "with",
    "will",
}


def _article_tokens(article: LiveArticle) -> set[str]:
    text = f"{article.title} {article.full_text or ''}"
    return {
        token
        for token in re.findall(r"[a-z0-9]{3,}", text.lower())
        if token not in _ARTICLE_STOPWORDS
    }


def _jaccard(left: set[str], right: set[str]) -> float:
    if not left or not right:
        return 0.0
    return len(left & right) / len(left | right)


def _representative_article(articles: list[LiveArticle]) -> LiveArticle:
    return max(
        articles,
        key=lambda article: (
            len(article.full_text or ""),
            len(article.title),
            article.published,
        ),
    )


def cluster_near_duplicate_articles(
    articles: list[LiveArticle],
    similarity_threshold: float = DEDUP_SIMILARITY_THRESHOLD,
) -> list[ArticleCluster]:
    """Cluster near-duplicate articles with cheap token Jaccard similarity."""
    clusters: list[tuple[list[LiveArticle], list[set[str]]]] = []
    for article in articles:
        tokens = _article_tokens(article)
        matched = None
        for cluster_articles, cluster_tokens in clusters:
            if article.url and any(article.url == existing.url for existing in cluster_articles):
                matched = (cluster_articles, cluster_tokens)
                break
            if any(
                _jaccard(tokens, existing_tokens) >= similarity_threshold
                for existing_tokens in cluster_tokens
            ):
                matched = (cluster_articles, cluster_tokens)
                break
        if matched is None:
            clusters.append(([article], [tokens]))
        else:
            matched[0].append(article)
            matched[1].append(tokens)

    return [
        ArticleCluster(representative=_representative_article(cluster), articles=cluster)
        for cluster, _tokens in clusters
    ]


class PersonaAnalyst:
    """LLM analyst for one persona: news in, FairValueUpdate out.

    Not a ``BaseAgent`` — it has no account and submits no orders. It streams
    blocks purely for cadence and a ``block_height`` stamp; the per-call LLM
    spend pause threshold (AR-6) is enforced here now that this is the only LLM caller.
    """

    def __init__(
        self,
        client: "SybilClient",
        news_feed: NewsFeed,
        bus: FairValueBus,
        api_key: str,
        persona: str,
        persona_key: str,
        model_name: str = "deepseek/deepseek-v4-flash",
        market_ids: list[int] | None = None,
        markets_info: dict[int, "Market"] | None = None,
        db: "DecisionDB | None" = None,
        min_llm_interval_s: float = 60.0,
        name: str | None = None,
        metrics: "ArenaMetrics | None" = None,
        llm_budget_usd: float | None = None,
        prompt_contract: PromptContract = "stage1",
        now_fn: Callable[[], datetime] | None = None,
        monotonic_fn: Callable[[], float] | None = None,
    ):
        self.client = client
        self.news_feed = news_feed
        self.bus = bus
        self.api_key = api_key
        self.persona = persona
        self.persona_key = persona_key
        self.model_name = model_name
        self.market_ids = set(market_ids) if market_ids else None
        self.markets_info = markets_info or {}
        self.db = db
        self.min_llm_interval_s = min_llm_interval_s
        self.name = name or "PersonaAnalyst"
        self.metrics = metrics
        if prompt_contract not in ("stage1", "pre_stage1_control"):
            raise ValueError(f"unknown analyst prompt contract: {prompt_contract}")
        self.prompt_contract = prompt_contract
        self._now = now_fn or (lambda: datetime.now(timezone.utc))
        self._monotonic = monotonic_fn or time.monotonic
        self.provider = ProviderCircuit(
            self.name,
            metrics,
            monotonic_fn=self._monotonic,
        )

        # SYB-64: per-agent LLM pause threshold. The analyst is the persona's sole LLM
        # caller (SYB-210), so this threshold is separate from the sizers'
        # trading bankroll — the analyst holds no trading account. When it hits
        # $0 the analyst PAUSES before its next call. The completed call that
        # crosses the threshold is still billed, so this is not a hard ceiling.
        # Once paused it stops issuing LLM calls, so it publishes no
        # new fair values and the persona's two sizers idle on stale FV (they
        # place no new news-driven orders). Other personas' analysts are
        # unaffected. ``None`` disables the threshold (unlimited).
        self.llm_budget_usd = llm_budget_usd
        # Reconstruct cumulative spend from persisted rows so the threshold and the
        # pause decision survive an arena restart (SYB-64 acceptance).
        self.llm_spent_usd = self.db.get_total_llm_cost(self.name) if self.db is not None else 0.0
        self._paused = False
        if self.llm_budget_usd is not None:
            self._paused = self.llm_spent_usd >= self.llm_budget_usd
            if self.metrics is not None:
                self.metrics.record_llm_cost(
                    self.name, 0.0, self.llm_budget_usd - self.llm_spent_usd
                )
                self.metrics.set_llm_paused(self.name, self._paused)

        # Own the subscription so the analyst drains its own view of the feed.
        self.news_sub: NewsSubscription | None = (
            news_feed.subscribe(name=self.name) if news_feed is not None else None
        )

        self._llm_client: openai.AsyncOpenAI | None = None
        self._last_llm_call: float = 0.0
        self._observed_first_block = False
        self._running = False
        self.on_block_error_count = 0
        self.parse_fallback_counts: dict[str, int] = {}

        # Per-market state (analyst-local; no portfolio).
        self.fair_values: dict[int, float] = {}
        # Recent (ts, fair_value, motivation) per market, for prompt context.
        self.fv_log: dict[int, list[tuple[datetime, float, str]]] = {}

    def attach_feed_and_bus(
        self,
        feed: NewsFeed,
        bus: FairValueBus,
        news_subscription: NewsSubscription | PairedNewsBatchView | None = None,
    ) -> None:
        """Wire in the shared feed + persona bus after construction.

        Used by the runner, which builds analysts before the feed/bus exist.
        """
        self.news_feed = feed
        self.bus = bus
        self.news_sub = news_subscription or feed.subscribe(name=self.name)

    def stop(self) -> None:
        self._running = False

    # -- LLM --

    def _get_llm_client(self) -> openai.AsyncOpenAI:
        if self._llm_client is None:
            self._llm_client = openai.AsyncOpenAI(
                base_url="https://openrouter.ai/api/v1",
                api_key=self.api_key,
                timeout=openai.Timeout(60.0, connect=10.0),
                max_retries=0,
            )
        return self._llm_client

    def _budget_remaining(self) -> float | None:
        """USD remaining to the pause threshold, or None when disabled."""
        if self.llm_budget_usd is None:
            return None
        return self.llm_budget_usd - self.llm_spent_usd

    def _budget_exhausted(self) -> bool:
        remaining = self._budget_remaining()
        return remaining is not None and remaining <= 0

    def _enter_paused(self) -> None:
        """Pause the analyst once its configured spend threshold is reached (SYB-64).

        Idempotent: the owner is notified once (error-level log — the arena has
        no separate notification channel, so we do not invent one) when the
        analyst first crosses into the paused state. Composes with the SYB-185
        per-block crash guard: this is ordinary control flow, never an
        exception, so ``run``'s fail-open loop keeps streaming blocks and other
        analysts are untouched.
        """
        if not self._paused:
            self._paused = True
            log.error(
                "[%s] LLM pause threshold reached ($%.4f spent vs $%.4f threshold); "
                "PAUSING — no "
                "further LLM calls or fair-value updates until the threshold is raised",
                self.name,
                self.llm_spent_usd,
                self.llm_budget_usd,
            )
        if self.metrics is not None:
            self.metrics.set_llm_paused(self.name, True)

    async def _ack_news_batch(self, market_id: int) -> None:
        if isinstance(self.news_sub, PairedNewsBatchView):
            await self.news_sub.ack_batch(market_id)

    async def _retry_news_batch(
        self,
        market_id: int,
        articles: list[LiveArticle],
    ) -> None:
        if isinstance(self.news_sub, PairedNewsBatchView):
            await self.news_sub.retry_batch(market_id)
        elif isinstance(self.news_sub, NewsSubscription):
            await self.news_sub.requeue_front(market_id, articles)

    async def _call_llm(self, prompt: str) -> tuple[str, float]:
        llm = self._get_llm_client()
        t0 = time.monotonic()
        resp = await llm.chat.completions.create(
            model=self.model_name,
            messages=[{"role": "user", "content": prompt}],
            temperature=LLM_TEMPERATURE,
            max_tokens=LLM_MAX_TOKENS,
            # SYB-64: ``usage.include`` makes OpenRouter return the actual billed
            # USD cost in ``resp.usage.cost`` (0% error vs. billing). We fall
            # back to a price table only when the field is absent.
            extra_body={
                "reasoning": {"max_tokens": LLM_REASONING_MAX_TOKENS},
                "usage": {"include": True},
            },
        )
        text = resp.choices[0].message.content or ""
        duration = time.monotonic() - t0
        if resp.usage:
            prompt_tokens = resp.usage.prompt_tokens
            completion_tokens = resp.usage.completion_tokens
            usd_cost, cost_source = cost_of_call(
                resp.usage, self.model_name, prompt_tokens, completion_tokens
            )
            # SYB-64: accumulate spend against the pause threshold and surface remaining.
            self.llm_spent_usd += usd_cost
            remaining = self._budget_remaining()
            log.info(
                "[%s] tokens: prompt=%d completion=%d cost=$%.5f (%s) spent=$%.4f (%.1fs)",
                self.name,
                prompt_tokens,
                completion_tokens,
                usd_cost,
                cost_source,
                self.llm_spent_usd,
                duration,
            )
            if self.metrics is not None:
                self.metrics.record_llm_cost(self.name, usd_cost, remaining)
            if self.db:
                # SYB-210: the analyst is now the sole LLM caller, so token cost
                # is attributed to the analyst (N rows/batch) rather than each
                # sizer (2N). sybil-api's token-usage endpoint groups by
                # trader_name, so this surfaces the persona analyst there.
                # SYB-64: usd_cost + source are persisted so spend is auditable
                # and reconstructable across restarts.
                self.db.log_token_usage(
                    self.name,
                    prompt_tokens,
                    completion_tokens,
                    self.model_name,
                    duration,
                    usd_cost,
                    cost_source,
                )
        return text, duration

    # -- Prompt building --

    def _format_recent_fair_values(self, market_id: int) -> str:
        records = self.fv_log.get(market_id, [])
        if not records:
            return "No estimates yet."
        lines = []
        for ts, fv, motivation in records[-5:]:
            t = ts.strftime("%H:%M")
            lines.append(f"- [{t}] FV={fv:.2f} | {motivation}")
        return "\n".join(lines).rstrip()

    def _build_prompt(
        self,
        articles: list[LiveArticle],
        market: "Market",
        block: Block,
        reference_price: float | None = None,
    ) -> str:
        market_id = market.id
        yes_price = (
            reference_price
            if reference_price is not None
            else market_price(self.news_feed, market_id, block)
        )
        if yes_price <= 0:
            return ""

        poly_price = (
            reference_price
            if reference_price is not None
            else self.news_feed.reference_prices.get_price(market_id)
        )
        if poly_price and poly_price > 0:
            price_line = f"- Polymarket consensus: YES=${poly_price:.4f} | NO=${1 - poly_price:.4f}"
        else:
            price_line = f"- YES price: ${yes_price:.4f} | NO price: ${1 - yes_price:.4f}"

        last_fv = self.fair_values.get(market_id)
        last_fv_line = f"\n- Your last fair value estimate: {last_fv:.2f}" if last_fv else ""

        context = ""
        if market.description:
            context += f"\n{market.description[:500]}"
        if market.resolution_criteria:
            context += f"\nResolution: {market.resolution_criteria}"

        if len(articles) == 1:
            art = articles[0]
            text = art.full_text[:3000] if art.full_text else "(text unavailable)"
            article_section = f'New article from {art.source}:\n"{art.title}"\n\n{text}'
        else:
            budget_per = max(500, 6000 // len(articles))
            parts = ["New articles this batch:\n"]
            for idx, art in enumerate(articles, 1):
                text = art.full_text[:budget_per] if art.full_text else "(text unavailable)"
                parts.append(f'[{idx}] From {art.source}: "{art.title}"\n{text}\n')
            article_section = "\n".join(parts)

        if self.prompt_contract == "pre_stage1_control":
            system_prompt = PRE_STAGE1_SYSTEM_PROMPT
            response_contract = PRE_STAGE1_RESPONSE_CONTRACT
        else:
            system_prompt = SYSTEM_PROMPT
            response_contract = STAGE1_RESPONSE_CONTRACT

        return PROMPT_TEMPLATE.format(
            system_prompt=system_prompt,
            persona=self.persona,
            market_name=market.name,
            context=context,
            price_line=price_line,
            last_fv_line=last_fv_line,
            recent_fair_values=self._format_recent_fair_values(market_id),
            article_section=article_section,
            response_contract=response_contract,
        )

    # -- LLM output parsing --

    def _record_parse_fallback(self, field: str) -> None:
        self.parse_fallback_counts[field] = self.parse_fallback_counts.get(field, 0) + 1
        if self.metrics is not None:
            self.metrics.record_analyst_parse_fallback(self.name, field)

    def _extract_section(self, text: str, label: str) -> str | None:
        labels = (
            "ANALYSIS",
            "COUNTERCASE",
            "CONFIDENCE",
            "EDGE",
            "FAIR_VALUE",
            "MOTIVATION",
            "ORDERS",
            "RESTATE",
        )
        keyword_pattern = "|".join(labels)
        match = re.search(
            rf"(?:^|\n){label}:\s*(.*?)(?=\n(?:{keyword_pattern}):|\Z)",
            text,
            re.DOTALL | re.IGNORECASE,
        )
        if not match:
            return None
        return match.group(1).strip()

    def _parse_confidence(self, text: str) -> float:
        raw = self._extract_section(text, "CONFIDENCE")
        if not raw:
            self._record_parse_fallback("confidence_missing")
            return DEFAULT_PARSE_CONFIDENCE
        match = re.search(r"[-+]?\d+(?:\.\d+)?", raw)
        if not match:
            self._record_parse_fallback("confidence_garbled")
            return DEFAULT_PARSE_CONFIDENCE
        try:
            confidence = float(match.group(0))
        except ValueError:
            self._record_parse_fallback("confidence_garbled")
            return DEFAULT_PARSE_CONFIDENCE
        if "%" in raw and 1 < confidence <= 100:
            confidence /= 100
        if not 0.0 <= confidence <= 1.0:
            self._record_parse_fallback("confidence_out_of_range")
            return DEFAULT_PARSE_CONFIDENCE
        return confidence

    def _parse_countercase(self, text: str) -> str:
        countercase = self._extract_section(text, "COUNTERCASE")
        if not countercase:
            self._record_parse_fallback("countercase_missing")
            return DEFAULT_COUNTERCASE
        return countercase

    def _parse_restate(self, text: str) -> str:
        restate = self._extract_section(text, "RESTATE")
        if not restate:
            # RESTATE did not exist in the pre-Stage-1 output contract, so its
            # absence is expected control behavior rather than a parse failure.
            if self.prompt_contract == "pre_stage1_control":
                return DEFAULT_RESTATE
            self._record_parse_fallback("restate_missing")
            return DEFAULT_RESTATE
        return restate

    def _parse_fair_value(self, text: str) -> ParsedFairValue | None:
        fv_match = re.search(r"FAIR_VALUE:\s*([\d.]+)", text, re.IGNORECASE)
        if not fv_match:
            log.warning("Failed to parse FAIR_VALUE from LLM output")
            return None
        raw_fair_value = fv_match.group(1)
        try:
            fair_value = float(raw_fair_value.rstrip("."))
        except ValueError:
            log.warning("Invalid FAIR_VALUE: %s", raw_fair_value)
            return None
        if not 0.01 <= fair_value <= 0.99:
            log.warning("FAIR_VALUE out of range: %s", fair_value)
            return None

        motivation = self._extract_section(text, "MOTIVATION") or ""
        analysis = self._extract_section(text, "ANALYSIS") or ""
        countercase = self._parse_countercase(text)
        confidence = self._parse_confidence(text)

        return ParsedFairValue(
            fair_value=fair_value,
            restate=self._parse_restate(text),
            motivation=motivation,
            analysis=analysis,
            countercase=countercase,
            confidence=confidence,
        )

    # -- Main loop --

    async def on_block(self, block: Block) -> None:
        """Drain news, run the analysis LLM (threshold-gated), publish updates."""
        now = self._now()

        if not self._observed_first_block:
            # Skip the warm-start block (the feed marks pre-existing candidates
            # seen without delivering them), matching the pre-split trader.
            self._observed_first_block = True
            return

        # SYB-64: after the spend threshold is reached, PAUSE — skip the whole block so
        # no LLM call is issued and no fair value is published. This is checked
        # before any draining/prompting, so a paused analyst does zero LLM work.
        if self._budget_exhausted():
            self._enter_paused()
            return
        if not self.provider.can_attempt():
            return

        # AR-6: the min interval is enforced per LLM *call*, not per block. A
        # single block can surface articles for many markets; stop draining once
        # the interval blocks another call and leave remaining articles pending for a later
        # block. Now that the analyst is the only LLM caller, this threshold governs
        # total analysis-LLM cost.
        for market_id in sorted(self.market_ids or []):
            elapsed_llm = self._monotonic() - self._last_llm_call
            if self._last_llm_call != 0 and elapsed_llm < self.min_llm_interval_s:
                break

            batch_reference_price = None
            if isinstance(self.news_sub, PairedNewsBatchView):
                batch = await self.news_sub.drain_batch(market_id)
                articles = batch.articles if batch is not None else []
                batch_reference_price = batch.reference_price if batch is not None else None
            else:
                articles = await self.news_sub.drain(market_id) if self.news_sub else []
            if not articles:
                continue
            raw_articles = articles
            clusters = cluster_near_duplicate_articles(articles)
            deduped_articles = [cluster.representative for cluster in clusters]
            if len(deduped_articles) < len(articles):
                log.info(
                    "[%s] deduped %d article(s) for market %d into %d evidence cluster(s)",
                    self.name,
                    len(articles),
                    market_id,
                    len(deduped_articles),
                )
            articles = deduped_articles

            market = self.markets_info.get(market_id)
            if not market:
                await self._ack_news_batch(market_id)
                continue

            ref_price = (
                batch_reference_price
                if batch_reference_price is not None
                else market_price(self.news_feed, market_id, block)
            )
            if ref_price <= 0:
                await self._retry_news_batch(market_id, raw_articles)
                continue

            # Skip resolved markets — don't waste LLM calls.
            if ref_price >= RESOLVED_HIGH or ref_price <= RESOLVED_LOW:
                if market_id in self.fair_values:
                    log.info(
                        "[%s] %s resolved (price=%.2f), clearing FV",
                        self.name,
                        market.name[:30],
                        ref_price,
                    )
                    del self.fair_values[market_id]
                await self._ack_news_batch(market_id)
                continue

            titles = "; ".join(f'"{a.title[:40]}"' for a in articles)
            log.info(
                "[%s] %d article(s) for %s (price=%.2f): %s",
                self.name,
                len(articles),
                market.name[:30],
                ref_price,
                titles,
            )

            prompt = self._build_prompt(
                articles,
                market,
                block,
                reference_price=batch_reference_price,
            )
            if not prompt:
                await self._retry_news_batch(market_id, raw_articles)
                continue

            # The interval governs attempts, not only successful responses.
            # Otherwise a transient 5xx/timeout (which deliberately has no
            # long circuit backoff) would be retried on every block.
            self._last_llm_call = self._monotonic()
            try:
                raw_text, llm_duration_s = await self._call_llm(prompt)
                self.provider.record_success()
                await self._ack_news_batch(market_id)
                if self.metrics is not None:
                    self.metrics.record_llm_call(self.name)
                log.info("[%s] LLM response (%.1fs):\n%s", self.name, llm_duration_s, raw_text)
            except Exception as exc:
                failure = self.provider.record_failure(exc)
                await self._retry_news_batch(market_id, raw_articles)
                log.error(
                    "[%s] LLM provider failure (kind=%s, backoff=%.0fs): %s",
                    self.name,
                    failure.kind,
                    failure.backoff_seconds,
                    exc,
                )
                break

            parsed = self._parse_fair_value(raw_text)
            if parsed is None:
                log.warning("[%s] Failed to parse LLM output", self.name)
                continue

            fair_value = parsed.fair_value
            motivation = parsed.motivation
            analysis = parsed.analysis
            old_fv = self.fair_values.get(market_id)
            self.fair_values[market_id] = fair_value
            records = self.fv_log.setdefault(market_id, [])
            records.append((now, fair_value, motivation))
            if len(records) > 200:
                self.fv_log[market_id] = records[-200:]

            log.info(
                "[%s] %s: FV %.2f->%.2f (market=%.2f, edge=%.2f, conf=%.2f) | %s",
                self.name,
                market.name[:30],
                old_fv or 0,
                fair_value,
                ref_price,
                fair_value - ref_price,
                parsed.confidence,
                motivation,
            )

            # Broadcast to both sizing arms of this persona. Both receive the
            # SAME update object, guaranteeing identical A/B inputs (SYB-210).
            await self.bus.publish(
                FairValueUpdate(
                    market_id=market_id,
                    persona_key=self.persona_key,
                    fair_value=fair_value,
                    restate=parsed.restate,
                    motivation=motivation,
                    analysis=analysis,
                    countercase=parsed.countercase,
                    confidence=parsed.confidence,
                    articles=articles,
                    block_height=block.height,
                    ts=now,
                    analysis_reference_price=batch_reference_price,
                )
            )

    async def run(self) -> None:
        """Stream blocks for cadence; drive on_block. Fail-open per block (SYB-185)."""
        self._running = True
        async for block in self.client.stream_live_blocks():
            if not self._running:
                break
            try:
                await self.on_block(block)
            except Exception:
                self.on_block_error_count += 1
                log.exception(
                    "Analyst on_block failed; continuing: name=%s block_height=%s "
                    "on_block_error_count=%d",
                    self.name,
                    block.height,
                    self.on_block_error_count,
                )
