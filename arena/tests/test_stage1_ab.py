"""Focused coverage for the opt-in SYB-114 concurrent Stage 1 A/B mode."""

from copy import deepcopy
from hashlib import sha256
from unittest.mock import AsyncMock, MagicMock

import pytest

from live.analyst import llm_generation_parameters, prompt_contract_fingerprint
from live.db import DecisionDB
from live.metrics import ArenaMetrics
from live.news_feed import LiveArticle, NewsFeed
from live.personas import PERSONAS
from live.runner import (
    STAGE1_AB_MODE,
    LiveConfig,
    _create_default_live_topology,
    _create_stage1_ab_topology,
    _require_committed_genesis_hash,
    _require_new_experiment,
    _stage1_ab_configuration,
    _validate_stage1_ab_config,
    _wire_live_inputs,
)
from live.strategy import FlatStrategy, KellyStrategy
from sybil_client.types import Block

GENESIS_A = "a" * 64
GENESIS_B = "b" * 64


def _market(mid: int = 7):
    market = MagicMock()
    market.id = mid
    market.name = f"Market {mid}"
    market.description = "Description"
    market.resolution_criteria = "The event occurs before the deadline."
    market.reference_price_nanos = None
    return market


def _block() -> Block:
    return Block(
        height=2,
        parent_hash="",
        state_root="",
        fills=[],
        clearing_prices={},
        total_welfare=0,
        total_volume=0,
        orders_filled=0,
    )


@pytest.fixture
def experiment_config() -> LiveConfig:
    return LiveConfig(
        api_key="test",
        personas=["news_trader"],
        market_ids=[7, 11],
        stage1_ab_experiment_id="stage1-july",
        llm_budget_usd=5.0,
        initial_balance=500.0,
    )


async def test_default_topology_and_names_remain_unchanged(tmp_path, monkeypatch):
    config = LiveConfig(
        api_key="test",
        personas=["news_trader"],
        market_ids=[7],
    )
    assert _validate_stage1_ab_config(config) is None

    account_resolver = AsyncMock(side_effect=[101, 102])
    monkeypatch.setattr("live.runner._resolve_bot_account", account_resolver)
    db = DecisionDB(str(tmp_path / "default.db"))
    try:
        topology = await _create_default_live_topology(
            MagicMock(), db, config, [7], {7: _market()}, ArenaMetrics()
        )
    finally:
        db.close()

    assert [analyst.name for analyst in topology.analysts] == ["News Trader (Analyst)"]
    assert topology.analysts[0].prompt_contract == "stage1"
    assert [trader.name for trader in topology.traders] == [
        "News Trader (Kelly)",
        "News Trader (Flat)",
    ]
    assert isinstance(topology.traders[0].strategy, KellyStrategy)
    assert isinstance(topology.traders[1].strategy, FlatStrategy)
    assert topology.analysts[0].bus is topology.traders[0].fv_sub._bus
    assert topology.analysts[0].bus is topology.traders[1].fv_sub._bus
    assert [call.args[2:4] for call in account_resolver.await_args_list] == [
        ("news_trader", "Kelly"),
        ("news_trader", "Flat"),
    ]
    feed = NewsFeed([], api_key=None)
    _wire_live_inputs(topology.analysts, topology.traders, feed)
    assert len(feed._subscribers) == 1
    assert topology.analysts[0].news_sub is feed._subscribers[0]


@pytest.mark.parametrize("market_ids", [None, []])
def test_stage1_ab_requires_explicit_nonempty_market_cohort(market_ids):
    config = LiveConfig(
        stage1_ab_experiment_id="stage1-july",
        market_ids=market_ids,
    )
    with pytest.raises(ValueError, match="explicit nonempty --market-ids cohort"):
        _validate_stage1_ab_config(config)


@pytest.mark.parametrize(
    ("experiment_id", "market_ids", "personas", "message"),
    [
        (" ", [7], ["news_trader"], "nonempty id"),
        ("bad/id", [7], ["news_trader"], "must use"),
        ("ok", [7, 7], ["news_trader"], "duplicates"),
        ("ok", [-1], ["news_trader"], "nonnegative"),
        ("ok", [7], ["unknown"], "unknown Stage 1 A/B personas"),
    ],
)
def test_stage1_ab_identity_cohort_and_personas_are_strict(
    experiment_id, market_ids, personas, message
):
    config = LiveConfig(
        stage1_ab_experiment_id=experiment_id,
        market_ids=market_ids,
        personas=personas,
    )
    with pytest.raises(ValueError, match=message):
        _validate_stage1_ab_config(config)


async def test_stage1_ab_topology_has_isolated_durable_flat_arms(
    tmp_path, monkeypatch, experiment_config
):
    assert _validate_stage1_ab_config(experiment_config) == "stage1-july"
    account_resolver = AsyncMock(side_effect=[201, 202])
    monkeypatch.setattr("live.runner._resolve_bot_account", account_resolver)
    markets_info = {7: _market(7), 11: _market(11)}
    db = DecisionDB(str(tmp_path / "ab.db"))
    try:
        topology = await _create_stage1_ab_topology(
            MagicMock(),
            db,
            experiment_config,
            "stage1-july",
            [7, 11],
            markets_info,
            ArenaMetrics(),
        )
    finally:
        db.close()

    assert [analyst.name for analyst in topology.analysts] == [
        "News Trader [SYB-114:stage1-july:control] (Analyst)",
        "News Trader [SYB-114:stage1-july:stage1] (Analyst)",
    ]
    assert [analyst.prompt_contract for analyst in topology.analysts] == [
        "pre_stage1_control",
        "stage1",
    ]
    assert [trader.name for trader in topology.traders] == [
        "News Trader [SYB-114:stage1-july:control] (Flat)",
        "News Trader [SYB-114:stage1-july:stage1] (Flat)",
    ]
    assert all(isinstance(trader.strategy, FlatStrategy) for trader in topology.traders)
    assert topology.traders[0].strategy is not topology.traders[1].strategy
    assert topology.analysts[0].bus is not topology.analysts[1].bus
    assert topology.paired_analyst_groups == [
        (topology.analysts[0], topology.analysts[1])
    ]
    assert all(len(analyst.bus._subscribers) == 1 for analyst in topology.analysts)
    assert all(analyst.market_ids == {7, 11} for analyst in topology.analysts)
    assert all(trader.market_ids == {7, 11} for trader in topology.traders)
    assert all(analyst.llm_budget_usd == 5.0 for analyst in topology.analysts)
    assert [call.args[2:4] for call in account_resolver.await_args_list] == [
        ("syb-114-stage1-ab:stage1-july:news_trader:control", "Flat"),
        ("syb-114-stage1-ab:stage1-july:news_trader:stage1", "Flat"),
    ]


async def test_stage1_ab_prompt_contracts_differ_only_where_intended(
    tmp_path, monkeypatch, experiment_config
):
    monkeypatch.setattr("live.runner._resolve_bot_account", AsyncMock(side_effect=[201, 202]))
    market = _market()
    db = DecisionDB(str(tmp_path / "prompt.db"))
    try:
        topology = await _create_stage1_ab_topology(
            MagicMock(),
            db,
            experiment_config,
            "stage1-july",
            [7],
            {7: market},
            ArenaMetrics(),
        )
    finally:
        db.close()

    feed = MagicMock()
    feed.polymarket_prices.get_price.return_value = 0.55
    article = LiveArticle(
        url="https://example.test/a",
        title="Event update",
        source="Wire",
        published=MagicMock(),
        full_text="Evidence.",
    )
    control, stage1 = topology.analysts
    control.news_feed = feed
    stage1.news_feed = feed
    control_prompt = control._build_prompt([article], market, _block())
    stage1_prompt = stage1._build_prompt([article], market, _block())

    assert "RESTATE: [1 sentence" not in control_prompt
    assert "discount aggregator and SEO-driven summaries" not in control_prompt
    assert "RESTATE: [1 sentence" in stage1_prompt
    assert "discount aggregator and SEO-driven summaries" in stage1_prompt
    parsed = control._parse_fair_value(
        "FAIR_VALUE: 0.61\nCOUNTERCASE: c\nCONFIDENCE: 0.7\nMOTIVATION: m\nANALYSIS: a"
    )
    assert parsed is not None and parsed.restate == ""
    assert "restate_missing" not in control.parse_fallback_counts


def test_experiment_metadata_rejects_all_resumes_and_diagnoses_drift(tmp_path, experiment_config):
    db_path = tmp_path / "metadata.db"
    configuration = _stage1_ab_configuration(experiment_config, GENESIS_A)
    db = DecisionDB(str(db_path))
    first = db.ensure_experiment("stage1-july", STAGE1_AB_MODE, configuration)
    assert first["preexisting"] is False
    db.close()

    restarted = DecisionDB(str(db_path))
    try:
        second = restarted.ensure_experiment("stage1-july", STAGE1_AB_MODE, dict(configuration))
        assert second["preexisting"] is True
        assert second["started_at_utc"] == first["started_at_utc"]
        with pytest.raises(ValueError, match="window invalidated.*use a new"):
            _require_new_experiment(second)
        persisted = restarted.get_experiment("stage1-july")
        assert persisted["started_at_utc"] == first["started_at_utc"]
        assert persisted["configuration"]["market_ids"] == [7, 11]
        assert persisted["configuration"]["genesis_hash"] == GENESIS_A
        assert persisted["configuration"]["model"] == experiment_config.model_name
        assert persisted["configuration"]["llm_pause_threshold_usd_per_analyst"] == 5.0
        assert persisted["configuration"]["llm_pause_threshold_usd_per_persona"] == 10.0
        assert persisted["configuration"]["configured_llm_pause_threshold_usd_total"] == 10.0
        assert persisted["configuration"]["llm_generation_parameters"] == llm_generation_parameters()
        variants = persisted["configuration"]["variants"]
        assert [variant["prompt_contract_sha256"] for variant in variants] == [
            prompt_contract_fingerprint("pre_stage1_control"),
            prompt_contract_fingerprint("stage1"),
        ]
        assert persisted["configuration"]["persona_text_sha256"] == {
            "news_trader": sha256(PERSONAS["news_trader"]["persona"].encode("utf-8")).hexdigest()
        }
        assert persisted["configuration"]["persona_display_name_sha256"] == {
            "news_trader": sha256(PERSONAS["news_trader"]["name"].encode("utf-8")).hexdigest()
        }

        drift_cases = []
        genesis_drift = deepcopy(configuration)
        genesis_drift["genesis_hash"] = GENESIS_B
        drift_cases.append(("genesis_hash", genesis_drift))
        cohort_drift = deepcopy(configuration)
        cohort_drift["market_ids"] = [7, 12]
        drift_cases.append(("market_ids", cohort_drift))
        model_drift = deepcopy(configuration)
        model_drift["model"] = "different/model"
        drift_cases.append(("model", model_drift))
        budget_drift = deepcopy(configuration)
        budget_drift["llm_pause_threshold_usd_per_analyst"] = 6.0
        drift_cases.append(("llm_pause_threshold_usd_per_analyst", budget_drift))
        generation_drift = deepcopy(configuration)
        generation_drift["llm_generation_parameters"]["temperature"] = 0.9
        drift_cases.append(("llm_generation_parameters", generation_drift))
        prompt_drift = deepcopy(configuration)
        prompt_drift["variants"][0]["prompt_contract_sha256"] = "0" * 64
        drift_cases.append(("variants", prompt_drift))
        persona_drift = deepcopy(configuration)
        persona_drift["persona_text_sha256"]["news_trader"] = "0" * 64
        drift_cases.append(("persona_text_sha256", persona_drift))
        display_drift = deepcopy(configuration)
        display_drift["persona_display_name_sha256"]["news_trader"] = "0" * 64
        drift_cases.append(("persona_display_name_sha256", display_drift))

        for changed_key, drifted in drift_cases:
            with pytest.raises(
                ValueError,
                match=rf"{changed_key}.*refusing configuration drift",
            ):
                restarted.ensure_experiment("stage1-july", STAGE1_AB_MODE, drifted)
    finally:
        restarted.close()


@pytest.mark.parametrize(
    "health",
    [
        {"height": 0, "genesis_hash": GENESIS_A},
        {"height": 1, "genesis_hash": None},
        {"height": 1, "genesis_hash": "0" * 64},
        {"height": 1, "genesis_hash": "short"},
    ],
)
async def test_stage1_ab_requires_committed_nonempty_genesis(health):
    client = MagicMock()
    client.health = AsyncMock(return_value=health)
    with pytest.raises(ValueError, match="committed"):
        await _require_committed_genesis_hash(client)


async def test_stage1_ab_normalizes_valid_genesis_hash():
    client = MagicMock()
    client.health = AsyncMock(return_value={"height": 1, "genesis_hash": GENESIS_A.upper()})
    assert await _require_committed_genesis_hash(client) == GENESIS_A


async def test_stage1_ab_analysts_share_one_paired_feed_subscription(
    tmp_path, monkeypatch, experiment_config
):
    monkeypatch.setattr("live.runner._resolve_bot_account", AsyncMock(side_effect=[201, 202]))
    db = DecisionDB(str(tmp_path / "feed.db"))
    try:
        topology = await _create_stage1_ab_topology(
            MagicMock(),
            db,
            experiment_config,
            "stage1-july",
            [7, 11],
            {7: _market(7), 11: _market(11)},
            ArenaMetrics(),
        )
        feed = NewsFeed([], api_key=None)
        _wire_live_inputs(
            topology.analysts,
            topology.traders,
            feed,
            topology.paired_analyst_groups,
        )
    finally:
        db.close()

    assert len(feed._subscribers) == 1
    assert topology.analysts[0].news_sub is not topology.analysts[1].news_sub
    assert all(analyst.news_feed is feed for analyst in topology.analysts)
    assert all(trader.news_feed is feed for trader in topology.traders)

    article = LiveArticle(
        url="https://example.test/shared",
        title="Shared evidence",
        source="Wire",
        published=MagicMock(),
        full_text="Evidence.",
    )
    async with feed._lock:
        feed._subscribers[0]._deliver(7, article)
    control_articles = await topology.analysts[0].news_sub.drain(7)
    stage1_articles = await topology.analysts[1].news_sub.drain(7)
    assert control_articles is stage1_articles
    assert control_articles == [article]
