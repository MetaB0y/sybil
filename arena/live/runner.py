"""Live trading bot orchestrator.

Usage:
    cd arena && OPENROUTER_API_KEY=... uv run python -m live.runner
    cd arena && OPENROUTER_API_KEY=... uv run python -m live.runner --max-markets 10
"""

import argparse
import asyncio
import logging
import os
import re
import signal
from dataclasses import dataclass, field
from hashlib import sha256
from pathlib import Path

from sybil_client import SybilClient
from sybil_client.types import NANOS_PER_DOLLAR, TimeInForce

from .analyst import (
    PersonaAnalyst,
    llm_generation_parameters,
    prompt_contract_fingerprint,
)
from .db import DecisionDB
from .fair_value_bus import FairValueBus
from .market_selection import MarketProfile, select_markets
from .metrics import ArenaMetrics, start_metrics_server
from .news_feed import NewsFeed, PairedNewsBatchBarrier
from .personas import PERSONAS
from .strategy import FairValueFreshnessConfig, FlatStrategy, KellyStrategy
from .synthetic import (
    CrossingNoiseTrader,
    FastReferenceTrader,
    NativeNoiseTrader,
    SyntheticStrategyConfig,
)
from .trader import LiveLlmTrader

log = logging.getLogger(__name__)

# When --require-reference-prices is set, arena may start before the Polymarket
# mirror has published any reference prices. Rather than exit, poll for a
# reference-backed market set on this cadence until one appears.
MARKET_DISCOVERY_RETRY_SECONDS = 30
STAGE1_AB_MODE = "syb-114-stage1-ab"
STAGE1_AB_VARIANTS = (
    {
        "id": "control",
        "prompt_contract": "pre_stage1_control",
        "sizer": "Flat",
    },
    {"id": "stage1", "prompt_contract": "stage1", "sizer": "Flat"},
)


@dataclass
class LiveConfig:
    sybil_url: str = "http://172.104.31.54:3000"
    api_key: str = ""
    model_name: str = "deepseek/deepseek-v4-flash"
    initial_balance: float = 500.0
    max_markets: int = 0
    market_profile: MarketProfile = "all"
    require_reference_prices: bool = False
    order_time_in_force: TimeInForce = "IOC"
    news_poll_interval: int = 300
    min_llm_interval: float = 60.0
    fair_value_ttl_s: float = FairValueFreshnessConfig.ttl_s
    fair_value_half_life_s: float = FairValueFreshnessConfig.half_life_s
    fair_value_hard_expiry_s: float = FairValueFreshnessConfig.hard_expiry_s
    # SYB-64: per-analyst LLM pause threshold (USD). The analyst is a persona's sole LLM
    # caller and holds no trading account, so this is a separate pool from the
    # sizers' trading bankroll. Exhausting it pauses the persona's analyst.
    # A completed call may cross it before the next call is blocked. None (or
    # <=0 on the CLI) disables the threshold (unlimited).
    llm_budget_usd: float | None = 5.0
    fast_count: int = 5
    noise_count: int = 5
    noise_balance: float = 50.0
    # Zero-fills fix: aggressive two-sided crossing noise on a durable (GTC) book.
    # noise_time_in_force overrides order_time_in_force for the crossing noise
    # traders so a resting book accumulates even while LLM/fast flow stays IOC.
    noise_time_in_force: TimeInForce = "GTC"
    synthetic_strategy: SyntheticStrategyConfig = field(default_factory=SyntheticStrategyConfig)
    db_path: str = ""
    metrics_host: str = "0.0.0.0"
    metrics_port: int = 0  # <=0 disables the exporter (default: off)
    personas: list[str] = field(default_factory=lambda: list(PERSONAS.keys()))
    market_ids: list[int] | None = None  # Manual market selection (overrides auto)
    mapping_path: str | None = None  # Path to polymarket_mapping.json
    # Opt-in concurrent Stage 1 A/B. Supplying an id enables the experiment;
    # the ordinary one-analyst + Kelly/Flat topology remains the default.
    stage1_ab_experiment_id: str | None = None


@dataclass
class LiveTopology:
    analysts: list[PersonaAnalyst]
    traders: list[LiveLlmTrader]
    paired_analyst_groups: list[tuple[PersonaAnalyst, PersonaAnalyst]] = field(
        default_factory=list
    )


def _validate_stage1_ab_config(config: LiveConfig) -> str | None:
    """Validate opt-in experiment identity and its frozen market cohort."""
    if config.stage1_ab_experiment_id is None:
        return None

    experiment_id = config.stage1_ab_experiment_id
    if not experiment_id or experiment_id != experiment_id.strip():
        raise ValueError(
            "--stage1-ab-experiment-id must be a nonempty id without surrounding whitespace"
        )
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,63}", experiment_id):
        raise ValueError(
            "--stage1-ab-experiment-id must use 1-64 letters, numbers, '.', '_' or '-'"
        )
    if not config.market_ids:
        raise ValueError(
            "--stage1-ab-experiment-id requires an explicit nonempty --market-ids cohort"
        )
    if any(market_id < 0 for market_id in config.market_ids):
        raise ValueError("--market-ids must contain only nonnegative ids in Stage 1 A/B mode")
    if len(set(config.market_ids)) != len(config.market_ids):
        raise ValueError("--market-ids must not contain duplicates in Stage 1 A/B mode")
    if not config.personas:
        raise ValueError("Stage 1 A/B mode requires at least one persona")
    unknown = [persona for persona in config.personas if persona not in PERSONAS]
    if unknown:
        raise ValueError(f"unknown Stage 1 A/B personas: {', '.join(unknown)}")
    if len(set(config.personas)) != len(config.personas):
        raise ValueError("Stage 1 A/B personas must not contain duplicates")
    if config.llm_budget_usd is not None and config.llm_budget_usd <= 0:
        raise ValueError(
            "Stage 1 A/B per-analyst LLM pause threshold must be positive or unlimited"
        )
    return experiment_id


def _stage1_ab_configuration(
    config: LiveConfig,
    genesis_hash: str,
    startup_reference_prices: dict[int, float],
) -> dict:
    """Canonical immutable configuration persisted for restart validation."""
    analyst_count = 2 * len(config.personas)
    total_llm_pause_threshold = (
        None if config.llm_budget_usd is None else config.llm_budget_usd * analyst_count
    )
    sizer_count = analyst_count
    return {
        "genesis_hash": genesis_hash,
        "market_ids": sorted(config.market_ids or []),
        "startup_reference_prices": {
            str(market_id): startup_reference_prices[market_id]
            for market_id in sorted(startup_reference_prices)
        },
        "model": config.model_name,
        "llm_generation_parameters": llm_generation_parameters(),
        "variants": [
            {
                **variant,
                "prompt_contract_sha256": prompt_contract_fingerprint(variant["prompt_contract"]),
            }
            for variant in STAGE1_AB_VARIANTS
        ],
        "personas": list(config.personas),
        "persona_text_sha256": {
            persona_key: sha256(PERSONAS[persona_key]["persona"].encode("utf-8")).hexdigest()
            for persona_key in config.personas
        },
        "persona_display_name_sha256": {
            persona_key: sha256(PERSONAS[persona_key]["name"].encode("utf-8")).hexdigest()
            for persona_key in config.personas
        },
        "analyst_count": analyst_count,
        "llm_pause_threshold_usd_per_analyst": config.llm_budget_usd,
        "llm_pause_threshold_usd_per_persona": (
            None if config.llm_budget_usd is None else 2 * config.llm_budget_usd
        ),
        "configured_llm_pause_threshold_usd_total": total_llm_pause_threshold,
        "sizer_count": sizer_count,
        "initial_balance_usd_per_sizer": config.initial_balance,
        "initial_balance_usd_total": config.initial_balance * sizer_count,
        "min_llm_interval_s": config.min_llm_interval,
        "news_poll_interval_s": config.news_poll_interval,
        "order_time_in_force": config.order_time_in_force,
        "fair_value_ttl_s": config.fair_value_ttl_s,
        "fair_value_half_life_s": config.fair_value_half_life_s,
        "fair_value_hard_expiry_s": config.fair_value_hard_expiry_s,
    }


async def _require_committed_genesis_hash(client: SybilClient) -> str:
    """Return the live chain identity, rejecting uncommitted/ambiguous health."""
    health = await client.health()
    genesis_hash = str(health.get("genesis_hash") or "").strip().lower()
    height = health.get("height")
    if not isinstance(height, int) or height < 1:
        raise ValueError(
            "Stage 1 A/B requires a committed chain (health height must be at least 1)"
        )
    if not re.fullmatch(r"[0-9a-f]{64}", genesis_hash) or set(genesis_hash) == {"0"}:
        raise ValueError(
            "Stage 1 A/B requires a committed nonzero 32-byte genesis_hash from /v1/health"
        )
    return genesis_hash


def _require_new_experiment(metadata: dict) -> None:
    """Reject reuse because analyst/FV/Flat basis state cannot be reconstructed."""
    if metadata.get("preexisting"):
        raise ValueError(
            f"experiment {metadata['experiment_id']!r} already exists; window invalidated by "
            "restart, so use a new --stage1-ab-experiment-id"
        )


def _require_stage1_ab_startup_reference_prices(markets: list) -> dict[int, float]:
    """Require a positive external reference for every frozen experiment market."""
    references = {}
    missing = []
    for market in markets:
        reference_nanos = getattr(market, "reference_price_nanos", None)
        if (
            not isinstance(reference_nanos, int)
            or reference_nanos <= 0
            or reference_nanos > NANOS_PER_DOLLAR
        ):
            missing.append(int(market.id))
            continue
        references[int(market.id)] = reference_nanos / NANOS_PER_DOLLAR
    if missing:
        raise ValueError(
            "Stage 1 A/B requires a positive external startup reference for every selected "
            f"market; missing market ids: {missing}"
        )
    return references


def _env_int(name: str, default: int) -> int:
    raw = os.environ.get(name, "").strip()
    if not raw:
        return default
    return int(raw)


def _env_float(name: str, default: float) -> float:
    raw = os.environ.get(name, "").strip()
    if not raw:
        return default
    return float(raw)


def _env_bool(name: str, default: bool) -> bool:
    raw = os.environ.get(name, "").strip().lower()
    if not raw:
        return default
    return raw in ("1", "true", "yes", "on")


def _env_market_profile(name: str, default: MarketProfile = "all") -> MarketProfile:
    raw = os.environ.get(name, "").strip()
    if not raw:
        return default
    if raw in ("all", "important-news"):
        return raw
    raise ValueError(f"{name} must be one of: all, important-news")


def _fallback_unfiltered_markets(markets, max_n: int = 0, require_reference_price: bool = False):
    """Return active mirrored markets without profile scoring or grouping."""

    def is_active_mirrored(market) -> bool:
        tags = {str(tag).strip().lower().replace("-", " ") for tag in getattr(market, "tags", [])}
        if "polymarket" not in tags:
            return False
        if str(getattr(market, "status", "")).lower() != "active":
            return False
        if require_reference_price:
            ref = getattr(market, "reference_price_nanos", None)
            if ref is None or ref <= 0:
                return False
        return True

    active = [m for m in markets if is_active_mirrored(m)]
    active.sort(key=lambda m: (-getattr(m, "volume_nanos", 0), getattr(m, "id", 0)))
    if max_n <= 0:
        return active
    return active[:max_n]


def _select_markets_resilient(
    markets,
    max_n: int = 0,
    profile: MarketProfile = "all",
    require_reference_price: bool = False,
):
    try:
        return select_markets(
            markets, max_n, profile, require_reference_price=require_reference_price
        )
    except Exception as e:
        log.warning(
            "Market selection failed for profile=%s: %s; falling back to unfiltered markets",
            profile,
            e,
            exc_info=True,
        )
        return _fallback_unfiltered_markets(
            markets, max_n, require_reference_price=require_reference_price
        )


async def snapshot_portfolios_once(
    traders,
    db: DecisionDB,
    *,
    required_trader_names: set[str] | None = None,
) -> int:
    """Record one portfolio snapshot per trader, optionally failing closed."""
    recorded = 0
    failures = []
    for trader in traders:
        try:
            portfolio = await trader.client.get_portfolio(trader.account_id)
            positions = {}
            for (mid, outcome), qty in trader.positions.items():
                if qty != 0:
                    positions.setdefault(str(mid), {})[outcome] = qty
            db.log_snapshot(
                trader_name=trader.name,
                balance=portfolio.balance_dollars,
                portfolio_value=portfolio.portfolio_value_dollars,
                pnl=portfolio.pnl_dollars,
                positions=positions,
                total_fills=len(getattr(trader, "_fill_history", [])),
                total_orders=getattr(trader, "total_orders_submitted", 0),
            )
            recorded += 1
        except Exception as exc:
            failures.append((trader.name, exc))
            log.warning("Snapshot error for %s: %s", trader.name, exc)

    required = required_trader_names or set()
    required_failures = [(name, exc) for name, exc in failures if name in required]
    if required_failures:
        names = ", ".join(name for name, _exc in required_failures)
        raise RuntimeError(
            f"experiment portfolio baseline failed for {len(required_failures)} arm(s): {names}; "
            "window invalidated; use a new experiment id"
        ) from required_failures[0][1]
    return recorded


async def snapshot_portfolios(traders, db: DecisionDB, interval_s: float = 300):
    """Periodically log portfolio snapshots for all traders after each interval."""
    while True:
        await asyncio.sleep(interval_s)
        await snapshot_portfolios_once(traders, db)


async def log_articles_loop(feed: NewsFeed, db: DecisionDB, interval_s: float = 30):
    """Periodically flush new articles from the feed into the DB."""
    while True:
        await asyncio.sleep(interval_s)
        articles = feed.drain_all_new()
        for article in articles:
            db.log_article(article)


async def supervise_bot(agent, stop_event: asyncio.Event, restart_delay_s: float = 5.0):
    """Run one bot with per-task restart supervision."""
    while not stop_event.is_set():
        try:
            await agent.run()
        except asyncio.CancelledError:
            raise
        except Exception:
            if stop_event.is_set():
                break
            log.exception("Bot task %s failed unexpectedly; restarting", agent.name)
        else:
            if stop_event.is_set():
                break
            log.error("Bot task %s exited unexpectedly; restarting", agent.name)

        if not stop_event.is_set():
            try:
                await asyncio.wait_for(stop_event.wait(), timeout=restart_delay_s)
            except TimeoutError:
                pass


async def _resolve_bot_account(
    client: SybilClient,
    db: DecisionDB,
    persona_key: str,
    strat_label: str,
    initial_balance_nanos: int,
    bot_name: str,
) -> int:
    """Reattach a (persona, strategy) bot to its persisted account, or mint one.

    AR-3: restarts must not abandon portfolios. We look up the persisted
    account id for this (persona, strategy) pair and reuse it when the account
    still exists on the server; otherwise we create a fresh account and persist
    the mapping so the next restart reattaches.
    """
    existing = db.get_bot_account_id(persona_key, strat_label)
    if existing is not None:
        try:
            await client.get_account(existing)
            log.info("Reattached %s to existing account %d", bot_name, existing)
            return existing
        except Exception as e:
            log.warning(
                "Persisted account %d for %s is unusable (%s); creating a new one",
                existing,
                bot_name,
                e,
            )

    account = await client.create_account(initial_balance_nanos)
    db.save_bot_account_id(persona_key, strat_label, account.id)
    log.info(
        "Created account %d for %s ($%.2f)",
        account.id,
        bot_name,
        initial_balance_nanos / NANOS_PER_DOLLAR,
    )
    return account.id


async def _create_stage1_ab_topology(
    client: SybilClient,
    db: DecisionDB,
    config: LiveConfig,
    experiment_id: str,
    market_ids: list[int],
    markets_info: dict,
    metrics: ArenaMetrics,
) -> LiveTopology:
    """Create isolated control/Stage-1 analyst + Flat-sizer arms."""
    analysts: list[PersonaAnalyst] = []
    traders: list[LiveLlmTrader] = []
    paired_analyst_groups: list[tuple[PersonaAnalyst, PersonaAnalyst]] = []

    for persona_key in config.personas:
        persona = PERSONAS[persona_key]
        persona_analysts = []
        for variant in STAGE1_AB_VARIANTS:
            variant_id = variant["id"]
            durable_key = f"{STAGE1_AB_MODE}:{experiment_id}:{persona_key}:{variant_id}"
            display_prefix = f"{persona['name']} [SYB-114:{experiment_id}:{variant_id}]"
            analyst_name = f"{display_prefix} (Analyst)"
            trader_name = f"{display_prefix} (Flat)"

            # An arm owns both sides of its analysis/sizing boundary: its own
            # bus, its own FlatStrategy state, and its own durable account.
            bus = FairValueBus(persona_key=durable_key)
            analyst = PersonaAnalyst(
                client=client,
                news_feed=None,
                bus=bus,
                api_key=config.api_key,
                persona=persona["persona"],
                persona_key=durable_key,
                model_name=config.model_name,
                market_ids=market_ids,
                markets_info=markets_info,
                db=db,
                min_llm_interval_s=config.min_llm_interval,
                name=analyst_name,
                metrics=metrics,
                llm_budget_usd=config.llm_budget_usd,
                prompt_contract=variant["prompt_contract"],
            )
            account_id = await _resolve_bot_account(
                client,
                db,
                durable_key,
                "Flat",
                int(config.initial_balance * NANOS_PER_DOLLAR),
                trader_name,
            )
            trader = LiveLlmTrader(
                client=client,
                account_id=account_id,
                news_feed=None,
                strategy=FlatStrategy(),
                market_ids=market_ids,
                markets_info=markets_info,
                db=db,
                name=trader_name,
                fair_value_bus=bus,
                fair_value_ttl_s=config.fair_value_ttl_s,
                fair_value_half_life_s=config.fair_value_half_life_s,
                fair_value_hard_expiry_s=config.fair_value_hard_expiry_s,
            )
            trader.time_in_force = config.order_time_in_force
            analysts.append(analyst)
            persona_analysts.append(analyst)
            traders.append(trader)
        paired_analyst_groups.append((persona_analysts[0], persona_analysts[1]))

    return LiveTopology(
        analysts=analysts,
        traders=traders,
        paired_analyst_groups=paired_analyst_groups,
    )


async def _create_default_live_topology(
    client: SybilClient,
    db: DecisionDB,
    config: LiveConfig,
    market_ids: list[int],
    markets_info: dict,
    metrics: ArenaMetrics,
) -> LiveTopology:
    """Build the ordinary one-analyst + Kelly/Flat graph unchanged."""
    # Preserve the existing strategy-object lifecycle: one instance per arm is
    # created for the runner and shared across persona sizers.
    strategies = [
        ("Kelly", KellyStrategy()),
        ("Flat", FlatStrategy()),
    ]
    analysts: list[PersonaAnalyst] = []
    traders: list[LiveLlmTrader] = []
    for persona_key in config.personas:
        if persona_key not in PERSONAS:
            log.warning("Unknown persona: %s, skipping", persona_key)
            continue
        persona = PERSONAS[persona_key]

        bus = FairValueBus(persona_key=persona_key)
        analyst = PersonaAnalyst(
            client=client,
            news_feed=None,
            bus=bus,
            api_key=config.api_key,
            persona=persona["persona"],
            persona_key=persona_key,
            model_name=config.model_name,
            market_ids=market_ids,
            markets_info=markets_info,
            db=db,
            min_llm_interval_s=config.min_llm_interval,
            name=f"{persona['name']} (Analyst)",
            metrics=metrics,
            llm_budget_usd=config.llm_budget_usd,
        )
        analysts.append(analyst)

        for strat_label, strategy in strategies:
            bot_name = f"{persona['name']} ({strat_label})"
            account_id = await _resolve_bot_account(
                client,
                db,
                persona_key,
                strat_label,
                int(config.initial_balance * NANOS_PER_DOLLAR),
                bot_name,
            )
            trader = LiveLlmTrader(
                client=client,
                account_id=account_id,
                news_feed=None,
                strategy=strategy,
                market_ids=market_ids,
                markets_info=markets_info,
                db=db,
                name=bot_name,
                fair_value_bus=bus,
                fair_value_ttl_s=config.fair_value_ttl_s,
                fair_value_half_life_s=config.fair_value_half_life_s,
                fair_value_hard_expiry_s=config.fair_value_hard_expiry_s,
            )
            trader.time_in_force = config.order_time_in_force
            traders.append(trader)

    return LiveTopology(analysts=analysts, traders=traders)


def _wire_live_inputs(
    analysts: list[PersonaAnalyst],
    traders: list[LiveLlmTrader],
    feed: NewsFeed,
    paired_analyst_groups: list[tuple[PersonaAnalyst, PersonaAnalyst]] | None = None,
    startup_reference_prices: dict[int, float] | None = None,
) -> None:
    """Attach default or paired analyst feed views and each sizer's price feed."""
    paired_analysts = set()
    for control, stage1 in paired_analyst_groups or []:
        paired_analysts.update((control, stage1))
        subscription = feed.subscribe(
            name=f"paired:{control.persona_key.rsplit(':', 1)[0]}"
        )
        barrier = PairedNewsBatchBarrier(
            subscription,
            (control.name, stage1.name),
            startup_reference_prices or {},
            feed.polymarket_prices.get_price,
        )
        control.attach_feed_and_bus(feed, control.bus, barrier.view(control.name))
        stage1.attach_feed_and_bus(feed, stage1.bus, barrier.view(stage1.name))
    for analyst in analysts:
        if analyst not in paired_analysts:
            analyst.attach_feed_and_bus(feed, analyst.bus)
    for trader in traders:
        trader.attach_news_feed(feed)


async def _start_live_tasks(
    feed: NewsFeed,
    analysts: list[PersonaAnalyst],
    traders: list[LiveLlmTrader],
    fast_traders: list[FastReferenceTrader],
    noise_traders: list,
    db: DecisionDB,
    stop_event: asyncio.Event,
    required_baseline_trader_names: set[str] | None = None,
) -> list[asyncio.Task]:
    """Persist every account baseline before starting any live worker."""
    snapshot_traders = [*traders, *fast_traders, *noise_traders]
    await snapshot_portfolios_once(
        snapshot_traders,
        db,
        required_trader_names=required_baseline_trader_names,
    )

    tasks = [
        asyncio.create_task(feed.run(), name="news_feed"),
        asyncio.create_task(
            snapshot_portfolios(snapshot_traders, db),
            name="snapshots",
        ),
        asyncio.create_task(log_articles_loop(feed, db), name="article_logger"),
    ]
    for analyst in analysts:
        tasks.append(
            asyncio.create_task(
                supervise_bot(analyst, stop_event), name=f"analyst:{analyst.name}"
            )
        )
    for trader in traders:
        tasks.append(
            asyncio.create_task(supervise_bot(trader, stop_event), name=f"trader:{trader.name}")
        )
    for fast in fast_traders:
        tasks.append(
            asyncio.create_task(supervise_bot(fast, stop_event), name=f"fast:{fast.name}")
        )
    for noise in noise_traders:
        tasks.append(
            asyncio.create_task(supervise_bot(noise, stop_event), name=f"noise:{noise.name}")
        )
    return tasks


async def run_live(config: LiveConfig):
    """Main entry point for live trading."""
    experiment_id = _validate_stage1_ab_config(config)

    # Resolve DB path
    db_path = config.db_path or str(Path(__file__).parent / "decisions.db")
    db = DecisionDB(db_path)
    log.info("Decision DB: %s", db_path)

    # Arena-owned metrics exporter (off by default; sybil-api owns sybil_bot_*).
    metrics = ArenaMetrics()
    metrics_server = start_metrics_server(metrics, config.metrics_port, config.metrics_host)
    if metrics_server is not None:
        log.info("Arena metrics listening on %s:%d", config.metrics_host, config.metrics_port)

    async with SybilClient(config.sybil_url) as client:
        # 1. Discover markets. When reference prices are required, arena may
        # start before the Polymarket mirror has published any; retry instead of
        # exiting so a cold start self-heals once the mirror catches up.
        active = []
        while not active:
            all_markets = await client.list_markets()
            log.info("Total markets on server: %d", len(all_markets))

            if config.market_ids:
                # Manual market selection by ID
                market_by_id = {m.id: m for m in all_markets}
                if experiment_id is not None:
                    missing = [mid for mid in config.market_ids if mid not in market_by_id]
                    if missing:
                        raise ValueError(
                            "Stage 1 A/B cohort contains market ids absent from the server: "
                            + ", ".join(str(mid) for mid in missing)
                        )
                active = []
                for mid in config.market_ids:
                    if mid in market_by_id:
                        active.append(market_by_id[mid])
                    else:
                        log.warning("Market ID %d not found on server, skipping", mid)
                log.info("Manual market selection: %d markets", len(active))
            else:
                active = _select_markets_resilient(
                    all_markets,
                    config.max_markets,
                    config.market_profile,
                    require_reference_price=config.require_reference_prices,
                )

            metrics.set_market_selection(
                len(active),
                sum(1 for m in active if (getattr(m, "reference_price_nanos", 0) or 0) > 0),
            )
            if active:
                break

            if not config.require_reference_prices:
                log.error("No suitable markets found!")
                return

            log.warning(
                "No reference-backed markets found for profile=%s; retrying in %ss",
                config.market_profile,
                MARKET_DISCOVERY_RETRY_SECONDS,
            )
            await asyncio.sleep(MARKET_DISCOVERY_RETRY_SECONDS)

        log.info(
            "Selected %d markets for trading with profile=%s:",
            len(active),
            config.market_profile,
        )
        for m in active:
            log.info(
                "  [%d] %s (YES=%.2f, vol=$%.0f)",
                m.id,
                m.name[:60],
                m.yes_price,
                m.volume_dollars,
            )

        markets_info = {m.id: m for m in active}
        market_ids = [m.id for m in active]
        synthetic_markets = [
            m for m in all_markets if str(getattr(m, "status", "")).lower() == "active"
        ]
        if config.market_ids:
            allowed = set(config.market_ids)
            synthetic_markets = [m for m in synthetic_markets if m.id in allowed]
        synthetic_markets_info = {m.id: m for m in synthetic_markets}
        synthetic_market_ids = [m.id for m in synthetic_markets]

        # 2. Create analyst/sizer accounts. The experiment is a fully opt-in
        # alternate topology; without an id, preserve the ordinary live graph
        # and names exactly (one analyst feeding Kelly + Flat per persona).
        startup_reference_prices = {}
        if experiment_id is not None:
            startup_reference_prices = _require_stage1_ab_startup_reference_prices(active)
            genesis_hash = await _require_committed_genesis_hash(client)
            experiment_config = _stage1_ab_configuration(
                config,
                genesis_hash,
                startup_reference_prices,
            )
            metadata = db.ensure_experiment(
                experiment_id,
                STAGE1_AB_MODE,
                experiment_config,
            )
            _require_new_experiment(metadata)
            log.info(
                "SYB-114 Stage 1 A/B enabled: id=%s start=%s cohort=%s "
                "genesis=%s model=%s variants=control,stage1 analysts=%d",
                experiment_id,
                metadata["started_at_utc"],
                experiment_config["market_ids"],
                genesis_hash,
                config.model_name,
                experiment_config["analyst_count"],
            )
            if config.llm_budget_usd is None:
                log.info(
                    "SYB-114 A/B LLM pause threshold: unlimited per analyst; two independent "
                    "analysts per persona (2x ordinary configured threshold)"
                )
            else:
                log.info(
                    "SYB-114 A/B LLM pause threshold: $%.2f per analyst, $%.2f per persona "
                    "(exactly 2x ordinary configured threshold), $%.2f configured total; "
                    "actual spend may cross each threshold by one completed call",
                    config.llm_budget_usd,
                    experiment_config["llm_pause_threshold_usd_per_persona"],
                    experiment_config["configured_llm_pause_threshold_usd_total"],
                )
            topology = await _create_stage1_ab_topology(
                client,
                db,
                config,
                experiment_id,
                market_ids,
                markets_info,
                metrics,
            )
            analysts = topology.analysts
            traders = topology.traders
        else:
            # SYB-210: split analysis from sizing. Each persona gets ONE analyst
            # (the sole LLM caller) publishing onto a per-persona FairValueBus,
            # and TWO sizers (Kelly + Flat) subscribing to that same bus.
            topology = await _create_default_live_topology(
                client,
                db,
                config,
                market_ids,
                markets_info,
                metrics,
            )
            analysts = topology.analysts
            traders = topology.traders

        # 3. Create synthetic fast/noise traders. Fast traders only act on
        # reference-backed mirror markets; noise traders only act on native
        # no-reference markets. Both consume the same config shape with per-bot
        # seed offsets for deterministic but non-identical streams.
        fast_traders = []
        for i in range(config.fast_count):
            account = await client.create_account(int(config.noise_balance * NANOS_PER_DOLLAR))
            fast = FastReferenceTrader(
                client=client,
                account_id=account.id,
                name=f"Fast-{i}",
                market_ids=synthetic_market_ids,
                markets_info=synthetic_markets_info,
                config=config.synthetic_strategy.with_seed(
                    config.synthetic_strategy.random_seed + i
                ),
            )
            fast.time_in_force = config.order_time_in_force
            fast_traders.append(fast)

        # Noise traders. When crossing is enabled (default), they cover ALL
        # active markets (mirror AND native — the same markets the LLM bots
        # trade) and post aggressive two-sided crossing orders on a durable
        # (GTC) book, which is the reliable path to fills. When disabled they
        # fall back to the legacy inventory-aware native-only noise flow.
        crossing = config.synthetic_strategy.crossing_enabled
        noise_traders = []
        for i in range(config.noise_count):
            account = await client.create_account(int(config.noise_balance * NANOS_PER_DOLLAR))
            noise_cfg = config.synthetic_strategy.with_seed(
                config.synthetic_strategy.random_seed + 10_000 + i
            )
            if crossing:
                noise = CrossingNoiseTrader(
                    client=client,
                    account_id=account.id,
                    name=f"Noise-{i}",
                    market_ids=synthetic_market_ids,
                    markets_info=synthetic_markets_info,
                    config=noise_cfg,
                )
                noise.time_in_force = config.noise_time_in_force
            else:
                noise = NativeNoiseTrader(
                    client=client,
                    account_id=account.id,
                    name=f"Noise-{i}",
                    market_ids=synthetic_market_ids,
                    markets_info=synthetic_markets_info,
                    config=noise_cfg,
                )
                noise.time_in_force = config.order_time_in_force
            noise_traders.append(noise)
        log.info(
            "Created %d fast traders and %d %s noise traders (TIF=%s) over %d active markets",
            len(fast_traders),
            len(noise_traders),
            "crossing" if crossing else "native",
            config.noise_time_in_force if crossing else config.order_time_in_force,
            len(synthetic_markets_info),
        )

        # 4. Create news feed (with LLM gate using cheap model)
        feed = NewsFeed(
            active,
            api_key=config.api_key,
            poll_interval_s=config.news_poll_interval,
            mapping_path=config.mapping_path,
            metrics=metrics,
        )

        # Wire feed into analysts (news subscription) and sizers (price cache
        # only). Each analyst registers its own subscriber view of the feed so
        # every persona sees every article (SYB-192); the two sizers of a
        # persona no longer subscribe to news — they consume the analyst's
        # FairValueBus instead (SYB-210).
        _wire_live_inputs(
            analysts,
            traders,
            feed,
            topology.paired_analyst_groups,
            startup_reference_prices,
        )

        # 5. Run everything
        log.info(
            "Starting live trading with %d analysts + %d sizers + %d fast + %d noise traders on %d selected markets",
            len(analysts),
            len(traders),
            len(fast_traders),
            len(noise_traders),
            len(active),
        )

        stop_event = asyncio.Event()
        tasks = await _start_live_tasks(
            feed,
            analysts,
            traders,
            fast_traders,
            noise_traders,
            db,
            stop_event,
            required_baseline_trader_names=(
                {trader.name for trader in traders} if experiment_id is not None else None
            ),
        )

        # Graceful shutdown
        def _signal_handler():
            log.info("Shutdown requested")
            stop_event.set()
            for a in analysts:
                a.stop()
            for t in traders:
                t.stop()
            for f in fast_traders:
                f.stop()
            for n in noise_traders:
                n.stop()

        loop = asyncio.get_event_loop()
        for sig in (signal.SIGINT, signal.SIGTERM):
            loop.add_signal_handler(sig, _signal_handler)

        log.info("All systems running. Press Ctrl+C to stop.")

        stop_task = asyncio.create_task(stop_event.wait(), name="stop_signal")
        watched_tasks = [stop_task, *tasks]
        done, _ = await asyncio.wait(watched_tasks, return_when=asyncio.FIRST_COMPLETED)

        failure: BaseException | None = None
        if stop_task in done:
            log.info("Stopping all tasks...")
        else:
            stop_event.set()
            for task in done:
                if task is stop_task:
                    continue
                exc = task.exception()
                if exc is not None:
                    log.error("Task %s failed: %s", task.get_name(), exc)
                    failure = exc
                else:
                    log.error("Task %s exited unexpectedly", task.get_name())
                    failure = RuntimeError(f"Task {task.get_name()} exited unexpectedly")
            log.info("Stopping all tasks after worker failure...")
            for a in analysts:
                a.stop()
            for t in traders:
                t.stop()
            for f in fast_traders:
                f.stop()
            for n in noise_traders:
                n.stop()

        # Give traders a moment to finish current block processing
        await asyncio.sleep(3)

        # Cancel remaining tasks
        for task in watched_tasks:
            task.cancel()
        await asyncio.gather(*watched_tasks, return_exceptions=True)

        db.close()
        if metrics_server is not None:
            try:
                server, _thread = metrics_server
                server.shutdown()
            except Exception:
                log.debug("Metrics server shutdown failed", exc_info=True)
        log.info("Shutdown complete.")
        if failure is not None:
            raise failure


def main():
    parser = argparse.ArgumentParser(description="Live AI trading bots")
    parser.add_argument("--sybil-url", default="http://172.104.31.54:3000")
    parser.add_argument("--model", default="deepseek/deepseek-v4-flash")
    parser.add_argument(
        "--max-markets",
        type=int,
        default=None,
        help=(
            "Maximum markets for bots to trade. Defaults to ARENA_MAX_MARKETS or 0. "
            "For --market-profile=all, 0 means all suitable mirrored markets; focused "
            "profiles use their profile default."
        ),
    )
    parser.add_argument(
        "--market-profile",
        choices=["all", "important-news"],
        default=None,
        help=(
            "Market selection profile for automatic market discovery. Defaults to "
            "ARENA_MARKET_PROFILE or all."
        ),
    )
    parser.add_argument(
        "--require-reference-prices",
        action="store_true",
        help="Only auto-select markets with live external reference prices.",
    )
    parser.add_argument(
        "--order-time-in-force",
        choices=["GTC", "IOC", "GTD"],
        default="IOC",
        help="Time-in-force for live bot/noise orders. IOC avoids stale resting orders.",
    )
    parser.add_argument("--balance", type=float, default=500.0, help="Initial balance per trader")
    parser.add_argument("--fast-count", type=int, default=None)
    parser.add_argument("--noise-count", type=int, default=None)
    parser.add_argument(
        "--synthetic-max-inventory",
        type=int,
        default=None,
        help="Max synthetic trader inventory per market side, in shares.",
    )
    parser.add_argument(
        "--synthetic-quote-width",
        type=float,
        default=None,
        help="Minimum price edge before a synthetic trader acts.",
    )
    parser.add_argument(
        "--synthetic-notional-budget",
        type=float,
        default=None,
        help="Per-order synthetic trader notional budget in dollars.",
    )
    parser.add_argument(
        "--synthetic-seed",
        type=int,
        default=None,
        help="Base RNG seed for deterministic synthetic traders.",
    )
    parser.add_argument(
        "--synthetic-randomization-range",
        type=float,
        default=None,
        help="Synthetic price jitter range; capped internally below 2%%.",
    )
    parser.add_argument(
        "--synthetic-market-ids",
        nargs="+",
        type=int,
        default=None,
        help="Optional per-market enablement for synthetic traders.",
    )
    parser.add_argument(
        "--news-interval", type=int, default=300, help="RSS poll interval (seconds)"
    )
    parser.add_argument(
        "--min-llm-interval",
        type=float,
        default=60.0,
        help="Min seconds between LLM calls",
    )
    parser.add_argument(
        "--fair-value-ttl-s",
        type=float,
        default=None,
        help=(
            "Seconds before an analyst fair value starts decaying toward market price. "
            "Defaults to ARENA_FAIR_VALUE_TTL_S or 600."
        ),
    )
    parser.add_argument(
        "--fair-value-half-life-s",
        type=float,
        default=None,
        help=(
            "Half-life in seconds for stale fair-value edge decay. Defaults to "
            "ARENA_FAIR_VALUE_HALF_LIFE_S or 1800."
        ),
    )
    parser.add_argument(
        "--fair-value-hard-expiry-s",
        type=float,
        default=None,
        help=(
            "Seconds after which an analyst fair value is treated as missing. "
            "Defaults to ARENA_FAIR_VALUE_HARD_EXPIRY_S or 7200."
        ),
    )
    parser.add_argument(
        "--llm-budget-usd",
        type=float,
        default=5.0,
        help=(
            "Per-analyst LLM pause threshold in USD (SYB-64). A completed call may "
            "cross it; later calls pause. <=0 disables the threshold (unlimited)."
        ),
    )
    parser.add_argument("--db-path", default="", help="SQLite DB path")
    parser.add_argument(
        "--metrics-port",
        type=int,
        default=0,
        help="Prometheus exporter port for arena metrics; <=0 (default) disables it.",
    )
    parser.add_argument(
        "--metrics-host",
        default="0.0.0.0",
        help="Bind host for the arena metrics exporter.",
    )
    parser.add_argument(
        "--personas", nargs="+", default=list(PERSONAS.keys()), help="Persona keys to use"
    )
    parser.add_argument(
        "--market-ids",
        nargs="+",
        type=int,
        default=None,
        help="Manually specify market IDs to trade (overrides --max-markets)",
    )
    parser.add_argument(
        "--stage1-ab-experiment-id",
        default=None,
        help=(
            "Opt into the concurrent SYB-114 Stage 1 control-vs-Stage1 experiment. "
            "Requires an explicit nonempty --market-ids cohort."
        ),
    )
    parser.add_argument(
        "--mapping-path", default=None, help="Path to polymarket_mapping.json for reference prices"
    )
    parser.add_argument("--log-level", default="INFO")
    args = parser.parse_args()
    try:
        max_markets = (
            args.max_markets if args.max_markets is not None else _env_int("ARENA_MAX_MARKETS", 0)
        )
        market_profile = args.market_profile or _env_market_profile("ARENA_MARKET_PROFILE")
        fast_count = (
            args.fast_count if args.fast_count is not None else _env_int("ARENA_FAST_COUNT", 5)
        )
        synthetic_max_inventory = (
            args.synthetic_max_inventory
            if args.synthetic_max_inventory is not None
            else _env_int("ARENA_SYNTHETIC_MAX_INVENTORY", 50)
        )
        synthetic_quote_width = (
            args.synthetic_quote_width
            if args.synthetic_quote_width is not None
            else _env_float("ARENA_SYNTHETIC_QUOTE_WIDTH", 0.005)
        )
        synthetic_notional_budget = (
            args.synthetic_notional_budget
            if args.synthetic_notional_budget is not None
            else _env_float("ARENA_SYNTHETIC_NOTIONAL_BUDGET", 5.0)
        )
        synthetic_seed = (
            args.synthetic_seed
            if args.synthetic_seed is not None
            else _env_int("ARENA_SYNTHETIC_SEED", 42)
        )
        synthetic_randomization_range = (
            args.synthetic_randomization_range
            if args.synthetic_randomization_range is not None
            else _env_float("ARENA_SYNTHETIC_RANDOMIZATION_RANGE", 0.02)
        )
        # Zero-fills fix: well-funded, aggressive two-sided crossing noise.
        noise_count = (
            args.noise_count if args.noise_count is not None else _env_int("ARENA_NOISE_COUNT", 5)
        )
        # Well-funded by default so crossing noise sustains a steady fill stream;
        # each crossing pair pays a small (~2*crossing_edge) mint premium.
        noise_balance = _env_float("ARENA_NOISE_BALANCE", 100_000.0)
        crossing_enabled = _env_bool("ARENA_NOISE_CROSSING", True)
        crossing_edge = _env_float("ARENA_NOISE_CROSSING_EDGE", 0.03)
        crossing_markets_per_block = _env_int("ARENA_NOISE_MARKETS_PER_BLOCK", 6)
        noise_tif_raw = os.environ.get("ARENA_NOISE_TIF", "GTC").strip().upper()
        if noise_tif_raw not in ("GTC", "IOC", "GTD"):
            raise ValueError("ARENA_NOISE_TIF must be one of: GTC, IOC, GTD")
        noise_time_in_force: TimeInForce = noise_tif_raw  # type: ignore[assignment]
        fair_value_ttl_s = (
            args.fair_value_ttl_s
            if args.fair_value_ttl_s is not None
            else _env_float("ARENA_FAIR_VALUE_TTL_S", FairValueFreshnessConfig.ttl_s)
        )
        fair_value_half_life_s = (
            args.fair_value_half_life_s
            if args.fair_value_half_life_s is not None
            else _env_float(
                "ARENA_FAIR_VALUE_HALF_LIFE_S",
                FairValueFreshnessConfig.half_life_s,
            )
        )
        fair_value_hard_expiry_s = (
            args.fair_value_hard_expiry_s
            if args.fair_value_hard_expiry_s is not None
            else _env_float(
                "ARENA_FAIR_VALUE_HARD_EXPIRY_S",
                FairValueFreshnessConfig.hard_expiry_s,
            )
        )
        FairValueFreshnessConfig(
            ttl_s=fair_value_ttl_s,
            half_life_s=fair_value_half_life_s,
            hard_expiry_s=fair_value_hard_expiry_s,
        )
    except ValueError as e:
        parser.error(str(e))

    api_key = os.environ.get("OPENROUTER_API_KEY", "")
    if not api_key:
        parser.error(
            "OPENROUTER_API_KEY is required in the environment; do not pass it as a CLI argument"
        )

    logging.basicConfig(
        level=getattr(logging, args.log_level.upper()),
        format="%(asctime)s %(name)-20s %(levelname)-5s %(message)s",
        datefmt="%H:%M:%S",
    )
    logging.getLogger("httpx").setLevel(logging.WARNING)

    config = LiveConfig(
        sybil_url=args.sybil_url,
        api_key=api_key,
        model_name=args.model,
        initial_balance=args.balance,
        max_markets=max_markets,
        market_profile=market_profile,
        require_reference_prices=args.require_reference_prices,
        order_time_in_force=args.order_time_in_force,
        fast_count=fast_count,
        noise_count=noise_count,
        noise_balance=noise_balance,
        noise_time_in_force=noise_time_in_force,
        synthetic_strategy=SyntheticStrategyConfig(
            max_inventory=synthetic_max_inventory,
            quote_width=synthetic_quote_width,
            notional_budget=synthetic_notional_budget,
            random_seed=synthetic_seed,
            randomization_range=synthetic_randomization_range,
            enabled_market_ids=(
                frozenset(args.synthetic_market_ids)
                if args.synthetic_market_ids is not None
                else None
            ),
            crossing_enabled=crossing_enabled,
            crossing_edge=crossing_edge,
            crossing_markets_per_block=crossing_markets_per_block,
        ),
        news_poll_interval=args.news_interval,
        min_llm_interval=args.min_llm_interval,
        fair_value_ttl_s=fair_value_ttl_s,
        fair_value_half_life_s=fair_value_half_life_s,
        fair_value_hard_expiry_s=fair_value_hard_expiry_s,
        llm_budget_usd=args.llm_budget_usd if args.llm_budget_usd > 0 else None,
        db_path=args.db_path,
        metrics_host=args.metrics_host,
        metrics_port=args.metrics_port,
        personas=args.personas,
        market_ids=args.market_ids,
        mapping_path=args.mapping_path,
        stage1_ab_experiment_id=args.stage1_ab_experiment_id,
    )

    try:
        _validate_stage1_ab_config(config)
    except ValueError as e:
        parser.error(str(e))

    asyncio.run(run_live(config))


if __name__ == "__main__":
    main()
