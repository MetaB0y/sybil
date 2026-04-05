"""Real-time news ingestion: Google News per-market feeds + LLM relevance gate.

Pipeline:
  1. Google News RSS per market (Google does broad relevance filtering)
  2. Dedup by URL
  3. LLM gate: "Is this headline relevant to {market question}? YES/NO"
     (cheap model: google/gemma-4-31b-it, ~10 tokens per call)
  4. Fetch full text for YES articles (trafilatura)
  5. Deliver to traders

Usage (standalone test):
    cd arena && uv run python -m live.news_feed --sybil-url http://172.104.31.54:3000 --duration 300
    cd arena && uv run python -m live.news_feed --sybil-url http://172.104.31.54:3000 --duration 300 --api-key $OPENROUTER_API_KEY
"""

import asyncio
import logging
import re
import urllib.parse
from collections import defaultdict, deque
from dataclasses import dataclass, field
from datetime import datetime, timezone

import feedparser
import httpx
import openai
import trafilatura

from sybil_client.types import Market

log = logging.getLogger(__name__)

GATE_MODEL = "google/gemma-4-31b-it"

# Default path where sybil-polymarket writes the mapping
DEFAULT_MAPPING_PATH = "/data/polymarket_mapping.json"
CLOB_URL = "https://clob.polymarket.com"


# --------------------------------------------------------------------------- #
# Polymarket price fetcher
# --------------------------------------------------------------------------- #
class PolymarketPrices:
    """Fetches mid prices from Polymarket CLOB REST API using the mapping file."""

    def __init__(self, mapping_path: str | None = None):
        self._mapping_path = mapping_path
        self._sybil_to_tokens: dict[int, str] = {}  # sybil_market_id -> YES token_id
        self._prices: dict[int, float] = {}  # sybil_market_id -> YES mid price
        self._loaded = False

    def _load_mapping(self):
        """Load the sybil-polymarket mapping file to get token IDs."""
        import json
        from pathlib import Path

        paths_to_try = [
            self._mapping_path,
            DEFAULT_MAPPING_PATH,
            # Local Docker volume
            str(Path.home() / "polymarket_mapping.json"),
        ]
        # Also check the Kamal/Docker volume paths
        for p in paths_to_try:
            if p and Path(p).exists():
                try:
                    data = json.loads(Path(p).read_text())
                    token_to_sybil = data.get("token_to_sybil", {})
                    for token_id, mapping in token_to_sybil.items():
                        if isinstance(mapping, list) and len(mapping) == 2:
                            sybil_id, outcome = mapping
                            if outcome == 0:  # YES token
                                self._sybil_to_tokens[sybil_id] = token_id
                    self._loaded = True
                    log.info("Loaded Polymarket mapping: %d markets from %s",
                             len(self._sybil_to_tokens), p)
                    return
                except Exception as e:
                    log.warning("Failed to load mapping from %s: %s", p, e)
        log.warning("No Polymarket mapping file found — prices unavailable")

    async def fetch_prices(self, http: httpx.AsyncClient, market_ids: list[int]) -> dict[int, float]:
        """Fetch Polymarket mid prices for the given Sybil market IDs.

        Returns dict of sybil_market_id -> YES probability (0.0-1.0).
        """
        if not self._loaded:
            self._load_mapping()

        # Collect token IDs we need
        tokens_to_fetch = []
        token_to_sybil = {}
        for mid in market_ids:
            token = self._sybil_to_tokens.get(mid)
            if token:
                tokens_to_fetch.append(token)
                token_to_sybil[token] = mid

        if not tokens_to_fetch:
            return {}

        # Batch midpoint request — returns {token_id: "price_string"}
        try:
            payload = [{"token_id": t} for t in tokens_to_fetch]
            resp = await http.post(
                f"{CLOB_URL}/midpoints",
                json=payload,
                timeout=10.0,
            )
            if resp.status_code != 200:
                log.warning("Polymarket midpoints returned %d", resp.status_code)
                return {}

            results = resp.json()  # {token_id: "0.55", ...}
            for token_id in tokens_to_fetch:
                mid_str = results.get(token_id, "0")
                mid = float(mid_str) if mid_str else 0.0
                sybil_id = token_to_sybil[token_id]
                if mid > 0:
                    self._prices[sybil_id] = mid

            log.info("Fetched %d Polymarket prices", len(self._prices))
            return dict(self._prices)

        except Exception as e:
            log.warning("Polymarket price fetch failed: %s", e)
            return dict(self._prices)  # return cached

    def get_price(self, sybil_market_id: int) -> float | None:
        """Get cached Polymarket YES price for a market."""
        return self._prices.get(sybil_market_id)


# --------------------------------------------------------------------------- #
# Data types
# --------------------------------------------------------------------------- #
@dataclass
class LiveArticle:
    url: str
    title: str
    source: str
    published: datetime
    full_text: str | None
    matched_market_ids: list[int] = field(default_factory=list)


# --------------------------------------------------------------------------- #
# Search query extraction from market names
# --------------------------------------------------------------------------- #
def build_search_query(market: Market) -> str:
    """Build a concise Google News search query from a market name.

    Examples:
        "NBA MVP : Ja Morant"          → "Ja Morant NBA MVP"
        "Next UK Prime Minister: Keir Starmer" → "Keir Starmer UK Prime Minister"
        "Will Bitcoin hit $100k by 2026?" → "Bitcoin 100k 2026"
        "F1 Drivers' Champion: Max Verstappen" → "Max Verstappen F1 Champion"
    """
    name = market.name

    # Split on common separators — the specific part (after : or -) is usually
    # the most distinctive, so put it first in the query
    for sep in [":", " - ", " – "]:
        if sep in name:
            prefix, specific = name.split(sep, 1)
            # Combine: specific part first (more unique), then prefix keywords
            query = f"{specific.strip()} {prefix.strip()}"
            break
    else:
        query = name

    # Strip filler words and punctuation
    filler = {"will", "the", "a", "an", "of", "in", "on", "to", "by", "be",
              "is", "has", "does", "do", "can", "next", "who", "what", "when"}
    words = query.split()
    words = [w.strip("?!.,-–") for w in words]
    words = [w for w in words if w and w.lower() not in filler]

    # Cap at 6 words to keep the query focused
    query = " ".join(words[:6])
    return query


# --------------------------------------------------------------------------- #
# Text extraction
# --------------------------------------------------------------------------- #
async def fetch_article_text(http: httpx.AsyncClient, url: str) -> str | None:
    """Fetch and extract article text from a URL using trafilatura."""
    try:
        resp = await http.get(
            url,
            follow_redirects=True,
            timeout=15.0,
            headers={"User-Agent": "Mozilla/5.0 (compatible; SybilBot/1.0)"},
        )
        if resp.status_code != 200:
            return None
        text = trafilatura.extract(resp.text)
        return text or None
    except Exception:
        return None


# --------------------------------------------------------------------------- #
# LLM relevance gate
# --------------------------------------------------------------------------- #
async def llm_gate_batch(
    llm_client: openai.AsyncOpenAI,
    headlines: list[str],
    market_question: str,
) -> list[bool]:
    """Batch-gate headlines: ask LLM which ones contain RECENT, ACTIONABLE news.

    Sends one LLM call with all headlines numbered. Returns list of bools.
    """
    if not headlines:
        return []

    # Cap to avoid huge prompts
    headlines = headlines[:30]

    numbered = "\n".join(f"{i+1}. {h}" for i, h in enumerate(headlines))
    prompt = (
        f'You are filtering news for a prediction market.\n'
        f'Market: "{market_question}"\n\n'
        f'For each headline below, decide: does it contain RECENT NEWS (2025-2026) '
        f'that could change the probability of this market outcome?\n'
        f'Old articles, biographical profiles, historical recaps, and betting odds '
        f'pages are NOT relevant. Only current events matter.\n\n'
        f'{numbered}\n\n'
        f'Reply with ONLY the numbers of relevant headlines, comma-separated. '
        f'If none are relevant, reply NONE.'
    )
    try:
        resp = await llm_client.chat.completions.create(
            model=GATE_MODEL,
            messages=[{"role": "user", "content": prompt}],
            temperature=0.0,
            max_tokens=100,
        )
        answer = (resp.choices[0].message.content or "").strip().upper()
        if "NONE" in answer:
            return [False] * len(headlines)

        # Parse comma-separated numbers
        import re as _re
        nums = {int(n) for n in _re.findall(r"\d+", answer)}
        return [(i + 1) in nums for i in range(len(headlines))]
    except Exception as e:
        log.warning("LLM gate error: %s", e)
        return [False] * len(headlines)


# --------------------------------------------------------------------------- #
# NewsFeed
# --------------------------------------------------------------------------- #
class NewsFeed:
    """Google News per-market feeds + LLM relevance gate."""

    def __init__(
        self,
        markets: list[Market],
        api_key: str | None = None,
        poll_interval_s: int = 300,
        max_seen: int = 10_000,
        mapping_path: str | None = None,
    ):
        self.markets = markets
        self.api_key = api_key
        self.poll_interval = poll_interval_s
        self.max_seen = max_seen
        self.polymarket_prices = PolymarketPrices(mapping_path)

        # Dedup
        self._seen_urls: deque[str] = deque(maxlen=max_seen)
        self._seen_set: set[str] = set()

        # Pending articles per market
        self._pending: dict[int, list[LiveArticle]] = defaultdict(list)
        self._lock = asyncio.Lock()

        # All articles (for DB logging)
        self._all_articles: list[LiveArticle] = []

        # LLM client for the gate (cheap model)
        self._llm_client: openai.AsyncOpenAI | None = None
        if api_key:
            self._llm_client = openai.AsyncOpenAI(
                base_url="https://openrouter.ai/api/v1",
                api_key=api_key,
                timeout=openai.Timeout(30.0, connect=10.0),
                max_retries=0,
            )

        # Build per-market Google News RSS URLs with better queries
        self._market_feeds: list[tuple[Market, str]] = []
        for m in markets:
            query = build_search_query(m)
            encoded = urllib.parse.quote_plus(query)
            url = f"https://news.google.com/rss/search?q={encoded}&hl=en&gl=US&ceid=US:en"
            self._market_feeds.append((m, url))
            log.debug("Market [%d] query: %s", m.id, query)

    def _mark_seen(self, url: str) -> bool:
        """Returns True if URL is new (not seen before)."""
        if url in self._seen_set:
            return False
        self._seen_set.add(url)
        self._seen_urls.append(url)
        while len(self._seen_set) > self.max_seen:
            old = self._seen_urls.popleft()
            self._seen_set.discard(old)
        return True

    async def _fetch_feed(self, http: httpx.AsyncClient, url: str) -> list[dict]:
        """Fetch and parse a single RSS feed."""
        try:
            resp = await http.get(
                url, timeout=15.0, follow_redirects=True,
                headers={"User-Agent": "Mozilla/5.0 (compatible; SybilBot/1.0)"},
            )
            if resp.status_code != 200:
                return []
            feed = feedparser.parse(resp.text)
            entries = []
            for entry in feed.entries:
                link = entry.get("link", "")
                title = entry.get("title", "")
                published = entry.get("published_parsed")
                pub_dt = (
                    datetime(*published[:6], tzinfo=timezone.utc)
                    if published
                    else datetime.now(timezone.utc)
                )
                source = entry.get("source", {}).get("title", "")
                entries.append({
                    "url": link,
                    "title": title,
                    "source": source or "google",
                    "published": pub_dt,
                })
            return entries
        except Exception as e:
            log.warning("Feed fetch error: %s", e)
            return []

    async def _poll_once(self, http: httpx.AsyncClient) -> int:
        """Poll Google News for each market, batch-gate with LLM, fetch text."""
        new_count = 0
        gate_yes = 0
        gate_no = 0

        # Fetch all market feeds concurrently
        feed_tasks = [
            self._fetch_feed(http, url) for _, url in self._market_feeds
        ]
        feed_results = await asyncio.gather(*feed_tasks, return_exceptions=True)

        # Group candidates by market (dedup first)
        per_market: dict[int, list[dict]] = {}
        market_by_id: dict[int, Market] = {m.id: m for m in self.markets}
        for (market, _), result in zip(self._market_feeds, feed_results):
            if isinstance(result, Exception) or not isinstance(result, list):
                continue
            for entry in result:
                url = entry["url"]
                if url and self._mark_seen(url):
                    per_market.setdefault(market.id, []).append(entry)

        total_candidates = sum(len(v) for v in per_market.values())
        if total_candidates == 0:
            return 0

        log.info("Poll: %d new candidates across %d markets",
                 total_candidates, len(per_market))

        # Batch LLM gate per market (1 LLM call per market, not per article)
        for market_id, entries in per_market.items():
            market = market_by_id[market_id]
            headlines = [e["title"] for e in entries]

            if self._llm_client:
                results = await llm_gate_batch(
                    self._llm_client, headlines, market.name,
                )
                passed = [(e, r) for e, r in zip(entries, results) if r]
                gate_yes += len(passed)
                gate_no += len(entries) - len(passed)
            else:
                # No API key → pass everything (testing mode)
                passed = [(e, True) for e in entries]

            # Fetch full text only for articles that passed the gate
            for entry, _ in passed:
                full_text = await fetch_article_text(http, entry["url"])

                article = LiveArticle(
                    url=entry["url"],
                    title=entry["title"],
                    source=entry["source"],
                    published=entry["published"],
                    full_text=full_text,
                    matched_market_ids=[market_id],
                )

                async with self._lock:
                    self._pending[market_id].append(article)
                    self._all_articles.append(article)

                log.info(
                    "✓ [%d] %s → \"%s\" [%s]",
                    market_id, market.name[:30], entry["title"][:50], entry["source"],
                )
                new_count += 1

        if self._llm_client:
            log.info(
                "Gate: %d YES, %d NO (%.0f%% pass rate)",
                gate_yes, gate_no,
                100 * gate_yes / max(gate_yes + gate_no, 1),
            )

        return new_count

    async def run(self):
        """Poll feeds continuously."""
        gate_status = "enabled" if self._llm_client else "DISABLED (no API key)"
        log.info(
            "NewsFeed started: %d markets, poll every %ds, LLM gate %s",
            len(self.markets), self.poll_interval, gate_status,
        )
        async with httpx.AsyncClient() as http:
            while True:
                try:
                    # Fetch Polymarket reference prices each cycle
                    market_ids = [m.id for m in self.markets]
                    await self.polymarket_prices.fetch_prices(http, market_ids)

                    n = await self._poll_once(http)
                    if n > 0:
                        log.info("Poll complete: %d relevant articles delivered", n)
                except Exception as e:
                    log.error("Poll error: %s", e)
                await asyncio.sleep(self.poll_interval)

    async def drain(self, market_id: int) -> list[LiveArticle]:
        """Pop all pending articles for a market."""
        async with self._lock:
            articles = self._pending.pop(market_id, [])
        return articles

    def drain_all_new(self) -> list[LiveArticle]:
        """Pop all new articles (for DB logging). Non-async for simplicity."""
        articles = self._all_articles[:]
        self._all_articles.clear()
        return articles


# --------------------------------------------------------------------------- #
# Standalone test
# --------------------------------------------------------------------------- #
async def _main():
    import argparse
    import sys

    sys.path.insert(0, str(__import__("pathlib").Path(__file__).parent.parent))

    parser = argparse.ArgumentParser(description="Test news feed")
    parser.add_argument("--sybil-url", default="http://172.104.31.54:3000")
    parser.add_argument("--api-key", default=None, help="OpenRouter API key for LLM gate")
    parser.add_argument("--duration", type=int, default=300, help="Run for N seconds")
    parser.add_argument("--max-markets", type=int, default=10)
    args = parser.parse_args()

    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(name)s %(message)s")

    from sybil_client import SybilClient

    async with SybilClient(args.sybil_url) as client:
        markets = await client.list_markets()
        log.info("Total markets: %d", len(markets))

        active = [
            m for m in markets
            if "polymarket" in m.tags and m.status.lower() == "active"
        ]
        active.sort(key=lambda m: (-m.volume_nanos, len(m.name)))
        active = active[: args.max_markets]
        log.info("Selected %d markets:", len(active))
        for m in active:
            query = build_search_query(m)
            log.info("  [%d] %s → query: \"%s\"", m.id, m.name[:50], query)

        feed = NewsFeed(active, api_key=args.api_key, poll_interval_s=60)

        async def _run_timed():
            await asyncio.sleep(args.duration)

        await asyncio.gather(feed.run(), _run_timed())


if __name__ == "__main__":
    asyncio.run(_main())
