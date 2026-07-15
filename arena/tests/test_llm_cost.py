"""Tests for per-agent LLM cost accounting and budget pausing (SYB-64).

Covers:
- Cost computed from a fixture response: provider-reported cost (0% error) and
  price-table fallback when the response carries no cost.
- Budget decrement per LLM call.
- Pause-at-zero: an analyst whose budget is spent skips its turn and issues NO
  LLM call, and does not raise (composes with the SYB-185 fail-open loop).
- Accounting survives a restart: a fresh analyst reconstructs cumulative spend
  from the persisted token_usage rows.
- Per-agent llm_cost_usd metric.
"""

from datetime import datetime, timezone
from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock

from live.analyst import PersonaAnalyst
from live.costs import cost_of_call, price_from_table
from live.db import DecisionDB
from live.fair_value_bus import FairValueBus
from live.metrics import ArenaMetrics
from live.news_feed import LiveArticle
from sybil_client.types import Block


# -- fixtures ------------------------------------------------------------- #

def _block(height=2):
    return Block(
        height=height, parent_hash="", state_root="", fills=[],
        clearing_prices={}, total_welfare=0, total_volume=0, orders_filled=0,
    )


def _article():
    return LiveArticle(
        url="http://x/a", title="Something happened", source="src",
        published=datetime(2026, 1, 1, tzinfo=timezone.utc), full_text="Body.",
    )


def _fixture_response(content, prompt_tokens, completion_tokens, cost=None):
    """A stand-in for an OpenRouter chat-completion response.

    Mirrors the attributes ``_call_llm`` reads. ``cost`` populates the field
    OpenRouter attaches when usage accounting is requested.
    """
    usage = SimpleNamespace(
        prompt_tokens=prompt_tokens,
        completion_tokens=completion_tokens,
        cost=cost,
    )
    message = SimpleNamespace(content=content)
    choice = SimpleNamespace(message=message)
    return SimpleNamespace(choices=[choice], usage=usage)


def _make_analyst(bus, market_ids, *, db=None, metrics=None, llm_budget_usd=None,
                  min_llm_interval_s=1000.0):
    news_feed = MagicMock()
    news_feed.reference_prices.get_price.return_value = 0.55
    news_feed.require_reference_prices = False
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
        client=MagicMock(), news_feed=news_feed, bus=bus, api_key="test",
        persona="Test persona", persona_key="test", model_name="test-model",
        market_ids=list(market_ids), markets_info=markets_info, db=db,
        min_llm_interval_s=min_llm_interval_s, name="Test (Analyst)",
        metrics=metrics, llm_budget_usd=llm_budget_usd,
    )


# -- cost computation from a fixture response ----------------------------- #

def test_cost_uses_provider_reported_cost_when_present():
    usage = SimpleNamespace(prompt_tokens=1000, completion_tokens=500, cost=0.0123)
    cost, source = cost_of_call(usage, "deepseek/deepseek-v4-flash", 1000, 500)
    assert cost == 0.0123  # 0% error: exactly what the provider billed
    assert source == "response"


def test_cost_falls_back_to_price_table_when_no_cost():
    usage = SimpleNamespace(prompt_tokens=1_000_000, completion_tokens=1_000_000, cost=None)
    cost, source = cost_of_call(usage, "deepseek/deepseek-v4-flash", 1_000_000, 1_000_000)
    # deepseek-v4-flash table: $0.10/M in + $0.30/M out = $0.40 for 1M+1M.
    assert cost == price_from_table("deepseek/deepseek-v4-flash", 1_000_000, 1_000_000)
    assert cost == 0.40
    assert source == "price_table"


def test_unknown_model_uses_default_price():
    cost, source = cost_of_call(None, "some/unlisted-model", 1_000_000, 0)
    assert source == "price_table"
    assert cost == 1.0  # DEFAULT_PRICE_PER_M input leg


# -- budget decrement per call -------------------------------------------- #

async def test_call_llm_decrements_budget_and_logs_cost(tmp_path):
    db = DecisionDB(str(tmp_path / "d.db"))
    metrics = ArenaMetrics()
    try:
        analyst = _make_analyst(
            FairValueBus(), [7], db=db, metrics=metrics, llm_budget_usd=1.0
        )
        analyst._get_llm_client = lambda: SimpleNamespace(
            chat=SimpleNamespace(completions=SimpleNamespace(
                create=AsyncMock(return_value=_fixture_response(
                    "FAIR_VALUE: 0.6\nMOTIVATION: m\nANALYSIS: a",
                    prompt_tokens=100, completion_tokens=50, cost=0.25,
                ))
            ))
        )

        text, _ = await analyst._call_llm("prompt")

        assert "FAIR_VALUE" in text
        assert analyst.llm_spent_usd == 0.25
        assert analyst._budget_remaining() == 0.75
        # Persisted with cost + source.
        row = db.conn.execute(
            "SELECT usd_cost, cost_source, prompt_tokens FROM token_usage"
        ).fetchone()
        assert row["usd_cost"] == 0.25
        assert row["cost_source"] == "response"
        assert row["prompt_tokens"] == 100
        # Metric reflects spend.
        assert metrics.registry.get_sample_value(
            "sybil_arena_llm_cost_usd_total", {"trader": "Test (Analyst)"}
        ) == 0.25
    finally:
        db.close()


# -- pause-at-zero: no LLM call, no crash --------------------------------- #

async def test_exhausted_budget_pauses_and_skips_llm_call():
    metrics = ArenaMetrics()
    analyst = _make_analyst(
        FairValueBus(), [7, 8], metrics=metrics, llm_budget_usd=1.0
    )
    analyst._observed_first_block = True
    analyst._call_llm = AsyncMock(
        return_value=("FAIR_VALUE: 0.6\nMOTIVATION: m\nANALYSIS: a", 0.1)
    )
    # Budget already spent.
    analyst.llm_spent_usd = 1.0

    await analyst.on_block(_block())

    assert analyst._call_llm.call_count == 0  # skipped the turn entirely
    assert analyst._paused is True
    assert metrics.registry.get_sample_value(
        "sybil_arena_llm_paused", {"trader": "Test (Analyst)"}
    ) == 1


async def test_pause_composes_with_failopen_run_loop():
    # Exhausting the budget is control flow, not an exception: on_block returns
    # cleanly and increments no crash counter (SYB-185 guard untouched).
    analyst = _make_analyst(FairValueBus(), [7], llm_budget_usd=0.5)
    analyst._observed_first_block = True
    analyst.llm_spent_usd = 0.5
    analyst._call_llm = AsyncMock()

    await analyst.on_block(_block())

    assert analyst.on_block_error_count == 0
    assert analyst._call_llm.call_count == 0


async def test_call_that_crosses_zero_pauses_on_next_block():
    analyst = _make_analyst(FairValueBus(), [7], llm_budget_usd=0.30)
    analyst._observed_first_block = True

    async def _spend(prompt):
        analyst.llm_spent_usd += 0.30  # this call exhausts the budget
        return ("FAIR_VALUE: 0.6\nMOTIVATION: m\nANALYSIS: a", 0.1)

    analyst._call_llm = AsyncMock(side_effect=_spend)

    await analyst.on_block(_block())
    assert analyst._call_llm.call_count == 1  # first call allowed
    assert analyst._paused is False

    await analyst.on_block(_block())
    assert analyst._call_llm.call_count == 1  # now paused; no further call
    assert analyst._paused is True


# -- accounting survives restart ------------------------------------------ #

async def test_spend_reconstructed_from_db_on_restart(tmp_path):
    db = DecisionDB(str(tmp_path / "d.db"))
    try:
        db.log_token_usage("Test (Analyst)", 100, 50, "m", 0.1, usd_cost=0.4, cost_source="response")
        db.log_token_usage("Test (Analyst)", 100, 50, "m", 0.1, usd_cost=0.7, cost_source="response")
        # Another agent's spend must not bleed in.
        db.log_token_usage("Other (Analyst)", 100, 50, "m", 0.1, usd_cost=9.0)

        assert db.get_total_llm_cost("Test (Analyst)") == 1.1

        # A fresh analyst (restart) picks up the persisted spend and pauses since
        # the reconstructed spend already exceeds a $1 budget.
        analyst = _make_analyst(FairValueBus(), [7], db=db, llm_budget_usd=1.0)
        assert analyst.llm_spent_usd == 1.1
        assert analyst._paused is True
        assert analyst._budget_exhausted() is True
    finally:
        db.close()


def test_no_budget_means_unlimited():
    analyst = _make_analyst(FairValueBus(), [7], llm_budget_usd=None)
    assert analyst._budget_remaining() is None
    assert analyst._budget_exhausted() is False
