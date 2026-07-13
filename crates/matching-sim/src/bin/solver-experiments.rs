//! Reproducible, preregistered solver experiment runner.
//!
//! This binary deliberately differs from `matching-sim`: it consumes a frozen
//! protocol, emits one machine-readable record for every declared run, retains
//! failures and timeouts, and never substitutes one solver for another.

#[path = "../witness.rs"]
mod witness;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use matching_engine::{Fill, MarketId, NANOS_PER_DOLLAR, Nanos, Problem, Qty, notional_nanos};
use matching_scenarios::{
    FlashLiquidityConfig, ScenarioConfig, generate_flash_liquidity_scenario, generate_scenario,
};
use matching_solver::{
    ConicConfig, ConicSolver, DecomposedSolver, EgConfig, EgSolver, IterLpConfig, IterLpSolver,
    LpConfig, LpSolver, MilpConfig, MilpSolver, MmBudgetMode, ObjectiveMode, PipelineResult,
    RetainedCashConfig, RetainedCashSolver, SolveStatus, TerminationStatus,
    retained_cash_objective_for_fills, retained_cash_welfare_gap_bound_for_fills,
};
use serde::{Deserialize, Serialize};
use sybil_verifier::verify_match;

use witness::{witness_from_milp, witness_from_pipeline};

#[derive(Parser, Debug)]
#[command(about = "Run the preregistered solver benchmark protocol without hidden fallbacks")]
struct Args {
    #[arg(long, default_value = "benchmarks/solver/protocol-v1.json")]
    protocol: PathBuf,

    #[arg(long)]
    output_dir: PathBuf,

    /// Immutable implementation revision used for the run. Required outside smoke mode.
    #[arg(long, default_value = "working-copy")]
    source_revision: String,

    /// Exercise one seed and one budget point from every experiment.
    #[arg(long)]
    smoke: bool,

    /// Replace an existing output directory.
    #[arg(long)]
    overwrite: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Protocol {
    schema_version: u32,
    protocol_id: String,
    title: String,
    analysis_policy: serde_json::Value,
    warmup: WarmupSpec,
    solvers: BTreeMap<String, SolverSpec>,
    scales: BTreeMap<String, ScaleSpec>,
    profiles: BTreeMap<String, ProfileSpec>,
    experiments: Vec<ExperimentSpec>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct WarmupSpec {
    profile: String,
    scale: String,
    seed: u64,
    budget_scale: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SolverSpec {
    kind: String,
    label: String,
    max_iterations: Option<usize>,
    inner_max_iterations: Option<usize>,
    tolerance: Option<f64>,
    inner_tolerance: Option<f64>,
    q_tolerance: Option<f64>,
    damping: Option<f64>,
    line_search_steps: Option<usize>,
    time_limit_seconds: Option<f64>,
    gap_tolerance: Option<f64>,
    absolute_tolerance: Option<f64>,
    fallback_to_lp: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ScaleSpec {
    num_markets: usize,
    num_orders: usize,
    num_mms: usize,
    order_size_min: u64,
    order_size_max: u64,
    mm_budget_min_dollars: u64,
    mm_budget_max_dollars: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ProfileSpec {
    generator: Option<String>,
    market_group_probability: f64,
    order_size_power: f64,
    retail_buy_probability: f64,
    liquidity_scarcity: f64,
    hot_market_fraction: f64,
    hot_order_probability: f64,
    liquidity_depth_levels: usize,
    liquidity_dispersion: f64,
    mm_spread_bps: u32,
    mm_capacity_multiplier: u64,
    mm_market_coverage_fraction: f64,
    mm_market_coverage_max: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ExperimentSpec {
    id: String,
    suite: String,
    profile: String,
    scale: String,
    seed_start: u64,
    seed_count: usize,
    budget_scales: Vec<f64>,
    budget_basis: Option<String>,
    solvers: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ProblemMetrics {
    markets: usize,
    orders: usize,
    declared_retail_orders: usize,
    buy_orders: usize,
    sell_orders: usize,
    mm_orders: usize,
    market_groups: usize,
    grouped_markets: usize,
    total_max_fill: u64,
    total_mm_budget_nanos: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
struct PhaseMetrics {
    price_discovery_seconds: f64,
    allocation_seconds: f64,
    partial_solving_seconds: f64,
    combining_seconds: f64,
}

#[derive(Clone, Debug, Serialize)]
struct MmUtilization {
    mm_id: u64,
    budget_nanos: u64,
    capital_used_nanos: Option<u64>,
    utilization: Option<f64>,
    limit_value_nanos: u64,
    limit_value_to_budget: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize)]
struct ComparisonMetrics {
    lp_welfare_gap_bps: Option<f64>,
    lp_allocation_l1_ratio: Option<f64>,
    lp_price_mae_nanos: Option<f64>,
    observed_best_welfare_gap_bps: Option<f64>,
    unconstrained_lp_welfare_gap_bps: Option<f64>,
    observed_best_retained_cash_objective_gap_bps: Option<f64>,
    milp_welfare_gap_bps: Option<f64>,
    paper_welfare_bound_nanos: Option<f64>,
    paper_bound_ratio: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
struct RunRecord {
    schema_version: u32,
    protocol_id: String,
    experiment_id: String,
    suite: String,
    profile: String,
    scale: String,
    seed: u64,
    budget_scale: f64,
    scenario_fingerprint_blake3: String,
    solver_id: String,
    solver_label: String,
    solver_config: SolverSpec,
    solver_position: usize,
    run_status: String,
    benchmark_success: bool,
    termination: String,
    iterations: Option<usize>,
    convergence_metric: Option<f64>,
    objective_value: Option<f64>,
    optimality_gap: Option<f64>,
    oracle_calls: Option<usize>,
    integer_landing_loss: Option<f64>,
    primal_residual: Option<f64>,
    dual_residual: Option<f64>,
    solver_message: Option<String>,
    panic_message: Option<String>,
    milp_gap_percent: Option<f64>,
    wall_time_seconds: f64,
    solver_reported_time_seconds: Option<f64>,
    phase_times: Option<PhaseMetrics>,
    verifier_valid: bool,
    violation_count: usize,
    violation_kinds: Vec<String>,
    violation_details: Vec<String>,
    verifier_computed_welfare_nanos: Option<i64>,
    gross_welfare_nanos: Option<i64>,
    signed_minting_cost_nanos: Option<i64>,
    net_welfare_nanos: Option<i64>,
    retained_cash_objective_nanos: Option<f64>,
    retail_gross_welfare_nanos: Option<i64>,
    mm_gross_welfare_nanos: Option<i64>,
    fills: Option<usize>,
    filled_quantity: Option<u64>,
    group_price_avg_delta_nanos: Option<u64>,
    mm_utilization: Vec<MmUtilization>,
    problem: ProblemMetrics,
    comparisons: ComparisonMetrics,
}

struct InternalRun {
    record: RunRecord,
    allocation: BTreeMap<u64, u64>,
    prices: HashMap<MarketId, Vec<Nanos>>,
}

struct SolveOutput {
    result: matching_solver::MatchingResult,
    prices: HashMap<MarketId, Vec<Nanos>>,
    reported_time: f64,
    phases: PhaseMetrics,
    termination: String,
    iterations: Option<usize>,
    convergence_metric: Option<f64>,
    objective_value: Option<f64>,
    optimality_gap: Option<f64>,
    oracle_calls: Option<usize>,
    integer_landing_loss: Option<f64>,
    primal_residual: Option<f64>,
    dual_residual: Option<f64>,
    message: Option<String>,
    milp_gap_percent: Option<f64>,
    witness: sybil_verifier::BlockWitness,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if !args.smoke && args.source_revision == "working-copy" {
        return Err("--source-revision is required for a full protocol run".into());
    }

    let protocol_bytes = fs::read(&args.protocol)?;
    let protocol: Protocol = serde_json::from_slice(&protocol_bytes)?;
    validate_protocol(&protocol)?;
    prepare_output_dir(&args.output_dir, args.overwrite)?;
    fs::write(args.output_dir.join("protocol.json"), &protocol_bytes)?;

    let start_unix = unix_seconds();
    let selected = selected_experiments(&protocol, args.smoke);
    let expected_records: usize = selected
        .iter()
        .map(|experiment| {
            experiment.seed_count * experiment.budget_scales.len() * experiment.solvers.len()
        })
        .sum();

    eprintln!(
        "protocol={} mode={} expected_records={}",
        protocol.protocol_id,
        if args.smoke { "smoke" } else { "full" },
        expected_records
    );

    run_warmups(&protocol, &selected)?;

    let results_path = args.output_dir.join("results.jsonl");
    let mut output = BufWriter::new(File::create(&results_path)?);
    let mut records_written = 0usize;

    for experiment in &selected {
        let profile = protocol
            .profiles
            .get(&experiment.profile)
            .ok_or_else(|| format!("missing profile {}", experiment.profile))?;
        let scale = protocol
            .scales
            .get(&experiment.scale)
            .ok_or_else(|| format!("missing scale {}", experiment.scale))?;

        for seed_offset in 0..experiment.seed_count {
            let seed = experiment.seed_start + seed_offset as u64;
            let base_problem = generate_problem(seed, scale, profile);
            let calibrated_budgets = match experiment.budget_basis.as_deref() {
                Some("lp_limit_value") => Some(unconstrained_lp_limit_values(&base_problem)),
                None | Some("generated") => None,
                Some(other) => panic!("unknown budget basis {other}"),
            };

            for (budget_index, &budget_scale) in experiment.budget_scales.iter().enumerate() {
                let mut problem = base_problem.clone();
                apply_mm_budgets(&mut problem, budget_scale, calibrated_budgets.as_deref());
                let fingerprint = problem_fingerprint(&problem)?;

                let mut solver_ids = experiment.solvers.clone();
                let solver_count = solver_ids.len();
                if solver_count > 1 {
                    solver_ids.rotate_left((seed as usize + budget_index) % solver_count);
                }

                let mut group_runs = Vec::with_capacity(solver_ids.len());
                for (position, solver_id) in solver_ids.iter().enumerate() {
                    let solver = protocol
                        .solvers
                        .get(solver_id)
                        .ok_or_else(|| format!("missing solver {solver_id}"))?;
                    eprintln!(
                        "[{}/{}] {} seed={} budget={} solver={}",
                        records_written + group_runs.len() + 1,
                        expected_records,
                        experiment.id,
                        seed,
                        budget_scale,
                        solver_id
                    );
                    group_runs.push(run_one(
                        &protocol,
                        experiment,
                        scale,
                        seed,
                        budget_scale,
                        &fingerprint,
                        solver_id,
                        solver,
                        position,
                        &problem,
                    ));
                }

                add_comparisons(&mut group_runs, &problem);
                for run in group_runs {
                    serde_json::to_writer(&mut output, &run.record)?;
                    output.write_all(b"\n")?;
                    records_written += 1;
                }
                output.flush()?;
            }
        }
    }

    let end_unix = unix_seconds();
    let metadata = serde_json::json!({
        "schema_version": 1,
        "protocol_id": protocol.protocol_id,
        "protocol_blake3": blake3::hash(&protocol_bytes).to_hex().to_string(),
        "protocol_complete": !args.smoke && records_written == expected_records,
        "mode": if args.smoke { "smoke" } else { "full" },
        "source_revision": args.source_revision,
        "started_unix_seconds": start_unix,
        "finished_unix_seconds": end_unix,
        "elapsed_seconds": end_unix.saturating_sub(start_unix),
        "expected_records": expected_records,
        "records_written": records_written,
        "rustc": command_output("rustc", &["-Vv"]),
        "cargo": command_output("cargo", &["-V"]),
        "os": command_output("uname", &["-a"]),
        "cpu": command_output("sh", &["-c", "lscpu | sed -n '1,24p'"]),
        "memory": command_output("sh", &["-c", "sed -n '1,5p' /proc/meminfo"]),
    });
    fs::write(
        args.output_dir.join("metadata.json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;

    if records_written != expected_records {
        return Err(format!("wrote {records_written} records, expected {expected_records}").into());
    }
    eprintln!("wrote {}", results_path.display());
    Ok(())
}

fn validate_protocol(protocol: &Protocol) -> Result<(), Box<dyn std::error::Error>> {
    if !matches!(protocol.schema_version, 1 | 2) {
        return Err(format!("unsupported protocol schema {}", protocol.schema_version).into());
    }
    if protocol.experiments.is_empty() {
        return Err("protocol has no experiments".into());
    }
    for (solver_id, solver) in &protocol.solvers {
        if solver.fallback_to_lp {
            return Err(format!("solver {solver_id} enables a forbidden LP fallback").into());
        }
    }
    for experiment in &protocol.experiments {
        if experiment.seed_count == 0 || experiment.budget_scales.is_empty() {
            return Err(format!("experiment {} has an empty dimension", experiment.id).into());
        }
        if !protocol.profiles.contains_key(&experiment.profile) {
            return Err(format!("experiment {} has unknown profile", experiment.id).into());
        }
        if !protocol.scales.contains_key(&experiment.scale) {
            return Err(format!("experiment {} has unknown scale", experiment.id).into());
        }
        for solver in &experiment.solvers {
            if !protocol.solvers.contains_key(solver) {
                return Err(
                    format!("experiment {} has unknown solver {solver}", experiment.id).into(),
                );
            }
        }
        if !matches!(
            experiment.budget_basis.as_deref(),
            None | Some("generated") | Some("lp_limit_value")
        ) {
            return Err(format!("experiment {} has unknown budget basis", experiment.id).into());
        }
    }
    Ok(())
}

fn selected_experiments(protocol: &Protocol, smoke: bool) -> Vec<ExperimentSpec> {
    protocol
        .experiments
        .iter()
        .cloned()
        .map(|mut experiment| {
            if smoke {
                experiment.seed_count = 1;
                experiment.budget_scales.truncate(1);
                if protocol.schema_version >= 2 {
                    // V2 keeps preregistered evaluation seeds untouched. Smoke
                    // mode maps 50000+ evaluation ranges onto the disjoint
                    // 20000+ development ranges used while tuning plumbing.
                    experiment.seed_start = experiment.seed_start.saturating_sub(30_000);
                }
            }
            experiment
        })
        .collect()
}

fn prepare_output_dir(path: &Path, overwrite: bool) -> std::io::Result<()> {
    if path.exists() {
        if !overwrite {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("{} already exists; pass --overwrite", path.display()),
            ));
        }
        fs::remove_dir_all(path)?;
    }
    fs::create_dir_all(path)
}

fn build_scenario(seed: u64, scale: &ScaleSpec, profile: &ProfileSpec) -> ScenarioConfig {
    ScenarioConfig {
        seed,
        num_markets: scale.num_markets,
        market_group_probability: profile.market_group_probability,
        num_orders: scale.num_orders,
        order_size_min: scale.order_size_min,
        order_size_max: scale.order_size_max,
        order_size_power: profile.order_size_power,
        retail_buy_probability: profile.retail_buy_probability,
        liquidity_scarcity: profile.liquidity_scarcity,
        hot_market_fraction: profile.hot_market_fraction,
        hot_order_probability: profile.hot_order_probability,
        liquidity_depth_levels: profile.liquidity_depth_levels,
        liquidity_dispersion: profile.liquidity_dispersion,
        num_mms: scale.num_mms,
        mm_budget_min: scale.mm_budget_min_dollars,
        mm_budget_max: scale.mm_budget_max_dollars,
        mm_spread_bps: profile.mm_spread_bps,
        mm_capacity_multiplier: profile.mm_capacity_multiplier,
        mm_market_coverage_fraction: profile.mm_market_coverage_fraction,
        mm_market_coverage_max: profile.mm_market_coverage_max,
    }
}

fn generate_problem(seed: u64, scale: &ScaleSpec, profile: &ProfileSpec) -> Problem {
    match profile.generator.as_deref().unwrap_or("random") {
        "random" => generate_scenario(build_scenario(seed, scale, profile)),
        "flash_ladder" => generate_flash_liquidity_scenario(FlashLiquidityConfig {
            seed,
            num_markets: scale.num_markets,
            opportunities_per_market: (scale.num_orders / (2 * scale.num_markets)).max(1),
            num_mms: scale.num_mms.max(1),
            quantity_min_shares: scale.order_size_min,
            quantity_max_shares: scale.order_size_max,
            initial_budget_dollars: scale.mm_budget_max_dollars.max(1),
        }),
        other => panic!("unknown scenario generator {other}"),
    }
}

fn apply_mm_budgets(problem: &mut Problem, scale: f64, calibrated: Option<&[u64]>) {
    for (index, mm) in problem.mm_constraints.iter_mut().enumerate() {
        let base = calibrated
            .map(|values| values[index])
            .unwrap_or(mm.max_capital.0);
        mm.max_capital = Nanos((base as f64 * scale).round() as u64);
    }
}

fn unconstrained_lp_limit_values(problem: &Problem) -> Vec<u64> {
    let mut unconstrained = problem.clone();
    unconstrained.mm_constraints.clear();
    let result = LpSolver::new().solve(&unconstrained);
    let fill_quantities = aggregate_allocation(&result.result.fills);
    let order_map: HashMap<_, _> = problem
        .orders
        .iter()
        .map(|order| (order.id, order))
        .collect();
    problem
        .mm_constraints
        .iter()
        .map(|mm| {
            mm.order_ids
                .iter()
                .filter_map(|order_id| {
                    let order = order_map.get(order_id)?;
                    let quantity = Qty(fill_quantities.get(order_id).copied().unwrap_or(0));
                    Some(notional_nanos(mm_reduced_value(order), quantity).0)
                })
                .fold(0u64, u64::saturating_add)
        })
        .collect()
}

fn run_warmups(
    protocol: &Protocol,
    experiments: &[ExperimentSpec],
) -> Result<(), Box<dyn std::error::Error>> {
    let profile = protocol
        .profiles
        .get(&protocol.warmup.profile)
        .ok_or("warmup profile missing")?;
    let scale = protocol
        .scales
        .get(&protocol.warmup.scale)
        .ok_or("warmup scale missing")?;
    let mut problem = generate_problem(protocol.warmup.seed, scale, profile);
    apply_mm_budgets(&mut problem, protocol.warmup.budget_scale, None);

    let solver_ids: BTreeSet<_> = experiments
        .iter()
        .flat_map(|experiment| experiment.solvers.iter().cloned())
        .collect();
    for solver_id in solver_ids {
        let solver = protocol
            .solvers
            .get(&solver_id)
            .ok_or("warmup solver missing")?;
        eprintln!("warmup solver={solver_id}");
        let _ = catch_unwind(AssertUnwindSafe(|| execute_solver(solver, &problem)));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_one(
    protocol: &Protocol,
    experiment: &ExperimentSpec,
    scale: &ScaleSpec,
    seed: u64,
    budget_scale: f64,
    fingerprint: &str,
    solver_id: &str,
    solver: &SolverSpec,
    position: usize,
    problem: &Problem,
) -> InternalRun {
    let problem_metrics = problem_metrics(problem, scale.num_orders);
    let started = Instant::now();
    let attempted = catch_unwind(AssertUnwindSafe(|| execute_solver(solver, problem)));
    let wall_time = started.elapsed().as_secs_f64();

    match attempted {
        Ok(output) => record_solve(
            protocol,
            experiment,
            seed,
            budget_scale,
            fingerprint,
            solver_id,
            solver,
            position,
            problem,
            problem_metrics,
            output,
            wall_time,
        ),
        Err(payload) => InternalRun {
            record: RunRecord {
                schema_version: protocol.schema_version,
                protocol_id: protocol.protocol_id.clone(),
                experiment_id: experiment.id.clone(),
                suite: experiment.suite.clone(),
                profile: experiment.profile.clone(),
                scale: experiment.scale.clone(),
                seed,
                budget_scale,
                scenario_fingerprint_blake3: fingerprint.to_string(),
                solver_id: solver_id.to_string(),
                solver_label: solver.label.clone(),
                solver_config: solver.clone(),
                solver_position: position,
                run_status: "panic".to_string(),
                benchmark_success: false,
                termination: "panic".to_string(),
                iterations: None,
                convergence_metric: None,
                objective_value: None,
                optimality_gap: None,
                oracle_calls: None,
                integer_landing_loss: None,
                primal_residual: None,
                dual_residual: None,
                solver_message: None,
                panic_message: Some(panic_payload(payload)),
                milp_gap_percent: None,
                wall_time_seconds: wall_time,
                solver_reported_time_seconds: None,
                phase_times: None,
                verifier_valid: false,
                violation_count: 0,
                violation_kinds: Vec::new(),
                violation_details: Vec::new(),
                verifier_computed_welfare_nanos: None,
                gross_welfare_nanos: None,
                signed_minting_cost_nanos: None,
                net_welfare_nanos: None,
                retained_cash_objective_nanos: None,
                retail_gross_welfare_nanos: None,
                mm_gross_welfare_nanos: None,
                fills: None,
                filled_quantity: None,
                group_price_avg_delta_nanos: None,
                mm_utilization: Vec::new(),
                problem: problem_metrics,
                comparisons: ComparisonMetrics::default(),
            },
            allocation: BTreeMap::new(),
            prices: HashMap::new(),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn record_solve(
    protocol: &Protocol,
    experiment: &ExperimentSpec,
    seed: u64,
    budget_scale: f64,
    fingerprint: &str,
    solver_id: &str,
    solver: &SolverSpec,
    position: usize,
    problem: &Problem,
    problem_metrics: ProblemMetrics,
    output: SolveOutput,
    wall_time: f64,
) -> InternalRun {
    let verification = verify_match(&output.witness, false);
    let empty_result = output.result.fills.is_empty();
    let core_failure = matches!(
        output.termination.as_str(),
        "unsupported_input" | "numerical_failure" | "post_processing_failure" | "infeasible"
    );
    let timeout = output.termination == "time_limit";
    let run_status = if timeout {
        "timeout"
    } else if core_failure {
        "solver_failure"
    } else if empty_result {
        "empty_result"
    } else if !verification.valid {
        "verifier_invalid"
    } else {
        "completed"
    };
    let benchmark_success = run_status == "completed";

    let mm_order_ids: BTreeSet<u64> = problem
        .mm_constraints
        .iter()
        .flat_map(|mm| mm.order_ids.iter().copied())
        .collect();
    let order_map: HashMap<_, _> = problem
        .orders
        .iter()
        .map(|order| (order.id, order))
        .collect();
    let mut retail_gross = 0i64;
    let mut mm_gross = 0i64;
    for fill in &output.result.fills {
        if let Some(order) = order_map.get(&fill.order_id) {
            let contribution = order.gross_welfare_contribution(fill.fill_qty);
            if mm_order_ids.contains(&fill.order_id) {
                mm_gross += contribution;
            } else {
                retail_gross += contribution;
            }
        }
    }

    let allocation = aggregate_allocation(&output.result.fills);
    let violation_kinds = verification
        .violations
        .iter()
        .map(|violation| format!("{:?}", violation.kind))
        .collect();
    let violation_details = verification
        .violations
        .iter()
        .map(ToString::to_string)
        .collect();

    InternalRun {
        record: RunRecord {
            schema_version: protocol.schema_version,
            protocol_id: protocol.protocol_id.clone(),
            experiment_id: experiment.id.clone(),
            suite: experiment.suite.clone(),
            profile: experiment.profile.clone(),
            scale: experiment.scale.clone(),
            seed,
            budget_scale,
            scenario_fingerprint_blake3: fingerprint.to_string(),
            solver_id: solver_id.to_string(),
            solver_label: solver.label.clone(),
            solver_config: solver.clone(),
            solver_position: position,
            run_status: run_status.to_string(),
            benchmark_success,
            termination: output.termination,
            iterations: output.iterations,
            convergence_metric: output.convergence_metric,
            objective_value: output.objective_value,
            optimality_gap: output.optimality_gap,
            oracle_calls: output.oracle_calls,
            integer_landing_loss: output.integer_landing_loss,
            primal_residual: output.primal_residual,
            dual_residual: output.dual_residual,
            solver_message: output.message,
            panic_message: None,
            milp_gap_percent: output.milp_gap_percent,
            wall_time_seconds: wall_time,
            solver_reported_time_seconds: Some(output.reported_time),
            phase_times: Some(output.phases),
            verifier_valid: verification.valid,
            violation_count: verification.violations.len(),
            violation_kinds,
            violation_details,
            verifier_computed_welfare_nanos: Some(verification.stats.computed_welfare),
            gross_welfare_nanos: Some(output.result.gross_welfare),
            signed_minting_cost_nanos: Some(output.result.minting_cost),
            net_welfare_nanos: Some(output.result.total_welfare()),
            retained_cash_objective_nanos: Some(retained_cash_objective_for_fills(
                problem,
                &output.result.fills,
            )),
            retail_gross_welfare_nanos: Some(retail_gross),
            mm_gross_welfare_nanos: Some(mm_gross),
            fills: Some(output.result.fills.len()),
            filled_quantity: Some(output.result.total_quantity_filled),
            group_price_avg_delta_nanos: verification.stats.market_group_avg_delta,
            mm_utilization: mm_utilization(problem, &output.result.fills),
            problem: problem_metrics,
            comparisons: ComparisonMetrics::default(),
        },
        allocation,
        prices: output.prices,
    }
}

fn execute_solver(solver: &SolverSpec, problem: &Problem) -> SolveOutput {
    match solver.kind.as_str() {
        "lp-unconstrained" => {
            let mut unconstrained = problem.clone();
            unconstrained.mm_constraints.clear();
            pipeline_output(LpSolver::new().solve(&unconstrained), problem)
        }
        "lp" => pipeline_output(
            LpSolver::with_config(LpConfig {
                max_mm_iterations: required_usize(solver, "max_iterations"),
            })
            .solve(problem),
            problem,
        ),
        "retained-cash-fw" => pipeline_output(
            RetainedCashSolver::with_config(RetainedCashConfig {
                max_iterations: required_usize(solver, "max_iterations"),
                gap_rel: required_f64(solver, "tolerance"),
                gap_abs_nanos: required_f64(solver, "absolute_tolerance"),
                line_search_steps: required_usize(solver, "line_search_steps"),
            })
            .solve(problem),
            problem,
        ),
        "iter-lp" => pipeline_output(
            IterLpSolver::with_config(IterLpConfig {
                max_iterations: required_usize(solver, "max_iterations"),
                mu_tol: required_f64(solver, "tolerance"),
                damping: required_f64(solver, "damping"),
            })
            .solve(problem),
            problem,
        ),
        "eg" => pipeline_output(
            EgSolver::with_config(EgConfig {
                max_fw_iterations: required_usize(solver, "max_iterations"),
                convergence_tol: required_f64(solver, "tolerance"),
                q_stability_tol: required_f64(solver, "q_tolerance"),
                line_search_steps: required_usize(solver, "line_search_steps"),
                max_mm_slp_iterations: 1,
            })
            .solve(problem),
            problem,
        ),
        "conic-quasi" => pipeline_output(
            conic_solver(solver, ObjectiveMode::QuasiFisher).solve(problem),
            problem,
        ),
        "conic-fisher" => pipeline_output(
            conic_solver(solver, ObjectiveMode::Fisher).solve(problem),
            problem,
        ),
        "decomposed-lp" => pipeline_output(
            DecomposedSolver::with_config(
                LpSolver::new(),
                required_usize(solver, "max_iterations"),
                required_f64(solver, "tolerance"),
            )
            .solve(problem),
            problem,
        ),
        "decomposed-quasi" => {
            let inner = ConicSolver::with_config(ConicConfig {
                mode: ObjectiveMode::QuasiFisher,
                max_iter: required_usize(solver, "inner_max_iterations") as u32,
                tol: required_f64(solver, "inner_tolerance"),
                time_limit: required_f64(solver, "time_limit_seconds"),
                ..Default::default()
            });
            pipeline_output(
                DecomposedSolver::with_config(
                    inner,
                    required_usize(solver, "max_iterations"),
                    required_f64(solver, "tolerance"),
                )
                .solve(problem),
                problem,
            )
        }
        "milp-exact" => {
            let milp = MilpSolver::with_config(MilpConfig {
                timeout_secs: Some(required_f64(solver, "time_limit_seconds")),
                gap_tolerance: required_f64(solver, "gap_tolerance"),
                mm_budget_mode: MmBudgetMode::Exact,
            });
            let result = milp.solve_with_status(problem);
            let termination = match &result.status {
                SolveStatus::Optimal => "converged",
                SolveStatus::TimeLimitReached { .. } => "time_limit",
                SolveStatus::Infeasible => "infeasible",
                SolveStatus::Error(_) => "numerical_failure",
            }
            .to_string();
            let gap = result.status.gap();
            let message = Some(format!("{:?}", result.status));
            let witness = witness_from_milp(problem, &result);
            SolveOutput {
                result: result.result,
                prices: result.clearing_prices,
                reported_time: result.solve_time_secs,
                phases: PhaseMetrics::default(),
                termination,
                iterations: None,
                convergence_metric: None,
                objective_value: Some(result.objective_welfare as f64),
                optimality_gap: None,
                oracle_calls: None,
                integer_landing_loss: None,
                primal_residual: None,
                dual_residual: None,
                message,
                milp_gap_percent: gap,
                witness,
            }
        }
        unknown => panic!("unknown solver kind {unknown}"),
    }
}

fn conic_solver(spec: &SolverSpec, mode: ObjectiveMode) -> ConicSolver {
    ConicSolver::with_config(ConicConfig {
        mode,
        max_iter: required_usize(spec, "max_iterations") as u32,
        tol: required_f64(spec, "tolerance"),
        time_limit: required_f64(spec, "time_limit_seconds"),
        ..Default::default()
    })
}

fn pipeline_output(pipeline: PipelineResult, problem: &Problem) -> SolveOutput {
    let witness = witness_from_pipeline(problem, &pipeline);
    SolveOutput {
        result: pipeline.result,
        prices: pipeline
            .price_discovery
            .map(|discovery| discovery.prices)
            .unwrap_or_default(),
        reported_time: pipeline.total_time_secs,
        phases: PhaseMetrics {
            price_discovery_seconds: pipeline.phase_times.price_discovery_secs,
            allocation_seconds: pipeline.phase_times.allocation_secs,
            partial_solving_seconds: pipeline.phase_times.partial_solving_secs,
            combining_seconds: pipeline.phase_times.combining_secs,
        },
        termination: termination_name(&pipeline.diagnostics.status).to_string(),
        iterations: pipeline.diagnostics.iterations,
        convergence_metric: pipeline.diagnostics.convergence_metric,
        objective_value: pipeline.diagnostics.objective_value,
        optimality_gap: pipeline.diagnostics.optimality_gap,
        oracle_calls: pipeline.diagnostics.oracle_calls,
        integer_landing_loss: pipeline.diagnostics.integer_landing_loss,
        primal_residual: pipeline.diagnostics.primal_residual,
        dual_residual: pipeline.diagnostics.dual_residual,
        message: pipeline.diagnostics.message,
        milp_gap_percent: None,
        witness,
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

fn add_comparisons(runs: &mut [InternalRun], problem: &Problem) {
    let lp_index = runs
        .iter()
        .position(|run| run.record.solver_id == "lp" && run.record.benchmark_success);
    let best_welfare = runs
        .iter()
        .filter(|run| run.record.benchmark_success)
        .filter_map(|run| run.record.net_welfare_nanos)
        .max();
    let unconstrained_index = runs
        .iter()
        .position(|run| run.record.solver_id == "lp-unconstrained");
    let best_retained_objective = runs
        .iter()
        .filter(|run| run.record.benchmark_success)
        .filter_map(|run| run.record.retained_cash_objective_nanos)
        .reduce(f64::max);
    let milp_index = runs.iter().position(|run| {
        run.record.solver_id == "milp-exact"
            && run.record.benchmark_success
            && run.record.termination == "converged"
    });

    let lp_snapshot = lp_index.map(|index| {
        (
            runs[index].record.net_welfare_nanos.unwrap_or(0),
            runs[index].allocation.clone(),
            runs[index].prices.clone(),
        )
    });
    let unconstrained_snapshot = unconstrained_index.map(|index| {
        let fills: Vec<_> = runs[index]
            .allocation
            .iter()
            .map(|(&order_id, &quantity)| Fill::new(order_id, Qty(quantity), Nanos::ZERO))
            .collect();
        (
            runs[index].record.net_welfare_nanos.unwrap_or(0),
            retained_cash_welfare_gap_bound_for_fills(problem, &fills),
        )
    });
    let milp_welfare = milp_index.and_then(|index| runs[index].record.net_welfare_nanos);

    for run in runs {
        if let Some(best) = best_welfare
            && let Some(welfare) = run.record.net_welfare_nanos
        {
            run.record.comparisons.observed_best_welfare_gap_bps = relative_gap_bps(best, welfare);
        }
        if let Some((lp_welfare, lp_allocation, lp_prices)) = &lp_snapshot {
            if let Some(welfare) = run.record.net_welfare_nanos {
                run.record.comparisons.lp_welfare_gap_bps = relative_gap_bps(*lp_welfare, welfare);
            }
            run.record.comparisons.lp_allocation_l1_ratio =
                allocation_l1_ratio(lp_allocation, &run.allocation);
            run.record.comparisons.lp_price_mae_nanos = price_mae(lp_prices, &run.prices);
        }
        if let Some((unconstrained_welfare, bound)) = unconstrained_snapshot
            && let Some(welfare) = run.record.net_welfare_nanos
        {
            run.record.comparisons.unconstrained_lp_welfare_gap_bps =
                relative_gap_bps(unconstrained_welfare, welfare);
            run.record.comparisons.paper_welfare_bound_nanos = Some(bound);
            if bound > 0.0 {
                run.record.comparisons.paper_bound_ratio =
                    Some((unconstrained_welfare - welfare) as f64 / bound);
            }
        }
        if let Some(reference) = best_retained_objective
            && let Some(observed) = run.record.retained_cash_objective_nanos
        {
            run.record
                .comparisons
                .observed_best_retained_cash_objective_gap_bps =
                relative_gap_bps_f64(reference, observed);
        }
        if let Some(reference) = milp_welfare
            && let Some(observed) = run.record.net_welfare_nanos
        {
            run.record.comparisons.milp_welfare_gap_bps = relative_gap_bps(reference, observed);
        }
    }
}

fn relative_gap_bps(reference: i64, observed: i64) -> Option<f64> {
    (reference != 0)
        .then(|| (reference - observed) as f64 / (reference.unsigned_abs() as f64) * 10_000.0)
}

fn relative_gap_bps_f64(reference: f64, observed: f64) -> Option<f64> {
    (reference != 0.0).then(|| (reference - observed) / reference.abs() * 10_000.0)
}

fn allocation_l1_ratio(a: &BTreeMap<u64, u64>, b: &BTreeMap<u64, u64>) -> Option<f64> {
    let keys: BTreeSet<_> = a.keys().chain(b.keys()).copied().collect();
    let numerator: u128 = keys
        .iter()
        .map(|key| {
            a.get(key)
                .copied()
                .unwrap_or(0)
                .abs_diff(b.get(key).copied().unwrap_or(0)) as u128
        })
        .sum();
    let denominator: u128 = keys
        .iter()
        .map(|key| {
            a.get(key)
                .copied()
                .unwrap_or(0)
                .max(b.get(key).copied().unwrap_or(0)) as u128
        })
        .sum();
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

fn price_mae(a: &HashMap<MarketId, Vec<Nanos>>, b: &HashMap<MarketId, Vec<Nanos>>) -> Option<f64> {
    let mut total = 0u128;
    let mut count = 0u128;
    for (market, a_prices) in a {
        if let Some(b_prices) = b.get(market) {
            for (left, right) in a_prices.iter().zip(b_prices) {
                total += left.0.abs_diff(right.0) as u128;
                count += 1;
            }
        }
    }
    (count > 0).then(|| total as f64 / count as f64)
}

fn problem_metrics(problem: &Problem, declared_retail_orders: usize) -> ProblemMetrics {
    let mm_order_ids: BTreeSet<u64> = problem
        .mm_constraints
        .iter()
        .flat_map(|mm| mm.order_ids.iter().copied())
        .collect();
    ProblemMetrics {
        markets: problem.markets.len(),
        orders: problem.orders.len(),
        declared_retail_orders,
        buy_orders: problem
            .orders
            .iter()
            .filter(|order| !order.is_seller())
            .count(),
        sell_orders: problem
            .orders
            .iter()
            .filter(|order| order.is_seller())
            .count(),
        mm_orders: mm_order_ids.len(),
        market_groups: problem.market_groups.len(),
        grouped_markets: problem
            .market_groups
            .iter()
            .map(|group| group.markets.len())
            .sum(),
        total_max_fill: problem.orders.iter().map(|order| order.max_fill.0).sum(),
        total_mm_budget_nanos: problem
            .mm_constraints
            .iter()
            .map(|mm| mm.max_capital.0)
            .sum(),
    }
}

fn mm_utilization(problem: &Problem, fills: &[Fill]) -> Vec<MmUtilization> {
    let mut fill_map: HashMap<u64, (Nanos, Qty)> = HashMap::new();
    for fill in fills {
        fill_map
            .entry(fill.order_id)
            .and_modify(|(_, qty)| qty.0 = qty.0.saturating_add(fill.fill_qty.0))
            .or_insert((fill.fill_price, fill.fill_qty));
    }
    let order_map: HashMap<_, _> = problem
        .orders
        .iter()
        .map(|order| (order.id, order))
        .collect();
    problem
        .mm_constraints
        .iter()
        .map(|mm| {
            let used = mm.checked_capital_used(&fill_map).map(|capital| capital.0);
            let limit_value_nanos = mm
                .order_ids
                .iter()
                .filter_map(|order_id| {
                    let order = order_map.get(order_id)?;
                    let quantity = fill_map
                        .get(order_id)
                        .map(|(_, quantity)| *quantity)
                        .unwrap_or(Qty::ZERO);
                    Some(notional_nanos(mm_reduced_value(order), quantity).0)
                })
                .fold(0u64, u64::saturating_add);
            MmUtilization {
                mm_id: mm.mm_id.0,
                budget_nanos: mm.max_capital.0,
                capital_used_nanos: used,
                utilization: used.and_then(|used| {
                    (mm.max_capital.0 > 0).then(|| used as f64 / mm.max_capital.0 as f64)
                }),
                limit_value_nanos,
                limit_value_to_budget: (mm.max_capital.0 > 0)
                    .then(|| limit_value_nanos as f64 / mm.max_capital.0 as f64),
            }
        })
        .collect()
}

fn mm_reduced_value(order: &matching_engine::Order) -> Nanos {
    if order.is_seller() {
        Nanos(NANOS_PER_DOLLAR.saturating_sub(order.limit_price.0))
    } else {
        order.limit_price
    }
}

fn aggregate_allocation(fills: &[Fill]) -> BTreeMap<u64, u64> {
    let mut allocation = BTreeMap::new();
    for fill in fills {
        *allocation.entry(fill.order_id).or_default() += fill.fill_qty.0;
    }
    allocation
}

fn problem_fingerprint(problem: &Problem) -> Result<String, serde_json::Error> {
    let bytes = serde_json::to_vec(&(
        &problem.markets,
        &problem.orders,
        &problem.mm_constraints,
        &problem.market_groups,
    ))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn required_usize(spec: &SolverSpec, field: &str) -> usize {
    match field {
        "max_iterations" => spec.max_iterations,
        "inner_max_iterations" => spec.inner_max_iterations,
        "line_search_steps" => spec.line_search_steps,
        _ => None,
    }
    .unwrap_or_else(|| panic!("{} missing {field}", spec.kind))
}

fn required_f64(spec: &SolverSpec, field: &str) -> f64 {
    match field {
        "tolerance" => spec.tolerance,
        "inner_tolerance" => spec.inner_tolerance,
        "q_tolerance" => spec.q_tolerance,
        "damping" => spec.damping,
        "time_limit_seconds" => spec.time_limit_seconds,
        "gap_tolerance" => spec.gap_tolerance,
        "absolute_tolerance" => spec.absolute_tolerance,
        _ => None,
    }
    .unwrap_or_else(|| panic!("{} missing {field}", spec.kind))
}

fn panic_payload(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

fn command_output(command: &str, args: &[&str]) -> String {
    Command::new(command)
        .args(args)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|error| format!("unavailable: {error}"))
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_distance_is_symmetric_and_normalized() {
        let a = BTreeMap::from([(1, 10), (2, 5)]);
        let b = BTreeMap::from([(1, 5), (3, 5)]);
        assert_eq!(allocation_l1_ratio(&a, &b), allocation_l1_ratio(&b, &a));
        assert_eq!(allocation_l1_ratio(&a, &a), Some(0.0));
        assert_eq!(allocation_l1_ratio(&a, &b), Some(0.75));
    }

    #[test]
    fn relative_gap_preserves_better_than_reference_sign() {
        assert_eq!(relative_gap_bps(100, 90), Some(1000.0));
        assert_eq!(relative_gap_bps(100, 110), Some(-1000.0));
        assert_eq!(relative_gap_bps(0, 0), None);
    }

    #[test]
    fn checked_in_protocol_is_valid() {
        let bytes = include_bytes!("../../../../benchmarks/solver/protocol-v1.json");
        let protocol: Protocol = serde_json::from_slice(bytes).expect("parse protocol");
        validate_protocol(&protocol).expect("valid protocol");
    }

    #[test]
    fn checked_in_v2_protocol_is_valid() {
        let bytes = include_bytes!("../../../../benchmarks/solver/protocol-v2.json");
        let protocol: Protocol = serde_json::from_slice(bytes).expect("parse protocol");
        validate_protocol(&protocol).expect("valid protocol");
    }
}
