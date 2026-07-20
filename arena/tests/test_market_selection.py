import logging
import time
from dataclasses import dataclass, field

from live.market_selection import (
    DEFAULT_IMPORTANT_NEWS_MARKETS,
    is_important_news_market,
    select_markets,
    select_synthetic_markets,
)
from live.runner import _select_markets_resilient
from sybil_client.types import NANOS_PER_DOLLAR


@dataclass
class FakeMarket:
    id: int
    name: str
    tags: list[str]
    volume_dollars_value: float = 100_000.0
    yes_price_value: float = 0.5
    category: str = ""
    status: str = "active"
    closed: bool = False
    expiry_timestamp_ms: int = 0
    reference_price_nanos: int | None = None
    reference_price_expires_at_ms: int | None = None
    volume_nanos: int = field(init=False)

    def __post_init__(self):
        self.volume_nanos = int(self.volume_dollars_value * NANOS_PER_DOLLAR)

    @property
    def yes_price(self) -> float:
        return self.yes_price_value

    @property
    def volume_dollars(self) -> float:
        return self.volume_nanos / NANOS_PER_DOLLAR


def market(name: str, *tags: str, id: int = 1) -> FakeMarket:
    return FakeMarket(id=id, name=name, tags=["polymarket", *tags])


def test_important_news_includes_discretionary_news_markets():
    assert is_important_news_market(
        market("Will the US and Iran sign a peace deal by December 31, 2099?", "Geopolitics")
    )
    assert is_important_news_market(
        market("Will Cursor be acquired before 2027?", "Tech", "Business")
    )
    assert is_important_news_market(
        market("Next French Presidential Election: Candidate A", "Elections")
    )


def test_important_news_excludes_sports_feeds_and_speaker_markets():
    assert not is_important_news_market(market("NBA Finals winner", "Sports", "Politics"))
    assert not is_important_news_market(
        market("Will Bitcoin hit $120k before July?", "Finance")
    )
    assert not is_important_news_market(
        market('Will Trump say "recession" before July?', "Politics")
    )
    assert not is_important_news_market(market("Will oil hit $100 before 2027?", "World"))


def test_important_news_profile_uses_default_limit():
    markets = [
        market(f"Will country {i} sign a peace deal?", "Geopolitics", id=i)
        for i in range(DEFAULT_IMPORTANT_NEWS_MARKETS + 10)
    ]

    selected = select_markets(markets, max_n=0, profile="important-news")

    assert len(selected) == DEFAULT_IMPORTANT_NEWS_MARKETS


def test_important_news_profile_limits_large_groups():
    markets = [
        market(f"Important Election: Candidate {i}", "Elections", id=i) for i in range(10)
    ]
    markets.extend(
        [
            market("Standalone peace deal", "Geopolitics", id=100),
            market("Standalone acquisition", "Business", id=101),
        ]
    )

    selected = select_markets(markets, max_n=8, profile="important-news")
    grouped = [m for m in selected if m.name.startswith("Important Election:")]

    assert len(grouped) == 3
    assert len(selected) == 5


def test_important_news_profile_limits_repeated_standalone_templates():
    markets = [
        market(f"Will company {i} be acquired before 2027?", "Business", id=i)
        for i in range(10)
    ]

    selected = select_markets(markets, max_n=8, profile="important-news")

    assert len(selected) == 3


def test_all_profile_excludes_expired_markets():
    expired = market("Will the US and Iran sign a peace deal by January 1, 2000?", id=1)
    expired.expiry_timestamp_ms = int(time.time() * 1000) - 1

    selected = select_markets([expired], max_n=0, profile="all")

    assert selected == []


def test_selection_excludes_closed_markets():
    closed = market("Closed market", "Politics", id=1)
    closed.closed = True

    assert select_markets([closed], max_n=0, profile="all") == []
    assert select_markets([closed], max_n=10, profile="important-news") == []


def test_synthetic_selection_covers_native_and_fresh_mirror_only():
    native = FakeMarket(id=1, name="Native", tags=["native"])
    mirror = market("Mirror", "Politics", id=2)
    mirror.reference_price_nanos = 500_000_000
    unknown = FakeMarket(
        id=3,
        name="Unknown provenance",
        tags=[],
    )
    stale_mirror = market("Stale mirror", "Politics", id=4)
    stale_mirror.reference_price_nanos = 500_000_000
    stale_mirror.reference_price_expires_at_ms = int(time.time() * 1000) - 1
    closed_native = FakeMarket(
        id=5,
        name="Closed native",
        tags=["native"],
        closed=True,
    )

    selected = select_synthetic_markets(
        [stale_mirror, unknown, mirror, closed_native, native]
    )

    assert [m.id for m in selected] == [1, 2]


def test_selection_skips_expired_markets():
    expired = market("Will the US and Iran sign a peace deal by May 31, 2000?", "Geopolitics")
    expired.expiry_timestamp_ms = int(time.time() * 1000) - 1
    live = market(
        "Will the US and Iran sign a peace deal by December 31, 2099?",
        "Geopolitics",
        id=2,
    )
    live.expiry_timestamp_ms = int(time.time() * 1000) + 86_400_000

    selected = select_markets([expired, live], max_n=10, profile="important-news")

    assert [m.id for m in selected] == [2]


def test_selection_skips_title_dates_that_have_passed_without_api_expiry():
    expired = market(
        "Will the US and Iran sign a peace deal by January 1, 2000?",
        "Geopolitics",
    )
    live = market("Will the US and Iran sign a peace deal before 2027?", "Geopolitics", id=2)

    selected = select_markets([expired, live], max_n=10, profile="important-news")

    assert [m.id for m in selected] == [2]


def test_api_expiry_is_authoritative_over_title_heuristic():
    # Title says a past date, but the API expiry is in the future → keep it.
    live_despite_title = market(
        "Will the US and Iran sign a peace deal by January 1, 2000?",
        "Geopolitics",
        id=1,
    )
    live_despite_title.expiry_timestamp_ms = int(time.time() * 1000) + 86_400_000
    # Title says a future date, but the API expiry has passed → drop it.
    expired_despite_title = market(
        "Will the US and Iran sign a peace deal by December 31, 2099?",
        "Geopolitics",
        id=2,
    )
    expired_despite_title.expiry_timestamp_ms = int(time.time() * 1000) - 1

    selected = select_markets(
        [live_despite_title, expired_despite_title], max_n=10, profile="important-news"
    )

    assert [m.id for m in selected] == [1]


def test_title_dates_without_year_roll_forward():
    from datetime import UTC, datetime

    from live.market_selection import _title_due_date

    today = datetime.now(UTC).date()
    # A year-less title date must never resolve into the past.
    due = _title_due_date("Will the ceasefire hold by January 1?")
    assert due is not None and due >= today

    # And such a market (no API expiry) is not treated as expired.
    m = market("Will the ceasefire hold by January 1?", "Geopolitics", id=1)
    selected = select_markets([m], max_n=10, profile="important-news")
    assert [x.id for x in selected] == [1]


def test_important_news_terms_match_on_word_boundaries():
    # AR-8: substring matching used to fire "war" on "warriors" and "ban" on
    # "urban"/"Taliban". Word-boundary matching must not.
    assert not is_important_news_market(market("Will the Warriors make the finals?"))
    assert not is_important_news_market(market("Will urban density rise?"))
    # Genuine terms still match on a word boundary.
    assert is_important_news_market(market("Will a ceasefire end the war?"))
    assert is_important_news_market(market("Will the ban take effect?"))


def test_runner_selection_failure_falls_back_to_unfiltered_set(monkeypatch, caplog):
    def broken_selector(*_args, **_kwargs):
        raise RuntimeError("selector unavailable")

    markets = [
        market("High volume mirrored market", "Politics", id=1),
        FakeMarket(id=2, name="Inactive mirrored market", tags=["polymarket"], status="closed"),
        FakeMarket(
            id=3,
            name="Higher volume mirrored market",
            tags=["polymarket"],
            volume_dollars_value=200_000,
        ),
        FakeMarket(id=4, name="Native market", tags=[], volume_dollars_value=500_000),
    ]

    monkeypatch.setattr("live.runner.select_markets", broken_selector)
    with caplog.at_level(logging.WARNING):
        selected = _select_markets_resilient(markets, max_n=10, profile="important-news")

    assert [m.id for m in selected] == [3, 1]
    assert "falling back to unfiltered markets" in caplog.text


def test_selection_can_require_reference_prices():
    without_ref = market("Will the US and Iran sign a peace deal before 2027?", "Geopolitics")
    with_ref = market(
        "Will the US and Iran sign a diplomatic deal before 2027?",
        "Geopolitics",
        id=2,
    )
    with_ref.reference_price_nanos = 410_000_000

    selected = select_markets(
        [without_ref, with_ref],
        max_n=10,
        profile="important-news",
        require_reference_price=True,
    )

    assert [m.id for m in selected] == [2]


def test_selection_scores_reference_price_before_first_clear():
    far = market("Important Election: Candidate far", "Elections", id=1)
    far.yes_price_value = 0.0
    far.reference_price_nanos = 50_000_000
    near = market("Important Election: Candidate near", "Elections", id=2)
    near.yes_price_value = 0.0
    near.reference_price_nanos = 490_000_000

    selected = select_markets(
        [far, near],
        max_n=1,
        profile="important-news",
        require_reference_price=True,
    )

    assert [m.id for m in selected] == [2]
