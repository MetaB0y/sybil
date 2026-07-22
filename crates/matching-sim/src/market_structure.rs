//! Research-only paired market-structure experiments.
//!
//! The FBA path calls the actual production solver. The CLOB is an explicit
//! event-time baseline local to this module and is never used by sequencing or
//! validity code.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Parser, ValueEnum};
use matching_engine::{
    MmConstraint, MmId, MmSide, NANOS_PER_DOLLAR, Nanos, Problem, SHARE_SCALE, outcome_buy,
    outcome_sell, shares_to_qty,
};
use matching_solver::{ProductionSolver, TerminationStatus};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sybil_verifier::verify_match;

use crate::witness::witness_from_pipeline;

const MIN_PRICE: u64 = 1;
const MAX_PRICE: u64 = NANOS_PER_DOLLAR - 1;
const MICRO_MAKER_SHARES: u64 = 10;
const MICRO_TRADER_SHARES: u64 = 5;
const PORTFOLIO_MAKER_SHARES: u64 = 10;
const PORTFOLIO_HALF_SPREAD_NANOS: u64 = 20_000_000;
const PORTFOLIO_BATCH_INTERVAL_MS: u64 = 500;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Run paired Sybil-FBA and fair-CLOB research episodes"
)]
pub struct Cli {
    /// Versioned protocol JSON controlling axes and allowed seeds.
    #[arg(
        long,
        default_value = "benchmarks/market-structure/protocol-development.json"
    )]
    protocol: PathBuf,

    /// Atomic JSONL output path. Every attempted engine row is retained.
    #[arg(long)]
    output: PathBuf,

    /// Restrict execution to one experiment family.
    #[arg(long, value_enum, default_value_t)]
    suite: SuiteChoice,

    /// First seed to run; must remain inside the protocol's active range.
    #[arg(long)]
    seed_start: Option<u64>,

    /// Number of seeds from --seed-start; defaults to the active range tail.
    #[arg(long)]
    seed_count: Option<u64>,

    /// Development-only cap on configurations per suite, before seed expansion.
    #[arg(long)]
    max_configs: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
enum SuiteChoice {
    Microstructure,
    Portfolio,
    #[default]
    All,
}

#[derive(Debug, Deserialize)]
struct Protocol {
    schema_version: u32,
    protocol_id: String,
    status: String,
    episode_families: Vec<EpisodeFamily>,
    development_seeds: Option<SeedRange>,
    run_seeds: Option<SeedRange>,
    held_out_embargo: Option<SeedRange>,
}

#[derive(Debug, Deserialize)]
struct EpisodeFamily {
    id: String,
    #[serde(default)]
    regimes: Vec<String>,
    #[serde(default)]
    development_axes: Option<Value>,
    #[serde(default)]
    axes: Option<Value>,
}

impl EpisodeFamily {
    fn parse_axes<T: for<'de> Deserialize<'de>>(&self) -> Result<T, String> {
        let value = self
            .axes
            .as_ref()
            .or(self.development_axes.as_ref())
            .ok_or_else(|| format!("episode family {} has no axes", self.id))?;
        serde_json::from_value(value.clone())
            .map_err(|error| format!("invalid axes for {}: {error}", self.id))
    }
}

#[derive(Clone, Debug, Deserialize)]
struct SeedRange {
    #[serde(alias = "planned_range_inclusive")]
    range_inclusive: [u64; 2],
}

#[derive(Clone, Debug, Deserialize)]
struct MicroAxes {
    batch_interval_ms: Vec<u64>,
    quote_half_spread_nanos: Vec<u64>,
    maker_reaction_ms: Vec<u64>,
    taker_reaction_ms: Vec<u64>,
    fba_bundle_cancel_policy: Vec<String>,
    informed_trader_count: Vec<usize>,
    jump_nanos: Vec<u64>,
}

#[derive(Clone, Debug, Deserialize)]
struct PortfolioAxes {
    market_count: Vec<usize>,
    budget_fraction_ppm: Vec<u64>,
    flow_concentration: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Side {
    Buy,
    Sell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TraderKind {
    Natural,
    Informed,
}

#[derive(Clone, Debug, Serialize)]
struct TraderIntent {
    id: u64,
    market_index: usize,
    kind: TraderKind,
    side: Side,
    value_nanos: u64,
    limit_nanos: u64,
    quantity_units: u64,
    venue_arrival_ms: u64,
    resting_until_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
struct MicroTape {
    seed: u64,
    regime: String,
    initial_fundamental_nanos: u64,
    final_fundamental_nanos: u64,
    shock_at_ms: u64,
    maker_replace_arrival_ms: u64,
    batch_interval_ms: u64,
    maker_quote_quantity_units: u64,
    trader_intents: Vec<TraderIntent>,
}

#[derive(Clone, Debug, Serialize)]
struct PortfolioTape {
    seed: u64,
    concentration: String,
    fundamentals_nanos: Vec<u64>,
    expected_flow_scores: Vec<u64>,
    realized_flow_scores: Vec<u64>,
    trader_intents: Vec<TraderIntent>,
    arrival_order: Vec<usize>,
}

#[derive(Clone, Copy, Debug)]
enum Role {
    Maker,
    Trader(TraderKind),
}

#[derive(Clone, Copy, Debug)]
struct OrderMeta {
    role: Role,
    side: Side,
    value_nanos: u64,
    submitted_at_ms: u64,
    quote_fundamental_nanos: Option<u64>,
    market_index: usize,
}

#[derive(Clone, Copy, Debug)]
struct FillObservation {
    meta: OrderMeta,
    quantity_units: u64,
    price_nanos: u64,
    executed_at_ms: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
struct Metrics {
    maker_markout_pnl_nanos: i64,
    maker_pnl_per_filled_share_nanos: Option<i64>,
    maker_stale_quote_loss_nanos: u64,
    maker_filled_quantity_units: u64,
    natural_trader_surplus_nanos: i64,
    informed_trader_surplus_nanos: i64,
    submitted_trader_quantity_units: u64,
    filled_trader_quantity_units: u64,
    fill_rate_ppm: u64,
    execution_delay_ms: Option<u64>,
    post_window_price_error_nanos: Option<u64>,
    displayed_quote_market_coverage_ppm: u64,
    single_market_executable_coverage_ppm: u64,
    simultaneous_worst_case_coverage_ppm: u64,
    filled_market_coverage_ppm: u64,
    capital_reserved_nanos: u64,
    capital_consumed_nanos: u64,
}

#[derive(Clone, Debug, Serialize)]
struct SolverEvidence {
    batch_index: u64,
    termination: String,
    message: Option<String>,
    wall_time_micros: u128,
    verifier_valid: bool,
    violation_count: usize,
    fills: usize,
}

#[derive(Debug, Serialize)]
struct RunRecord {
    record_schema_version: u32,
    protocol_schema_version: u32,
    protocol_id: String,
    protocol_status: String,
    protocol_blake3: String,
    suite: String,
    case_id: String,
    seed: u64,
    tape_blake3: String,
    engine: String,
    regime: String,
    parameters: Value,
    run_status: String,
    solver_evidence: Vec<SolverEvidence>,
    metrics: Metrics,
}

#[derive(Clone, Debug)]
struct EngineOutcome {
    status: String,
    observations: Vec<FillObservation>,
    price_error_nanos: Option<u64>,
    solver_evidence: Vec<SolverEvidence>,
    coverage: Coverage,
    capital_reserved_nanos: u64,
    capital_consumed_nanos: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Coverage {
    displayed_ppm: u64,
    single_executable_ppm: u64,
    simultaneous_worst_case_ppm: u64,
}

#[derive(Clone, Debug)]
struct MicroConfig {
    regime: String,
    batch_interval_ms: u64,
    quote_half_spread_nanos: u64,
    maker_reaction_ms: u64,
    taker_reaction_ms: u64,
    informed_trader_count: usize,
    jump_nanos: u64,
}

#[derive(Clone, Debug)]
struct PortfolioConfig {
    market_count: usize,
    budget_fraction_ppm: u64,
    concentration: String,
}

pub fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let protocol_bytes = fs::read(&cli.protocol)?;
    let protocol: Protocol = serde_json::from_slice(&protocol_bytes)?;
    validate_protocol(&protocol)?;
    let protocol_hash = blake3::hash(&protocol_bytes).to_hex().to_string();
    let seeds = selected_seeds(&protocol, &cli)?;

    let parent = cli.output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = cli
        .output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("output path needs a UTF-8 file name")?;
    let temporary = cli.output.with_file_name(format!(".{file_name}.tmp"));
    let mut output = BufWriter::new(File::create(&temporary)?);
    let mut rows = 0usize;

    if matches!(cli.suite, SuiteChoice::Microstructure | SuiteChoice::All) {
        let family = family(&protocol, "single-market-microstructure")?;
        let axes: MicroAxes = family.parse_axes()?;
        let configs = micro_configs(family, &axes, cli.max_configs)?;
        for config in &configs {
            for &seed in &seeds {
                let tape = generate_micro_tape(seed, config);
                let tape_hash = fingerprint(&tape)?;
                let parameters = json!({
                    "batch_interval_ms": config.batch_interval_ms,
                    "quote_half_spread_nanos": config.quote_half_spread_nanos,
                    "maker_reaction_ms": config.maker_reaction_ms,
                    "taker_reaction_ms": config.taker_reaction_ms,
                    "informed_trader_count": config.informed_trader_count,
                    "jump_nanos": config.jump_nanos,
                    "maker_quote_shares": MICRO_MAKER_SHARES,
                    "trader_order_shares": MICRO_TRADER_SHARES,
                    "clob_same_timestamp_priority": "cancel_replace_before_taker",
                    "natural_time_in_force": "rest-until-window-end",
                    "informed_time_in_force": "ioc"
                });
                let case_id = micro_case_id(config);
                let clob = run_micro_clob(&tape, config.quote_half_spread_nanos);
                write_record(
                    &mut output,
                    record(
                        &protocol,
                        &protocol_hash,
                        "single-market-microstructure",
                        &case_id,
                        seed,
                        &tape_hash,
                        "clob-firm-reserve",
                        &config.regime,
                        parameters.clone(),
                        clob,
                        &tape.trader_intents,
                        &[tape.final_fundamental_nanos],
                    ),
                )?;
                rows += 1;

                for policy in axes.fba_bundle_cancel_policy.iter().filter(|policy| {
                    config.regime == "jump" || policy.as_str() == "current-noncancellable"
                }) {
                    let cancellable = match policy.as_str() {
                        "current-noncancellable" => false,
                        "counterfactual-cancellable" => true,
                        other => return Err(format!("unknown FBA cancel policy {other}").into()),
                    };
                    let engine = if cancellable {
                        "fba-cancellable-sensitivity"
                    } else {
                        "sybil-fba"
                    };
                    let fba = run_micro_fba(&tape, config.quote_half_spread_nanos, cancellable);
                    let mut fba_parameters = parameters.clone();
                    fba_parameters["fba_bundle_cancel_policy"] = Value::String(policy.clone());
                    write_record(
                        &mut output,
                        record(
                            &protocol,
                            &protocol_hash,
                            "single-market-microstructure",
                            &case_id,
                            seed,
                            &tape_hash,
                            engine,
                            &config.regime,
                            fba_parameters,
                            fba,
                            &tape.trader_intents,
                            &[tape.final_fundamental_nanos],
                        ),
                    )?;
                    rows += 1;
                }
            }
        }
    }

    if matches!(cli.suite, SuiteChoice::Portfolio | SuiteChoice::All) {
        let family = family(&protocol, "shared-budget-portfolio")?;
        let axes: PortfolioAxes = family.parse_axes()?;
        let configs = portfolio_configs(axes, cli.max_configs)?;
        for config in &configs {
            for &seed in &seeds {
                let tape = generate_portfolio_tape(seed, config);
                let tape_hash = fingerprint(&tape)?;
                let reserve_by_market = portfolio_reserve_by_market(&tape);
                let single_exposure_by_market = portfolio_single_quote_exposure_by_market(&tape);
                let total_reserve: u64 = reserve_by_market.iter().copied().sum();
                let budget = mul_ppm(total_reserve, config.budget_fraction_ppm).max(1);
                let parameters = json!({
                    "market_count": config.market_count,
                    "budget_fraction_ppm": config.budget_fraction_ppm,
                    "flow_concentration": config.concentration,
                    "maker_quote_shares": PORTFOLIO_MAKER_SHARES,
                    "quote_half_spread_nanos": PORTFOLIO_HALF_SPREAD_NANOS,
                    "batch_interval_ms": PORTFOLIO_BATCH_INTERVAL_MS,
                    "budget_nanos": budget,
                    "total_two_sided_quote_reserve_nanos": total_reserve
                });
                let case_id = portfolio_case_id(config);
                for (engine, outcome) in [
                    (
                        "clob-firm-reserve",
                        run_portfolio_clob(
                            &tape,
                            budget,
                            &reserve_by_market,
                            &single_exposure_by_market,
                            true,
                        ),
                    ),
                    (
                        "clob-shared-risk",
                        run_portfolio_clob(
                            &tape,
                            budget,
                            &reserve_by_market,
                            &single_exposure_by_market,
                            false,
                        ),
                    ),
                    (
                        "sybil-fba",
                        run_portfolio_fba(
                            &tape,
                            budget,
                            &reserve_by_market,
                            &single_exposure_by_market,
                        ),
                    ),
                ] {
                    write_record(
                        &mut output,
                        record(
                            &protocol,
                            &protocol_hash,
                            "shared-budget-portfolio",
                            &case_id,
                            seed,
                            &tape_hash,
                            engine,
                            "portfolio",
                            parameters.clone(),
                            outcome,
                            &tape.trader_intents,
                            &tape.fundamentals_nanos,
                        ),
                    )?;
                    rows += 1;
                }
            }
        }
    }

    output.flush()?;
    output.get_ref().sync_all()?;
    drop(output);
    fs::rename(&temporary, &cli.output)?;
    eprintln!(
        "wrote {rows} complete engine rows to {}",
        cli.output.display()
    );
    Ok(())
}

fn validate_protocol(protocol: &Protocol) -> Result<(), String> {
    if protocol.schema_version != 1 {
        return Err(format!(
            "unsupported market-structure protocol schema {}",
            protocol.schema_version
        ));
    }
    if protocol.protocol_id.trim().is_empty() || protocol.status.trim().is_empty() {
        return Err("protocol id and status must be non-empty".to_string());
    }
    if protocol.run_seeds.is_some() && protocol.status.contains("development") {
        return Err("development protocol cannot unlock run_seeds".to_string());
    }
    if !protocol.status.contains("development") && protocol.run_seeds.is_none() {
        return Err("non-development protocol must declare run_seeds".to_string());
    }
    Ok(())
}

fn family<'a>(protocol: &'a Protocol, id: &str) -> Result<&'a EpisodeFamily, String> {
    protocol
        .episode_families
        .iter()
        .find(|family| family.id == id)
        .ok_or_else(|| format!("protocol is missing episode family {id}"))
}

fn selected_seeds(protocol: &Protocol, cli: &Cli) -> Result<Vec<u64>, String> {
    let allowed = protocol
        .run_seeds
        .as_ref()
        .or(protocol.development_seeds.as_ref())
        .ok_or_else(|| "protocol has no active seed range".to_string())?;
    let [allowed_start, allowed_end] = allowed.range_inclusive;
    if allowed_start > allowed_end {
        return Err("active seed range is inverted".to_string());
    }
    let start = cli.seed_start.unwrap_or(allowed_start);
    if start < allowed_start || start > allowed_end {
        return Err(format!(
            "requested seed start {start} leaves allowed range {allowed_start}..={allowed_end}"
        ));
    }
    let default_count = allowed_end - start + 1;
    let count = cli.seed_count.unwrap_or(default_count);
    if count == 0 {
        return Err("seed count must be positive".to_string());
    }
    let end = start
        .checked_add(count - 1)
        .ok_or_else(|| "seed range overflow".to_string())?;
    if start < allowed_start || end > allowed_end {
        return Err(format!(
            "requested seeds {start}..={end} leave allowed range {allowed_start}..={allowed_end}"
        ));
    }
    if protocol.run_seeds.is_none()
        && let Some(embargo) = &protocol.held_out_embargo
    {
        let [embargo_start, embargo_end] = embargo.range_inclusive;
        if start <= embargo_end && end >= embargo_start {
            return Err("requested development seeds overlap held-out embargo".to_string());
        }
    }
    Ok((start..=end).collect())
}

fn micro_configs(
    family: &EpisodeFamily,
    axes: &MicroAxes,
    max_configs: Option<usize>,
) -> Result<Vec<MicroConfig>, String> {
    for (name, empty) in [
        ("regimes", family.regimes.is_empty()),
        ("batch_interval_ms", axes.batch_interval_ms.is_empty()),
        (
            "quote_half_spread_nanos",
            axes.quote_half_spread_nanos.is_empty(),
        ),
        ("maker_reaction_ms", axes.maker_reaction_ms.is_empty()),
        ("taker_reaction_ms", axes.taker_reaction_ms.is_empty()),
        (
            "informed_trader_count",
            axes.informed_trader_count.is_empty(),
        ),
        ("jump_nanos", axes.jump_nanos.is_empty()),
        (
            "fba_bundle_cancel_policy",
            axes.fba_bundle_cancel_policy.is_empty(),
        ),
    ] {
        if empty {
            return Err(format!("microstructure axis {name} is empty"));
        }
    }
    if !family
        .regimes
        .iter()
        .all(|regime| matches!(regime.as_str(), "quiet" | "jump"))
    {
        return Err("microstructure regimes must be quiet or jump".to_string());
    }
    if axes.batch_interval_ms.contains(&0)
        || axes
            .quote_half_spread_nanos
            .iter()
            .any(|&value| value >= NANOS_PER_DOLLAR / 2)
        || axes.informed_trader_count.contains(&0)
        || axes
            .jump_nanos
            .iter()
            .any(|&value| value >= NANOS_PER_DOLLAR - 100_000_000)
    {
        return Err("microstructure axes contain an out-of-range value".to_string());
    }
    if !axes
        .fba_bundle_cancel_policy
        .iter()
        .any(|policy| policy == "current-noncancellable")
        || !axes.fba_bundle_cancel_policy.iter().all(|policy| {
            matches!(
                policy.as_str(),
                "current-noncancellable" | "counterfactual-cancellable"
            )
        })
    {
        return Err("FBA policies must include current-noncancellable".to_string());
    }
    if max_configs == Some(0) {
        return Err("--max-configs must be positive".to_string());
    }
    let mut configs = Vec::new();
    for regime in &family.regimes {
        let quiet = regime == "quiet";
        let maker_reactions = if quiet {
            &axes.maker_reaction_ms[..1]
        } else {
            &axes.maker_reaction_ms
        };
        let taker_reactions = if quiet {
            &axes.taker_reaction_ms[..1]
        } else {
            &axes.taker_reaction_ms
        };
        let informed_counts = if quiet {
            &axes.informed_trader_count[..1]
        } else {
            &axes.informed_trader_count
        };
        let jumps = if quiet {
            &axes.jump_nanos[..1]
        } else {
            &axes.jump_nanos
        };
        for &batch_interval_ms in &axes.batch_interval_ms {
            for &quote_half_spread_nanos in &axes.quote_half_spread_nanos {
                for &maker_reaction_ms in maker_reactions {
                    for &taker_reaction_ms in taker_reactions {
                        for &informed_trader_count in informed_counts {
                            for &jump_nanos in jumps {
                                configs.push(MicroConfig {
                                    regime: regime.clone(),
                                    batch_interval_ms,
                                    quote_half_spread_nanos,
                                    maker_reaction_ms,
                                    taker_reaction_ms,
                                    informed_trader_count,
                                    jump_nanos,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(limit) = max_configs {
        configs.truncate(limit);
    }
    Ok(configs)
}

fn portfolio_configs(
    axes: PortfolioAxes,
    max_configs: Option<usize>,
) -> Result<Vec<PortfolioConfig>, String> {
    if axes.market_count.is_empty()
        || axes.budget_fraction_ppm.is_empty()
        || axes.flow_concentration.is_empty()
    {
        return Err("portfolio axes must be non-empty".to_string());
    }
    if axes.market_count.contains(&0)
        || axes
            .budget_fraction_ppm
            .iter()
            .any(|&value| value == 0 || value > 1_000_000)
        || !axes.flow_concentration.iter().all(|value| {
            matches!(
                value.as_str(),
                "uniform" | "head-heavy" | "held-out-shuffled"
            )
        })
        || max_configs == Some(0)
    {
        return Err("portfolio axes contain an out-of-range value".to_string());
    }
    let mut configs = Vec::new();
    for &market_count in &axes.market_count {
        for &budget_fraction_ppm in &axes.budget_fraction_ppm {
            for concentration in &axes.flow_concentration {
                configs.push(PortfolioConfig {
                    market_count,
                    budget_fraction_ppm,
                    concentration: concentration.clone(),
                });
            }
        }
    }
    if let Some(limit) = max_configs {
        configs.truncate(limit);
    }
    Ok(configs)
}

fn generate_micro_tape(seed: u64, config: &MicroConfig) -> MicroTape {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let shock_at_ms = rng.random_range(1..config.batch_interval_ms.max(2));
    let up = rng.random_bool(0.5);
    let (p0, p1) = if config.regime == "jump" {
        let margin = 50_000_000;
        if up {
            let upper = MAX_PRICE.saturating_sub(config.jump_nanos + margin);
            let initial = rng.random_range(margin..=upper.max(margin));
            (
                initial,
                clamp_price(initial.saturating_add(config.jump_nanos)),
            )
        } else {
            let lower = config.jump_nanos.saturating_add(margin).min(MAX_PRICE);
            let initial = rng.random_range(lower..=MAX_PRICE);
            (
                initial,
                clamp_price(initial.saturating_sub(config.jump_nanos)),
            )
        }
    } else {
        let initial = rng.random_range(200_000_000..=800_000_000);
        (initial, initial)
    };

    let mut intents = Vec::new();
    if config.regime == "jump" {
        for index in 0..config.informed_trader_count {
            intents.push(TraderIntent {
                id: 100 + index as u64,
                market_index: 0,
                kind: TraderKind::Informed,
                side: if up { Side::Buy } else { Side::Sell },
                value_nanos: p1,
                limit_nanos: p1,
                quantity_units: shares_to_qty(MICRO_TRADER_SHARES).0,
                venue_arrival_ms: shock_at_ms
                    .saturating_add(config.taker_reaction_ms)
                    .saturating_add(index as u64),
                resting_until_ms: None,
            });
        }
    } else {
        for index in 0..4usize {
            let side = if index.is_multiple_of(2) {
                Side::Buy
            } else {
                Side::Sell
            };
            let distance = rng.random_range(10_000_000..=120_000_000);
            let value = match side {
                Side::Buy => clamp_price(p1.saturating_add(distance)),
                Side::Sell => clamp_price(p1.saturating_sub(distance)),
            };
            intents.push(TraderIntent {
                id: 100 + index as u64,
                market_index: 0,
                kind: TraderKind::Natural,
                side,
                value_nanos: value,
                limit_nanos: value,
                quantity_units: shares_to_qty(MICRO_TRADER_SHARES).0,
                venue_arrival_ms: rng.random_range(1..config.batch_interval_ms.max(2)),
                resting_until_ms: Some(config.batch_interval_ms),
            });
        }
    }
    intents.sort_by_key(|intent| (intent.venue_arrival_ms, intent.id));

    MicroTape {
        seed,
        regime: config.regime.clone(),
        initial_fundamental_nanos: p0,
        final_fundamental_nanos: p1,
        shock_at_ms,
        maker_replace_arrival_ms: shock_at_ms.saturating_add(config.maker_reaction_ms),
        batch_interval_ms: config.batch_interval_ms,
        maker_quote_quantity_units: shares_to_qty(MICRO_MAKER_SHARES).0,
        trader_intents: intents,
    }
}

fn run_micro_clob(tape: &MicroTape, spread: u64) -> EngineOutcome {
    #[derive(Clone, Copy)]
    struct RestingOrder {
        meta: OrderMeta,
        limit_nanos: u64,
        remaining_units: u64,
        sequence: u64,
    }

    let mut observations = Vec::new();
    let mut book = Vec::new();
    let mut next_sequence = 0u64;
    let post_maker_quotes =
        |book: &mut Vec<RestingOrder>, quote_fundamental: u64, next_sequence: &mut u64| {
            for side in [Side::Buy, Side::Sell] {
                let limit_nanos = match side {
                    Side::Buy => clamp_price(quote_fundamental.saturating_sub(spread)),
                    Side::Sell => clamp_price(quote_fundamental.saturating_add(spread)),
                };
                book.push(RestingOrder {
                    meta: OrderMeta {
                        role: Role::Maker,
                        side,
                        value_nanos: tape.final_fundamental_nanos,
                        submitted_at_ms: 0,
                        quote_fundamental_nanos: Some(quote_fundamental),
                        market_index: 0,
                    },
                    limit_nanos,
                    remaining_units: tape.maker_quote_quantity_units,
                    sequence: *next_sequence,
                });
                *next_sequence += 1;
            }
        };
    post_maker_quotes(
        &mut book,
        tape.initial_fundamental_nanos,
        &mut next_sequence,
    );
    let mut replaced = tape.regime != "jump";
    let mut last_price = None;

    for intent in &tape.trader_intents {
        if !replaced && tape.maker_replace_arrival_ms <= intent.venue_arrival_ms {
            book.retain(|order| !matches!(order.meta.role, Role::Maker));
            post_maker_quotes(&mut book, tape.final_fundamental_nanos, &mut next_sequence);
            replaced = true;
        }
        let incoming_meta = OrderMeta {
            role: Role::Trader(intent.kind),
            side: intent.side,
            value_nanos: intent.value_nanos,
            submitted_at_ms: intent.venue_arrival_ms,
            quote_fundamental_nanos: None,
            market_index: 0,
        };
        let mut remaining = intent.quantity_units;
        while remaining > 0 {
            let best = book
                .iter()
                .enumerate()
                .filter(|(_, resting)| {
                    resting.meta.side == opposite(intent.side)
                        && match intent.side {
                            Side::Buy => intent.limit_nanos >= resting.limit_nanos,
                            Side::Sell => intent.limit_nanos <= resting.limit_nanos,
                        }
                })
                .min_by(|(_, left), (_, right)| match intent.side {
                    Side::Buy => left
                        .limit_nanos
                        .cmp(&right.limit_nanos)
                        .then_with(|| left.sequence.cmp(&right.sequence)),
                    Side::Sell => right
                        .limit_nanos
                        .cmp(&left.limit_nanos)
                        .then_with(|| left.sequence.cmp(&right.sequence)),
                })
                .map(|(index, _)| index);
            let Some(best) = best else { break };
            let resting = book[best];
            let quantity = remaining.min(resting.remaining_units);
            remaining -= quantity;
            book[best].remaining_units -= quantity;
            observations.push(FillObservation {
                meta: resting.meta,
                quantity_units: quantity,
                price_nanos: resting.limit_nanos,
                executed_at_ms: intent.venue_arrival_ms,
            });
            observations.push(FillObservation {
                meta: incoming_meta,
                quantity_units: quantity,
                price_nanos: resting.limit_nanos,
                executed_at_ms: intent.venue_arrival_ms,
            });
            last_price = Some(resting.limit_nanos);
            if book[best].remaining_units == 0 {
                book.remove(best);
            }
        }
        if remaining > 0
            && intent
                .resting_until_ms
                .is_some_and(|expiry| expiry > intent.venue_arrival_ms)
        {
            book.push(RestingOrder {
                meta: incoming_meta,
                limit_nanos: intent.limit_nanos,
                remaining_units: remaining,
                sequence: next_sequence,
            });
            next_sequence += 1;
        }
    }

    EngineOutcome {
        status: completed_status(&observations),
        capital_consumed_nanos: maker_capital_consumed(&observations),
        observations,
        price_error_nanos: last_price.map(|price| price.abs_diff(tape.final_fundamental_nanos)),
        solver_evidence: Vec::new(),
        coverage: Coverage {
            displayed_ppm: 1_000_000,
            single_executable_ppm: 1_000_000,
            simultaneous_worst_case_ppm: 1_000_000,
        },
        capital_reserved_nanos: micro_quote_reserve(tape.initial_fundamental_nanos, spread),
    }
}

fn run_micro_fba(tape: &MicroTape, spread: u64, cancellable: bool) -> EngineOutcome {
    let mut observations = Vec::new();
    let mut evidence = Vec::new();
    let mut price_error = None;
    let mut status = "completed_zero_fill".to_string();

    for batch_index in 1..=2u64 {
        let lower = (batch_index - 1) * tape.batch_interval_ms;
        let cutoff = batch_index * tape.batch_interval_ms;
        let eligible: Vec<_> = tape
            .trader_intents
            .iter()
            .filter(|intent| intent.venue_arrival_ms > lower && intent.venue_arrival_ms <= cutoff)
            .collect();
        if eligible.is_empty() {
            continue;
        }
        let fresh = batch_index > 1
            || tape.regime != "jump"
            || (cancellable && tape.maker_replace_arrival_ms <= cutoff);
        let quote_fundamental = if fresh {
            tape.final_fundamental_nanos
        } else {
            tape.initial_fundamental_nanos
        };
        let solved = solve_fba_batch(
            batch_index,
            cutoff,
            &[quote_fundamental],
            &[tape.final_fundamental_nanos],
            spread,
            tape.maker_quote_quantity_units,
            &eligible,
            None,
        );
        match solved {
            Ok(mut batch) => {
                if !batch.solver.verifier_valid {
                    status = "verifier_invalid".to_string();
                } else if is_solver_failure(&batch.solver.termination) {
                    status = "solver_failure".to_string();
                }
                if !batch.observations.is_empty() && status == "completed_zero_fill" {
                    status = "completed".to_string();
                }
                if batch.price_error_nanos.is_some() {
                    price_error = batch.price_error_nanos;
                }
                observations.append(&mut batch.observations);
                evidence.push(batch.solver);
            }
            Err(message) => {
                status = "panic".to_string();
                evidence.push(SolverEvidence {
                    batch_index,
                    termination: "panic".to_string(),
                    message: Some(message),
                    wall_time_micros: 0,
                    verifier_valid: false,
                    violation_count: 0,
                    fills: 0,
                });
            }
        }
    }

    let consumed = maker_capital_consumed(&observations);
    EngineOutcome {
        status,
        observations,
        price_error_nanos: price_error,
        solver_evidence: evidence,
        coverage: Coverage {
            displayed_ppm: 1_000_000,
            single_executable_ppm: 1_000_000,
            simultaneous_worst_case_ppm: 1_000_000,
        },
        capital_reserved_nanos: micro_quote_reserve(tape.initial_fundamental_nanos, spread),
        capital_consumed_nanos: consumed,
    }
}

struct SolvedBatch {
    observations: Vec<FillObservation>,
    price_error_nanos: Option<u64>,
    solver: SolverEvidence,
}

#[allow(
    clippy::too_many_arguments,
    reason = "explicit research inputs keep paired assumptions auditable"
)]
fn solve_fba_batch(
    batch_index: u64,
    cutoff_ms: u64,
    quote_fundamentals: &[u64],
    mark_fundamentals: &[u64],
    spread: u64,
    maker_quantity_units: u64,
    traders: &[&TraderIntent],
    shared_budget_nanos: Option<u64>,
) -> Result<SolvedBatch, String> {
    if quote_fundamentals.len() != mark_fundamentals.len() {
        return Err("quote and mark fundamental lengths differ".to_string());
    }
    let mut problem = Problem::new(format!("market-structure-batch-{batch_index}"));
    let markets: Vec<_> = quote_fundamentals
        .iter()
        .enumerate()
        .map(|(index, _)| problem.markets.add_binary(format!("research-{index}")))
        .collect();
    let mut metadata = HashMap::new();
    let unconstrained_reserve = quote_fundamentals
        .iter()
        .map(|&fundamental| two_sided_quote_reserve(fundamental, spread, maker_quantity_units))
        .fold(0u64, u64::saturating_add);
    let mut mm = MmConstraint::new(
        MmId::new(1),
        Nanos(shared_budget_nanos.unwrap_or(unconstrained_reserve)),
    );
    let mut next_order_id = batch_index * 1_000_000 + 1;
    for (market_index, ((&market, &quote_fundamental), &mark_fundamental)) in markets
        .iter()
        .zip(quote_fundamentals)
        .zip(mark_fundamentals)
        .enumerate()
    {
        let bid_id = next_order_id;
        next_order_id += 1;
        let ask_id = next_order_id;
        next_order_id += 1;
        let bid = clamp_price(quote_fundamental.saturating_sub(spread));
        let ask = clamp_price(quote_fundamental.saturating_add(spread));
        problem.orders.push(outcome_buy(
            &problem.markets,
            bid_id,
            market,
            0,
            bid,
            maker_quantity_units,
        ));
        problem.orders.push(outcome_sell(
            &problem.markets,
            ask_id,
            market,
            0,
            ask,
            maker_quantity_units,
        ));
        mm.add_order(bid_id, MmSide::BuyYes);
        mm.add_order(ask_id, MmSide::SellYes);
        metadata.insert(
            bid_id,
            OrderMeta {
                role: Role::Maker,
                side: Side::Buy,
                value_nanos: mark_fundamental,
                submitted_at_ms: 0,
                quote_fundamental_nanos: Some(quote_fundamental),
                market_index,
            },
        );
        metadata.insert(
            ask_id,
            OrderMeta {
                role: Role::Maker,
                side: Side::Sell,
                value_nanos: mark_fundamental,
                submitted_at_ms: 0,
                quote_fundamental_nanos: Some(quote_fundamental),
                market_index,
            },
        );
    }
    problem.mm_constraints.push(mm);
    for intent in traders {
        let order_id = next_order_id;
        next_order_id += 1;
        let market = markets[intent.market_index];
        let order = match intent.side {
            Side::Buy => outcome_buy(
                &problem.markets,
                order_id,
                market,
                0,
                intent.limit_nanos,
                intent.quantity_units,
            ),
            Side::Sell => outcome_sell(
                &problem.markets,
                order_id,
                market,
                0,
                intent.limit_nanos,
                intent.quantity_units,
            ),
        };
        problem.orders.push(order);
        metadata.insert(
            order_id,
            OrderMeta {
                role: Role::Trader(intent.kind),
                side: intent.side,
                value_nanos: intent.value_nanos,
                submitted_at_ms: intent.venue_arrival_ms,
                quote_fundamental_nanos: None,
                market_index: intent.market_index,
            },
        );
    }

    let started = Instant::now();
    let attempted = catch_unwind(AssertUnwindSafe(|| ProductionSolver::new().solve(&problem)));
    let elapsed = started.elapsed().as_micros();
    let pipeline = attempted.map_err(panic_message)?;
    let witness = witness_from_pipeline(&problem, &pipeline);
    let verification = verify_match(&witness, false);
    let termination = termination_name(&pipeline.diagnostics.status).to_string();
    let mut observations = Vec::new();
    for fill in &pipeline.result.fills {
        if fill.fill_qty.0 == 0 {
            continue;
        }
        let Some(meta) = metadata.get(&fill.order_id).copied() else {
            continue;
        };
        observations.push(FillObservation {
            meta,
            quantity_units: fill.fill_qty.0,
            price_nanos: fill.fill_price.0,
            executed_at_ms: cutoff_ms,
        });
    }
    let trader_filled_markets: HashSet<_> = observations
        .iter()
        .filter_map(|observation| {
            matches!(observation.meta.role, Role::Trader(_))
                .then_some(observation.meta.market_index)
        })
        .collect();
    let price_error_nanos = pipeline.price_discovery.as_ref().and_then(|discovery| {
        let errors: Vec<_> = trader_filled_markets
            .iter()
            .filter_map(|&index| {
                discovery
                    .prices
                    .get(&markets[index])
                    .and_then(|prices| prices.first())
                    .map(|price| price.0.abs_diff(mark_fundamentals[index]))
            })
            .collect();
        (!errors.is_empty()).then(|| errors.iter().sum::<u64>() / errors.len() as u64)
    });
    Ok(SolvedBatch {
        observations,
        price_error_nanos,
        solver: SolverEvidence {
            batch_index,
            termination,
            message: pipeline.diagnostics.message.clone(),
            wall_time_micros: elapsed,
            verifier_valid: verification.valid,
            violation_count: verification.violations.len(),
            fills: pipeline.result.fills.len(),
        },
    })
}

fn generate_portfolio_tape(seed: u64, config: &PortfolioConfig) -> PortfolioTape {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let fundamentals: Vec<u64> = (0..config.market_count)
        .map(|_| rng.random_range(100_000_000u64..=900_000_000u64))
        .collect();
    let head = config.market_count.div_ceil(5).max(1);
    let expected: Vec<u64> = (0..config.market_count)
        .map(|index| match config.concentration.as_str() {
            "uniform" => 5,
            "head-heavy" | "held-out-shuffled" => {
                if index < head {
                    10
                } else {
                    2
                }
            }
            other => panic!("unknown flow concentration {other}"),
        })
        .collect();
    let mut realized = expected.clone();
    if config.concentration == "held-out-shuffled" {
        realized.shuffle(&mut rng);
    }
    let mut intents = Vec::with_capacity(config.market_count);
    for (market_index, &fundamental) in fundamentals.iter().enumerate() {
        let side = if rng.random_bool(0.5) {
            Side::Buy
        } else {
            Side::Sell
        };
        let willingness = rng.random_range(50_000_000u64..=150_000_000u64);
        let value = match side {
            Side::Buy => clamp_price(fundamental.saturating_add(willingness)),
            Side::Sell => clamp_price(fundamental.saturating_sub(willingness)),
        };
        intents.push(TraderIntent {
            id: 100 + market_index as u64,
            market_index,
            kind: TraderKind::Natural,
            side,
            value_nanos: value,
            limit_nanos: value,
            quantity_units: shares_to_qty(realized[market_index]).0,
            venue_arrival_ms: market_index as u64,
            resting_until_ms: None,
        });
    }
    let mut arrival_order: Vec<_> = (0..config.market_count).collect();
    arrival_order.shuffle(&mut rng);
    PortfolioTape {
        seed,
        concentration: config.concentration.clone(),
        fundamentals_nanos: fundamentals,
        expected_flow_scores: expected,
        realized_flow_scores: realized,
        trader_intents: intents,
        arrival_order,
    }
}

fn portfolio_reserve_by_market(tape: &PortfolioTape) -> Vec<u64> {
    tape.fundamentals_nanos
        .iter()
        .map(|&fundamental| {
            two_sided_quote_reserve(
                fundamental,
                PORTFOLIO_HALF_SPREAD_NANOS,
                shares_to_qty(PORTFOLIO_MAKER_SHARES).0,
            )
        })
        .collect()
}

fn portfolio_single_quote_exposure_by_market(tape: &PortfolioTape) -> Vec<u64> {
    let quantity = shares_to_qty(PORTFOLIO_MAKER_SHARES).0;
    tape.fundamentals_nanos
        .iter()
        .map(|&fundamental| {
            let bid = clamp_price(fundamental.saturating_sub(PORTFOLIO_HALF_SPREAD_NANOS));
            let ask = clamp_price(fundamental.saturating_add(PORTFOLIO_HALF_SPREAD_NANOS));
            fill_capital(Side::Buy, bid, quantity).max(fill_capital(Side::Sell, ask, quantity))
        })
        .collect()
}

fn run_portfolio_fba(
    tape: &PortfolioTape,
    budget: u64,
    reserve_by_market: &[u64],
    single_exposure_by_market: &[u64],
) -> EngineOutcome {
    let trader_refs: Vec<_> = tape.trader_intents.iter().collect();
    let solved = solve_fba_batch(
        1,
        PORTFOLIO_BATCH_INTERVAL_MS,
        &tape.fundamentals_nanos,
        &tape.fundamentals_nanos,
        PORTFOLIO_HALF_SPREAD_NANOS,
        shares_to_qty(PORTFOLIO_MAKER_SHARES).0,
        &trader_refs,
        Some(budget),
    );
    let coverage = portfolio_coverage(
        tape.fundamentals_nanos.len(),
        budget,
        reserve_by_market,
        single_exposure_by_market,
        true,
    );
    match solved {
        Ok(batch) => {
            let status = if !batch.solver.verifier_valid {
                "verifier_invalid".to_string()
            } else if is_solver_failure(&batch.solver.termination) {
                "solver_failure".to_string()
            } else {
                completed_status(&batch.observations)
            };
            let consumed = maker_capital_consumed(&batch.observations);
            EngineOutcome {
                status,
                observations: batch.observations,
                price_error_nanos: batch.price_error_nanos,
                solver_evidence: vec![batch.solver],
                coverage,
                capital_reserved_nanos: budget,
                capital_consumed_nanos: consumed,
            }
        }
        Err(message) => EngineOutcome {
            status: "panic".to_string(),
            observations: Vec::new(),
            price_error_nanos: None,
            solver_evidence: vec![SolverEvidence {
                batch_index: 1,
                termination: "panic".to_string(),
                message: Some(message),
                wall_time_micros: 0,
                verifier_valid: false,
                violation_count: 0,
                fills: 0,
            }],
            coverage,
            capital_reserved_nanos: budget,
            capital_consumed_nanos: 0,
        },
    }
}

fn run_portfolio_clob(
    tape: &PortfolioTape,
    budget: u64,
    reserve_by_market: &[u64],
    single_exposure_by_market: &[u64],
    firm_reserve: bool,
) -> EngineOutcome {
    let market_count = tape.fundamentals_nanos.len();
    let selected: HashSet<usize> = if firm_reserve {
        let mut ranked: Vec<_> = (0..market_count).collect();
        ranked.sort_by(|&left, &right| {
            let left_score =
                tape.expected_flow_scores[left] as u128 * reserve_by_market[right] as u128;
            let right_score =
                tape.expected_flow_scores[right] as u128 * reserve_by_market[left] as u128;
            right_score.cmp(&left_score).then_with(|| left.cmp(&right))
        });
        let mut reserved = 0u64;
        ranked
            .into_iter()
            .filter(|&index| {
                let next = reserved.saturating_add(reserve_by_market[index]);
                if next <= budget {
                    reserved = next;
                    true
                } else {
                    false
                }
            })
            .collect()
    } else {
        (0..market_count).collect()
    };
    let reserved = if firm_reserve {
        selected.iter().map(|&index| reserve_by_market[index]).sum()
    } else {
        budget
    };
    let mut remaining_budget = budget;
    let mut observations = Vec::new();
    let mut execution_price_error_weighted = 0u128;
    let mut execution_price_error_quantity = 0u128;
    for &intent_index in &tape.arrival_order {
        let intent = &tape.trader_intents[intent_index];
        if !selected.contains(&intent.market_index) {
            continue;
        }
        let fundamental = tape.fundamentals_nanos[intent.market_index];
        let price = match intent.side {
            Side::Buy => clamp_price(fundamental.saturating_add(PORTFOLIO_HALF_SPREAD_NANOS)),
            Side::Sell => clamp_price(fundamental.saturating_sub(PORTFOLIO_HALF_SPREAD_NANOS)),
        };
        let crosses = match intent.side {
            Side::Buy => intent.limit_nanos >= price,
            Side::Sell => intent.limit_nanos <= price,
        };
        if !crosses {
            continue;
        }
        let quantity = intent
            .quantity_units
            .min(shares_to_qty(PORTFOLIO_MAKER_SHARES).0);
        let maker_side = opposite(intent.side);
        let quantity = if firm_reserve {
            quantity
        } else {
            quantity.min(max_quantity_for_capital(
                maker_side,
                price,
                remaining_budget,
            ))
        };
        if quantity == 0 {
            continue;
        }
        let capital = fill_capital(maker_side, price, quantity);
        remaining_budget = remaining_budget.saturating_sub(capital);
        observations.push(FillObservation {
            meta: OrderMeta {
                role: Role::Maker,
                side: opposite(intent.side),
                value_nanos: fundamental,
                submitted_at_ms: 0,
                quote_fundamental_nanos: Some(fundamental),
                market_index: intent.market_index,
            },
            quantity_units: quantity,
            price_nanos: price,
            executed_at_ms: intent.venue_arrival_ms,
        });
        observations.push(FillObservation {
            meta: OrderMeta {
                role: Role::Trader(intent.kind),
                side: intent.side,
                value_nanos: intent.value_nanos,
                submitted_at_ms: intent.venue_arrival_ms,
                quote_fundamental_nanos: None,
                market_index: intent.market_index,
            },
            quantity_units: quantity,
            price_nanos: price,
            executed_at_ms: intent.venue_arrival_ms,
        });
        execution_price_error_weighted = execution_price_error_weighted
            .saturating_add(price.abs_diff(fundamental) as u128 * quantity as u128);
        execution_price_error_quantity += quantity as u128;
    }
    let coverage = if firm_reserve {
        Coverage {
            displayed_ppm: ratio_ppm(selected.len() as u64, market_count as u64),
            single_executable_ppm: ratio_ppm(selected.len() as u64, market_count as u64),
            simultaneous_worst_case_ppm: ratio_ppm(selected.len() as u64, market_count as u64),
        }
    } else {
        portfolio_coverage(
            market_count,
            budget,
            reserve_by_market,
            single_exposure_by_market,
            true,
        )
    };
    let consumed = maker_capital_consumed(&observations);
    EngineOutcome {
        status: completed_status(&observations),
        observations,
        price_error_nanos: (execution_price_error_quantity > 0)
            .then(|| (execution_price_error_weighted / execution_price_error_quantity) as u64),
        solver_evidence: Vec::new(),
        coverage,
        capital_reserved_nanos: reserved,
        capital_consumed_nanos: consumed,
    }
}

fn portfolio_coverage(
    market_count: usize,
    budget: u64,
    reserve_by_market: &[u64],
    single_exposure_by_market: &[u64],
    displayed_all: bool,
) -> Coverage {
    let single = single_exposure_by_market
        .iter()
        .filter(|&&reserve| reserve <= budget)
        .count();
    let mut cheapest = reserve_by_market.to_vec();
    cheapest.sort_unstable();
    let mut total = 0u64;
    let simultaneous = cheapest
        .into_iter()
        .take_while(|reserve| {
            let next = total.saturating_add(*reserve);
            if next <= budget {
                total = next;
                true
            } else {
                false
            }
        })
        .count();
    Coverage {
        displayed_ppm: if displayed_all { 1_000_000 } else { 0 },
        single_executable_ppm: ratio_ppm(single as u64, market_count as u64),
        simultaneous_worst_case_ppm: ratio_ppm(simultaneous as u64, market_count as u64),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "a flat artifact schema keeps provenance fields reviewable"
)]
fn record(
    protocol: &Protocol,
    protocol_hash: &str,
    suite: &str,
    case_id: &str,
    seed: u64,
    tape_hash: &str,
    engine: &str,
    regime: &str,
    parameters: Value,
    outcome: EngineOutcome,
    intents: &[TraderIntent],
    final_fundamentals: &[u64],
) -> RunRecord {
    let metrics = build_metrics(&outcome, intents, final_fundamentals);
    RunRecord {
        record_schema_version: 1,
        protocol_schema_version: protocol.schema_version,
        protocol_id: protocol.protocol_id.clone(),
        protocol_status: protocol.status.clone(),
        protocol_blake3: protocol_hash.to_string(),
        suite: suite.to_string(),
        case_id: case_id.to_string(),
        seed,
        tape_blake3: tape_hash.to_string(),
        engine: engine.to_string(),
        regime: regime.to_string(),
        parameters,
        run_status: outcome.status,
        solver_evidence: outcome.solver_evidence,
        metrics,
    }
}

fn build_metrics(
    outcome: &EngineOutcome,
    intents: &[TraderIntent],
    final_fundamentals: &[u64],
) -> Metrics {
    let submitted: u64 = intents.iter().map(|intent| intent.quantity_units).sum();
    let mut metrics = Metrics {
        submitted_trader_quantity_units: submitted,
        displayed_quote_market_coverage_ppm: outcome.coverage.displayed_ppm,
        single_market_executable_coverage_ppm: outcome.coverage.single_executable_ppm,
        simultaneous_worst_case_coverage_ppm: outcome.coverage.simultaneous_worst_case_ppm,
        capital_reserved_nanos: outcome.capital_reserved_nanos,
        capital_consumed_nanos: outcome.capital_consumed_nanos,
        ..Metrics::default()
    };
    let mut delay_weighted = 0u128;
    let mut filled_markets = HashSet::new();
    for observation in &outcome.observations {
        let delta = match observation.meta.side {
            Side::Buy => observation.meta.value_nanos as i64 - observation.price_nanos as i64,
            Side::Sell => observation.price_nanos as i64 - observation.meta.value_nanos as i64,
        };
        let pnl = signed_notional(delta, observation.quantity_units);
        match observation.meta.role {
            Role::Maker => {
                metrics.maker_markout_pnl_nanos =
                    metrics.maker_markout_pnl_nanos.saturating_add(pnl);
                metrics.maker_filled_quantity_units = metrics
                    .maker_filled_quantity_units
                    .saturating_add(observation.quantity_units);
                filled_markets.insert(observation.meta.market_index);
                if observation.meta.quote_fundamental_nanos
                    != Some(final_fundamentals[observation.meta.market_index])
                    && pnl < 0
                {
                    metrics.maker_stale_quote_loss_nanos = metrics
                        .maker_stale_quote_loss_nanos
                        .saturating_add(pnl.unsigned_abs());
                }
            }
            Role::Trader(kind) => {
                metrics.filled_trader_quantity_units = metrics
                    .filled_trader_quantity_units
                    .saturating_add(observation.quantity_units);
                match kind {
                    TraderKind::Natural => {
                        metrics.natural_trader_surplus_nanos =
                            metrics.natural_trader_surplus_nanos.saturating_add(pnl);
                    }
                    TraderKind::Informed => {
                        metrics.informed_trader_surplus_nanos =
                            metrics.informed_trader_surplus_nanos.saturating_add(pnl);
                    }
                }
                delay_weighted = delay_weighted.saturating_add(
                    observation
                        .executed_at_ms
                        .saturating_sub(observation.meta.submitted_at_ms)
                        as u128
                        * observation.quantity_units as u128,
                );
            }
        }
    }
    metrics.maker_pnl_per_filled_share_nanos =
        (metrics.maker_filled_quantity_units > 0).then(|| {
            ((metrics.maker_markout_pnl_nanos as i128 * SHARE_SCALE as i128)
                / metrics.maker_filled_quantity_units as i128) as i64
        });
    metrics.fill_rate_ppm = ratio_ppm(metrics.filled_trader_quantity_units, submitted);
    metrics.execution_delay_ms = (metrics.filled_trader_quantity_units > 0)
        .then(|| (delay_weighted / metrics.filled_trader_quantity_units as u128) as u64);
    metrics.post_window_price_error_nanos = outcome.price_error_nanos;
    metrics.filled_market_coverage_ppm =
        ratio_ppm(filled_markets.len() as u64, final_fundamentals.len() as u64);
    metrics
}

fn maker_capital_consumed(observations: &[FillObservation]) -> u64 {
    observations
        .iter()
        .filter(|observation| matches!(observation.meta.role, Role::Maker))
        .map(|observation| {
            fill_capital(
                observation.meta.side,
                observation.price_nanos,
                observation.quantity_units,
            )
        })
        .fold(0u64, u64::saturating_add)
}

fn fill_capital(side: Side, price: u64, quantity: u64) -> u64 {
    let capital_price = match side {
        Side::Buy => price,
        Side::Sell => NANOS_PER_DOLLAR.saturating_sub(price),
    };
    ((capital_price as u128 * quantity as u128).div_ceil(SHARE_SCALE as u128)) as u64
}

fn max_quantity_for_capital(side: Side, price: u64, capital: u64) -> u64 {
    let capital_price = match side {
        Side::Buy => price,
        Side::Sell => NANOS_PER_DOLLAR.saturating_sub(price),
    };
    if capital_price == 0 {
        return u64::MAX;
    }
    ((capital as u128 * SHARE_SCALE as u128) / capital_price as u128) as u64
}

fn two_sided_quote_reserve(fundamental: u64, spread: u64, quantity: u64) -> u64 {
    let bid = clamp_price(fundamental.saturating_sub(spread));
    let ask = clamp_price(fundamental.saturating_add(spread));
    fill_capital(Side::Buy, bid, quantity).saturating_add(fill_capital(Side::Sell, ask, quantity))
}

fn micro_quote_reserve(fundamental: u64, spread: u64) -> u64 {
    two_sided_quote_reserve(fundamental, spread, shares_to_qty(MICRO_MAKER_SHARES).0)
}

fn signed_notional(delta_nanos: i64, quantity_units: u64) -> i64 {
    ((delta_nanos as i128 * quantity_units as i128) / SHARE_SCALE as i128) as i64
}

fn clamp_price(price: u64) -> u64 {
    price.clamp(MIN_PRICE, MAX_PRICE)
}

fn opposite(side: Side) -> Side {
    match side {
        Side::Buy => Side::Sell,
        Side::Sell => Side::Buy,
    }
}

fn ratio_ppm(numerator: u64, denominator: u64) -> u64 {
    if denominator == 0 {
        return 0;
    }
    ((numerator as u128 * 1_000_000u128) / denominator as u128) as u64
}

fn mul_ppm(value: u64, ppm: u64) -> u64 {
    ((value as u128 * ppm as u128) / 1_000_000u128) as u64
}

fn completed_status(observations: &[FillObservation]) -> String {
    if observations.is_empty() {
        "completed_zero_fill".to_string()
    } else {
        "completed".to_string()
    }
}

fn termination_name(status: &TerminationStatus) -> &'static str {
    match status {
        TerminationStatus::EmptyInput => "empty_input",
        TerminationStatus::UnsupportedInput => "unsupported_input",
        TerminationStatus::Converged => "converged",
        TerminationStatus::IterationLimit => "iteration_limit",
        TerminationStatus::TimeLimit => "time_limit",
        TerminationStatus::Infeasible => "infeasible",
        TerminationStatus::NumericalFailure => "numerical_failure",
        TerminationStatus::PostProcessingFailure => "post_processing_failure",
        TerminationStatus::Delegated => "delegated",
        TerminationStatus::NotReported => "not_reported",
    }
}

fn is_solver_failure(termination: &str) -> bool {
    matches!(
        termination,
        "unsupported_input" | "infeasible" | "numerical_failure" | "post_processing_failure"
    )
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

fn fingerprint<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_vec(value).map(|bytes| blake3::hash(&bytes).to_hex().to_string())
}

fn micro_case_id(config: &MicroConfig) -> String {
    format!(
        "micro-{}-b{}-s{}-mr{}-tr{}-n{}-j{}",
        config.regime,
        config.batch_interval_ms,
        config.quote_half_spread_nanos,
        config.maker_reaction_ms,
        config.taker_reaction_ms,
        config.informed_trader_count,
        config.jump_nanos,
    )
}

fn portfolio_case_id(config: &PortfolioConfig) -> String {
    format!(
        "portfolio-m{}-b{}-{}",
        config.market_count, config.budget_fraction_ppm, config.concentration
    )
}

fn write_record(
    output: &mut BufWriter<File>,
    record: RunRecord,
) -> Result<(), Box<dyn std::error::Error>> {
    serde_json::to_writer(&mut *output, &record)?;
    output.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn micro_config(regime: &str) -> MicroConfig {
        MicroConfig {
            regime: regime.to_string(),
            batch_interval_ms: 500,
            quote_half_spread_nanos: 20_000_000,
            maker_reaction_ms: 100,
            taker_reaction_ms: 25,
            informed_trader_count: 4,
            jump_nanos: 300_000_000,
        }
    }

    #[test]
    fn paired_micro_tape_is_reproducible() {
        let config = micro_config("jump");
        assert_eq!(
            fingerprint(&generate_micro_tape(7, &config)).unwrap(),
            fingerprint(&generate_micro_tape(7, &config)).unwrap()
        );
    }

    #[test]
    fn faster_clob_cancel_prevents_stale_fill() {
        let mut config = micro_config("jump");
        config.maker_reaction_ms = 1;
        config.taker_reaction_ms = 100;
        let tape = generate_micro_tape(9, &config);
        let outcome = run_micro_clob(&tape, config.quote_half_spread_nanos);
        let metrics = build_metrics(
            &outcome,
            &tape.trader_intents,
            &[tape.final_fundamental_nanos],
        );
        assert_eq!(metrics.maker_stale_quote_loss_nanos, 0);
    }

    #[test]
    fn current_fba_keeps_stale_bundle_when_counterfactual_can_replace() {
        let mut config = micro_config("jump");
        config.maker_reaction_ms = 1;
        let tape = generate_micro_tape(11, &config);
        let current = run_micro_fba(&tape, config.quote_half_spread_nanos, false);
        let counterfactual = run_micro_fba(&tape, config.quote_half_spread_nanos, true);
        let current_stale = build_metrics(
            &current,
            &tape.trader_intents,
            &[tape.final_fundamental_nanos],
        )
        .maker_stale_quote_loss_nanos;
        let counterfactual_stale = build_metrics(
            &counterfactual,
            &tape.trader_intents,
            &[tape.final_fundamental_nanos],
        )
        .maker_stale_quote_loss_nanos;
        assert!(current_stale >= counterfactual_stale);
    }

    #[test]
    fn shared_risk_coverage_labels_do_not_claim_simultaneous_firmness() {
        let config = PortfolioConfig {
            market_count: 8,
            budget_fraction_ppm: 250_000,
            concentration: "uniform".to_string(),
        };
        let tape = generate_portfolio_tape(3, &config);
        let reserves = portfolio_reserve_by_market(&tape);
        let single_exposure = portfolio_single_quote_exposure_by_market(&tape);
        let budget = mul_ppm(reserves.iter().sum(), config.budget_fraction_ppm);
        let outcome = run_portfolio_clob(&tape, budget, &reserves, &single_exposure, false);
        assert_eq!(outcome.coverage.displayed_ppm, 1_000_000);
        assert!(outcome.coverage.simultaneous_worst_case_ppm < 1_000_000);
    }

    #[test]
    fn natural_clob_limits_can_rest_and_match_without_maker() {
        let tape = MicroTape {
            seed: 1,
            regime: "quiet".to_string(),
            initial_fundamental_nanos: 500_000_000,
            final_fundamental_nanos: 500_000_000,
            shock_at_ms: 100,
            maker_replace_arrival_ms: 200,
            batch_interval_ms: 500,
            maker_quote_quantity_units: shares_to_qty(10).0,
            trader_intents: vec![
                TraderIntent {
                    id: 100,
                    market_index: 0,
                    kind: TraderKind::Natural,
                    side: Side::Buy,
                    value_nanos: 600_000_000,
                    limit_nanos: 600_000_000,
                    quantity_units: shares_to_qty(5).0,
                    venue_arrival_ms: 10,
                    resting_until_ms: Some(500),
                },
                TraderIntent {
                    id: 101,
                    market_index: 0,
                    kind: TraderKind::Natural,
                    side: Side::Sell,
                    value_nanos: 400_000_000,
                    limit_nanos: 400_000_000,
                    quantity_units: shares_to_qty(5).0,
                    venue_arrival_ms: 20,
                    resting_until_ms: Some(500),
                },
            ],
        };
        let outcome = run_micro_clob(&tape, 200_000_000);
        let metrics = build_metrics(&outcome, &tape.trader_intents, &[500_000_000]);
        assert_eq!(metrics.filled_trader_quantity_units, shares_to_qty(10).0);
        assert_eq!(metrics.maker_filled_quantity_units, 0);
    }

    #[test]
    fn development_runner_rejects_seed_outside_active_range() {
        let protocol = Protocol {
            schema_version: 1,
            protocol_id: "development".to_string(),
            status: "development-only-not-evidence".to_string(),
            episode_families: Vec::new(),
            development_seeds: Some(SeedRange {
                range_inclusive: [0, 31],
            }),
            run_seeds: None,
            held_out_embargo: Some(SeedRange {
                range_inclusive: [10_000, 10_255],
            }),
        };
        let cli = Cli {
            protocol: PathBuf::new(),
            output: PathBuf::new(),
            suite: SuiteChoice::All,
            seed_start: Some(10_000),
            seed_count: Some(1),
            max_configs: Some(1),
        };
        assert!(selected_seeds(&protocol, &cli).is_err());
    }
}
