"""Market selection profiles for live arena bots."""

from __future__ import annotations

import math
import re
import time
from calendar import monthrange
from collections import defaultdict
from collections.abc import Callable
from datetime import UTC, date, datetime
from typing import Literal, Protocol


MarketProfile = Literal["all", "important-news"]

DEFAULT_IMPORTANT_NEWS_MARKETS = 64


class MarketLike(Protocol):
    id: int
    name: str
    status: str
    category: str
    tags: list[str]
    yes_price: float
    volume_nanos: int
    volume_dollars: float
    expiry_timestamp_ms: int


INCLUDE_TAGS = {
    "ai",
    "business",
    "economy",
    "elections",
    "finance",
    "geopolitics",
    "global elections",
    "government",
    "main election",
    "politics",
    "science",
    "tech",
    "us election",
    "world",
    "world elections",
}

EXCLUDE_TAGS = {
    "awards",
    "bitcoin",
    "commodities",
    "crypto",
    "crypto prices",
    "culture",
    "earn",
    "esports",
    "hide from new",
    "hit price",
    "mentions",
    "movies",
    "music",
    "nba",
    "nfl",
    "nhl",
    "rewards",
    "soccer",
    "sports",
    "tweet markets",
    "ufc",
    "weather",
}

IMPORTANT_NEWS_TERMS = (
    "acquisition",
    "acquired",
    "antitrust",
    "ban",
    "bill",
    "ceasefire",
    "congress",
    "court",
    "deal",
    "diplomatic",
    "election",
    "government",
    "invasion",
    "ipo",
    "lawsuit",
    "merger",
    "peace",
    "prime minister",
    "president",
    "regulation",
    "resign",
    "sanction",
    "senate",
    "strike",
    "tariff",
    "trade",
    "war",
)

EXCLUDED_TITLE_PATTERNS = [
    # Single-speaker and social-media markets are usually about an utterance,
    # not a broad event whose probability changes through messy news.
    r"\b(tweet|tweets|tweeted|post on|truth social|say|says|said|mention|mentions)\b",
    r"\b(donald trump|trump|elon musk|musk|biden|putin|xi)\b.*\bannounce",
    # Clear quantitative feeds and price-threshold markets.
    r"\b(hit|touch|above|below|higher than|lower than)\s+\$?\d",
    r"\b(bitcoin|btc|ethereum|eth|solana|xrp|dogecoin|crypto)\b",
    r"\b(oil|crude|brent|wti|gold|silver)\b",
    r"\b(fed|fomc|rate cut|interest rate|cpi|inflation|unemployment|jobless|gdp)\b",
    r"\b(largest company|market cap|stock price|share price)\b",
    # Entertainment, sports, and weather markets are outside this live profile
    # even if Polymarket tags are incomplete.
    r"\b(nba|nfl|nhl|mlb|ufc|fifa|epl|champions league|world cup|super bowl)\b",
    r"\b(oscar|grammy|emmy|eurovision|movie|album|song|episode|box office)\b",
    r"\b(weather|hurricane|storm|temperature|snow|rain)\b",
]

IMPORTANT_GROUP_PATTERNS = [
    (r"\bbe acquired before\b", "acquisition-before"),
    (r"\bceasefire continue through\b", "ceasefire-continue"),
    (r"\bdiplomatic meeting by\b", "diplomatic-meeting"),
    (r"\bcloses? (its )?airspace by\b", "airspace-close"),
    (r"\bleadership change by\b", "leadership-change"),
    (r"\bregime fall before\b", "regime-fall"),
]

MONTHS = {
    "january": 1,
    "february": 2,
    "march": 3,
    "april": 4,
    "may": 5,
    "june": 6,
    "july": 7,
    "august": 8,
    "september": 9,
    "october": 10,
    "november": 11,
    "december": 12,
}

MONTH_RE = "|".join(MONTHS)
TITLE_DATE_PATTERNS = [
    re.compile(
        rf"\b(?:by|through|before|until|on)\s+({MONTH_RE})\s+(\d{{1,2}})"
        rf"(?:,\s*(\d{{4}}))?",
        re.IGNORECASE,
    ),
    re.compile(
        rf"\b(?:by|through|before|until)\s+(?:the\s+)?end of ({MONTH_RE})"
        rf"(?:\s+(\d{{4}}))?",
        re.IGNORECASE,
    ),
]


def select_markets(
    markets: list[MarketLike],
    max_n: int = 0,
    profile: MarketProfile = "all",
) -> list[MarketLike]:
    """Pick Polymarket-mirrored markets for live trading."""
    if profile == "important-news":
        active = [
            m
            for m in markets
            if "polymarket" in {_normalize_tag(t) for t in m.tags}
            and m.status.lower() == "active"
            and not _is_expired(m)
        ]
        return _select_important_news(active, max_n)

    active = [
        m for m in markets if "polymarket" in m.tags and m.status.lower() == "active"
    ]
    return _select_diverse(
        active,
        max_n,
        per_group_limit=2,
        ranking_value=_ranking_volume,
        group_key=_colon_group_key,
        group_by_size=True,
        prefer_uncertain_group_members=True,
    )


def is_important_news_market(market: MarketLike) -> bool:
    """Return whether a market fits the focused live news profile."""
    tags = _market_tags(market)
    if tags & EXCLUDE_TAGS:
        return False

    title = market.name.lower()
    if any(re.search(pattern, title) for pattern in EXCLUDED_TITLE_PATTERNS):
        return False

    has_included_tag = bool(tags & INCLUDE_TAGS)
    has_important_term = any(term in title for term in IMPORTANT_NEWS_TERMS)
    return has_included_tag or has_important_term


def _select_important_news(markets: list[MarketLike], max_n: int) -> list[MarketLike]:
    limit = max_n if max_n > 0 else DEFAULT_IMPORTANT_NEWS_MARKETS
    candidates = [m for m in markets if is_important_news_market(m)]
    return _select_diverse(
        candidates,
        limit,
        per_group_limit=3,
        ranking_value=_important_news_score,
        group_key=_important_news_group_key,
        group_by_size=False,
        prefer_uncertain_group_members=False,
    )


def _select_diverse(
    markets: list[MarketLike],
    max_n: int,
    per_group_limit: int,
    ranking_value: Callable[[MarketLike], float],
    group_key: Callable[[MarketLike], str | None],
    group_by_size: bool,
    prefer_uncertain_group_members: bool,
) -> list[MarketLike]:
    all_suitable = max_n <= 0
    standalone = []
    groups: dict[str, list[MarketLike]] = defaultdict(list)
    for market in markets:
        key = group_key(market)
        if key is None:
            standalone.append(market)
        else:
            groups[key].append(market)

    standalone.sort(key=lambda m: (-ranking_value(m), m.id))
    selected = list(standalone)

    def group_sort_value(prefix: str):
        if group_by_size:
            return (-len(groups[prefix]), prefix)
        return (-max(ranking_value(m) for m in groups[prefix]), prefix)

    for prefix in sorted(groups, key=group_sort_value):
        members = groups[prefix]
        if prefer_uncertain_group_members:
            members.sort(key=lambda m: (abs(m.yes_price - 0.5), -ranking_value(m), m.id))
        else:
            members.sort(key=lambda m: (-ranking_value(m), abs(m.yes_price - 0.5), m.id))
        selected.extend(members if all_suitable else members[:per_group_limit])

    if all_suitable:
        return selected
    return selected[:max_n]


def _important_news_score(market: MarketLike) -> float:
    title = market.name.lower()
    tags = _market_tags(market)

    score = math.log10(max(1.0, market.volume_dollars) + 1.0) * 8.0
    score += (1.0 - min(1.0, abs(market.yes_price - 0.5) * 2.0)) * 4.0
    score += len(tags & INCLUDE_TAGS) * 2.0

    for term in IMPORTANT_NEWS_TERMS:
        if term in title:
            score += 2.5

    if ":" in market.name:
        score -= 1.0
    return score


def _market_tags(market: MarketLike) -> set[str]:
    raw_tags = [market.category, *market.tags]
    return {_normalize_tag(tag) for tag in raw_tags if tag}


def _normalize_tag(tag: str) -> str:
    return re.sub(r"\s+", " ", tag.strip().lower().replace("-", " "))


def _ranking_volume(market: MarketLike) -> float:
    return market.volume_dollars if hasattr(market, "volume_dollars") else market.volume_nanos


def _is_expired(market: MarketLike) -> bool:
    expiry_ms = getattr(market, "expiry_timestamp_ms", 0)
    if expiry_ms > 0 and expiry_ms <= int(time.time() * 1000):
        return True
    due_date = _title_due_date(market.name)
    return due_date is not None and due_date < datetime.now(UTC).date()


def _colon_group_key(market: MarketLike) -> str | None:
    if ":" not in market.name:
        return None
    return market.name.split(":", 1)[0].strip()


def _important_news_group_key(market: MarketLike) -> str | None:
    title = market.name.lower()
    for pattern, key in IMPORTANT_GROUP_PATTERNS:
        if re.search(pattern, title):
            return key
    return _colon_group_key(market)


def _title_due_date(title: str) -> date | None:
    current_year = datetime.now(UTC).year

    month_day = TITLE_DATE_PATTERNS[0].search(title)
    if month_day is not None:
        month = MONTHS[month_day.group(1).lower()]
        day = int(month_day.group(2))
        year = int(month_day.group(3) or current_year)
        _, max_day = monthrange(year, month)
        return date(year, month, min(day, max_day))

    end_of_month = TITLE_DATE_PATTERNS[1].search(title)
    if end_of_month is not None:
        month = MONTHS[end_of_month.group(1).lower()]
        year = int(end_of_month.group(2) or current_year)
        _, max_day = monthrange(year, month)
        return date(year, month, max_day)

    return None
