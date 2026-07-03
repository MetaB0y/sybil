"""Tests for the analysis/sizing split (SYB-210).

Covers:
- PersonaAnalyst fair-value parsing (moved from the pre-split trader).
- The per-call LLM budget (AR-6) now enforced on the analyst.
- One analyst LLM call serving BOTH sizing arms (the 2N -> N cost delta).
- Two sizers of one persona receiving the SAME FairValueUpdate object/values.
- The sizer running LLM-free.
"""

from datetime import datetime, timezone
from unittest.mock import AsyncMock, MagicMock

from live.analyst import PersonaAnalyst
from live.fair_value_bus import FairValueBus, FairValueUpdate
from live.news_feed import LiveArticle
from live.trader import LiveLlmTrader
from sybil_client.types import Block


def _block(height=2):
    return Block(
        height=height,
        parent_hash="",
        state_root="",
        fills=[],
        clearing_prices={},
        total_welfare=0,
        total_volume=0,
        orders_filled=0,
    )


def _article():
    return LiveArticle(
        url="http://x/a",
        title="Something happened",
        source="src",
        published=datetime(2026, 1, 1, tzinfo=timezone.utc),
        full_text="Body text.",
    )


def _make_analyst(bus, market_ids, min_llm_interval_s=1000.0):
    news_feed = MagicMock()
    news_feed.polymarket_prices.get_price.return_value = 0.55
    news_feed.subscribe.return_value.drain = AsyncMock(return_value=[_article()])

    markets_info = {}
    for mid in market_ids:
        m = MagicMock()
        m.id = mid
        m.name = f"Market {mid}"
        m.description = ""
        m.resolution_criteria = ""
        m.reference_price_nanos = None
        markets_info[mid] = m

    return PersonaAnalyst(
        client=MagicMock(),
        news_feed=news_feed,
        bus=bus,
        api_key="test",
        persona="Test persona",
        persona_key="test",
        model_name="test-model",
        market_ids=list(market_ids),
        markets_info=markets_info,
        min_llm_interval_s=min_llm_interval_s,
        name="Test (Analyst)",
    )


def _make_sizer(bus, name, market_ids=(7,)):
    news_feed = MagicMock()
    news_feed.polymarket_prices.get_price.return_value = 0.55
    markets_info = {}
    for mid in market_ids:
        m = MagicMock()
        m.id = mid
        m.name = f"Market {mid}"
        m.reference_price_nanos = None
        markets_info[mid] = m
    sizer = LiveLlmTrader(
        client=MagicMock(),
        account_id=1,
        news_feed=news_feed,
        market_ids=list(market_ids),
        markets_info=markets_info,
        name=name,
        fair_value_bus=bus,
    )
    sizer.balance_history = [500.0]
    sizer._observed_first_block = True
    return sizer


# -- Fair-value parsing (moved from the trader) --------------------------- #

def test_parse_fair_value_tolerates_trailing_dot():
    analyst = _make_analyst(FairValueBus(), [7])
    parsed = analyst._parse_fair_value(
        "FAIR_VALUE: 0.85.\n"
        "MOTIVATION: Strong new evidence.\n"
        "ANALYSIS: The article directly updates the market."
    )
    assert parsed == (
        0.85,
        "Strong new evidence.",
        "The article directly updates the market.",
    )


def test_parse_fair_value_invalid_number_returns_none():
    analyst = _make_analyst(FairValueBus(), [7])
    assert analyst._parse_fair_value("FAIR_VALUE: 0.8.5\nMOTIVATION: bad") is None


# -- AR-6: per-call LLM budget enforced on the analyst -------------------- #

async def test_analyst_llm_budget_gates_per_call_not_per_block():
    # Several markets have fresh articles in a single block, but the min
    # interval must cap the analyst to one LLM call — not a burst.
    analyst = _make_analyst(FairValueBus(), [7, 8, 9], min_llm_interval_s=1000.0)
    analyst._observed_first_block = True
    analyst._call_llm = AsyncMock(
        return_value=("FAIR_VALUE: 0.60\nMOTIVATION: m\nANALYSIS: a", 0.1)
    )

    await analyst.on_block(_block())

    assert analyst._call_llm.call_count == 1


async def test_analyst_llm_budget_allows_one_call_per_elapsed_interval():
    analyst = _make_analyst(FairValueBus(), [7, 8], min_llm_interval_s=1000.0)
    analyst._observed_first_block = True
    analyst._call_llm = AsyncMock(
        return_value=("FAIR_VALUE: 0.60\nMOTIVATION: m\nANALYSIS: a", 0.1)
    )

    await analyst.on_block(_block())
    assert analyst._call_llm.call_count == 1

    analyst._last_llm_call = 0.0  # interval elapsed
    await analyst.on_block(_block())
    assert analyst._call_llm.call_count == 2


# -- Cost delta: one analyst call serves BOTH sizing arms (2N -> N) ------- #

async def test_one_analyst_call_serves_both_sizers():
    bus = FairValueBus(persona_key="test")
    analyst = _make_analyst(bus, [7], min_llm_interval_s=1000.0)
    analyst._observed_first_block = True
    analyst._call_llm = AsyncMock(
        return_value=("FAIR_VALUE: 0.60\nMOTIVATION: m\nANALYSIS: a", 0.1)
    )

    kelly = _make_sizer(bus, "Test (Kelly)")
    flat = _make_sizer(bus, "Test (Flat)")

    await analyst.on_block(_block())

    # The analysis LLM was called exactly once for this batch...
    assert analyst._call_llm.call_count == 1

    # ...yet BOTH sizing arms receive the update (2 sizers, 1 LLM call).
    kelly_updates = await kelly.fv_sub.drain(7)
    flat_updates = await flat.fv_sub.drain(7)
    assert len(kelly_updates) == 1
    assert len(flat_updates) == 1


async def test_both_sizers_receive_same_update_object_and_values():
    bus = FairValueBus(persona_key="test")
    analyst = _make_analyst(bus, [7], min_llm_interval_s=1000.0)
    analyst._observed_first_block = True
    analyst._call_llm = AsyncMock(
        return_value=("FAIR_VALUE: 0.73\nMOTIVATION: m\nANALYSIS: a", 0.1)
    )

    kelly = _make_sizer(bus, "Test (Kelly)")
    flat = _make_sizer(bus, "Test (Flat)")

    await analyst.on_block(_block())

    kelly_update = (await kelly.fv_sub.drain(7))[0]
    flat_update = (await flat.fv_sub.drain(7))[0]

    # Provably identical A/B inputs: same object, same fair value.
    assert kelly_update is flat_update
    assert kelly_update.fair_value == 0.73
    assert flat_update.fair_value == 0.73


# -- The sizer runs LLM-free ---------------------------------------------- #

async def test_sizer_consumes_fair_value_without_llm():
    bus = FairValueBus(persona_key="test")
    sizer = _make_sizer(bus, "Test (Kelly)")

    # The sizer has no LLM machinery at all.
    assert not hasattr(sizer, "_call_llm")
    assert not hasattr(sizer, "api_key")

    # Publish an update as the analyst would, then let the sizer consume it.
    await bus.publish(FairValueUpdate(
        market_id=7,
        persona_key="test",
        fair_value=0.66,
        motivation="m",
        analysis="a",
        articles=[_article()],
        block_height=2,
        ts=datetime(2026, 1, 1, tzinfo=timezone.utc),
    ))

    await sizer.on_block(_block())

    assert sizer.fair_values[7] == 0.66


async def test_sizer_logs_decision_per_trader_for_reader_compat(tmp_path):
    # DB semantics preserved: each sizer logs its OWN decision row (trader_name
    # = the sizer), so sybil-api's per-trader_name reader is unchanged.
    from live.db import DecisionDB

    db = DecisionDB(str(tmp_path / "decisions.db"))
    try:
        bus = FairValueBus(persona_key="test")
        kelly = _make_sizer(bus, "Test (Kelly)")
        flat = _make_sizer(bus, "Test (Flat)")
        kelly.db = db
        flat.db = db

        await bus.publish(FairValueUpdate(
            market_id=7, persona_key="test", fair_value=0.66,
            motivation="m", analysis="a", articles=[], block_height=2,
            ts=datetime(2026, 1, 1, tzinfo=timezone.utc),
        ))
        await kelly.on_block(_block())
        await flat.on_block(_block())

        rows = db.conn.execute(
            "SELECT trader_name, analysis, fair_value FROM decisions "
            "WHERE analysis = 'a' ORDER BY trader_name"
        ).fetchall()
        names = sorted(r["trader_name"] for r in rows)
        assert names == ["Test (Flat)", "Test (Kelly)"]
        assert all(r["fair_value"] == 0.66 for r in rows)
    finally:
        db.close()
