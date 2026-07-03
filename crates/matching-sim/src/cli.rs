//! Command-line argument parsing and solver-selection helpers.

use matching_scenarios::ScenarioConfig;
use matching_solver::{MilpConfig, MilpSolver, MmBudgetMode};

pub fn parse_scenario_config(args: &[String]) -> ScenarioConfig {
    if let Some(preset) = get_arg_value(args, "--preset") {
        let mut config = match preset.as_str() {
            "quick" => ScenarioConfig::quick(),
            "small" => ScenarioConfig::small(),
            "medium" => ScenarioConfig::medium(),
            "large" => ScenarioConfig::large(),
            "extreme" => ScenarioConfig::extreme(),
            "milp-killer" | "milp_killer" => ScenarioConfig::milp_killer(),
            _ => {
                eprintln!("Unknown preset: {}, using medium", preset);
                ScenarioConfig::medium()
            }
        };

        if let Some(seed) = get_arg_value(args, "--seed") {
            config.seed = seed.parse().unwrap_or(42);
        }
        if let Some(v) = get_arg_value(args, "--mms") {
            config.num_mms = v.parse().unwrap_or(config.num_mms);
        }
        if let Some(v) = get_arg_value(args, "--mm-capacity-mult") {
            config.mm_capacity_multiplier = v.parse().unwrap_or(10);
        }
        if let Some(v) = get_arg_value(args, "--markets") {
            config.num_markets = v.parse().unwrap_or(config.num_markets);
        }
        if let Some(v) = get_arg_value(args, "--orders") {
            config.num_orders = v.parse().unwrap_or(config.num_orders);
        }
        if let Some(v) = get_arg_value(args, "--scarcity") {
            config.liquidity_scarcity = v.parse().unwrap_or(config.liquidity_scarcity);
        }

        return config;
    }

    let mut config = ScenarioConfig::default();

    if let Some(v) = get_arg_value(args, "--seed") {
        config.seed = v.parse().unwrap_or(42);
    }
    if let Some(v) = get_arg_value(args, "--markets") {
        config.num_markets = v.parse().unwrap_or(30);
    }
    if let Some(v) = get_arg_value(args, "--orders") {
        config.num_orders = v.parse().unwrap_or(1000);
    }
    if let Some(v) = get_arg_value(args, "--scarcity") {
        config.liquidity_scarcity = v.parse().unwrap_or(0.5);
    }
    if let Some(v) = get_arg_value(args, "--mms") {
        config.num_mms = v.parse().unwrap_or(2);
    }
    if let Some(v) = get_arg_value(args, "--mm-capacity-mult") {
        config.mm_capacity_multiplier = v.parse().unwrap_or(10);
    }

    config
}

#[derive(Clone, Debug, PartialEq)]
pub enum SolverChoice {
    Milp,
    #[cfg(feature = "lp")]
    Lp,
    #[cfg(feature = "lp")]
    Eg,
    #[cfg(feature = "conic")]
    Conic,
    #[cfg(feature = "lp")]
    DecomposedLp,
    #[cfg(feature = "lp")]
    DecomposedEg,
    #[cfg(feature = "conic")]
    DecomposedConic,
    #[cfg(feature = "lp")]
    IterLp,
    #[cfg(feature = "lp")]
    DecomposedIterLp,
    All,
}

pub fn supports_detailed_pipeline(choice: &SolverChoice) -> bool {
    match choice {
        #[cfg(feature = "lp")]
        SolverChoice::Lp
        | SolverChoice::Eg
        | SolverChoice::DecomposedLp
        | SolverChoice::DecomposedEg
        | SolverChoice::IterLp
        | SolverChoice::DecomposedIterLp => true,
        #[cfg(feature = "conic")]
        SolverChoice::Conic | SolverChoice::DecomposedConic => true,
        SolverChoice::Milp | SolverChoice::All => false,
    }
}

pub fn parse_solver_choice(args: &[String]) -> SolverChoice {
    match get_arg_value(args, "--solver").as_deref() {
        Some("milp") => SolverChoice::Milp,
        #[cfg(feature = "lp")]
        Some("lp") => SolverChoice::Lp,
        #[cfg(feature = "lp")]
        Some("eg") => SolverChoice::Eg,
        #[cfg(feature = "conic")]
        Some("conic") => SolverChoice::Conic,
        #[cfg(feature = "lp")]
        #[cfg(feature = "lp")]
        Some("decomposed-lp") | Some("decomposed") => SolverChoice::DecomposedLp,
        #[cfg(feature = "lp")]
        Some("decomposed-eg") => SolverChoice::DecomposedEg,
        #[cfg(feature = "conic")]
        Some("decomposed-conic") => SolverChoice::DecomposedConic,
        #[cfg(feature = "lp")]
        Some("iter-lp") => SolverChoice::IterLp,
        #[cfg(feature = "lp")]
        Some("decomposed-iter-lp") => SolverChoice::DecomposedIterLp,
        Some("all") => SolverChoice::All,
        #[cfg(feature = "lp")]
        _ => SolverChoice::Lp, // Default to LP solver when available.
        #[cfg(not(feature = "lp"))]
        _ => SolverChoice::Milp, // No-feature builds only have the always-enabled MILP backend.
    }
}

pub fn parse_milp_timeout(args: &[String]) -> Option<f64> {
    get_arg_value(args, "--milp-timeout").and_then(|v| v.parse().ok())
}

pub fn parse_mm_mode(args: &[String]) -> MmBudgetMode {
    match get_arg_value(args, "--mm-mode").as_deref() {
        Some("exact") => MmBudgetMode::Exact,
        Some("mccormick") => MmBudgetMode::McCormick,
        Some("ignore") => MmBudgetMode::Ignore,
        _ => MmBudgetMode::Exact, // Default: exact bilinear via SCIP MIQCQP
    }
}

#[cfg(feature = "conic")]
fn parse_objective_mode(args: &[String]) -> matching_solver::ObjectiveMode {
    match get_arg_value(args, "--mode").as_deref() {
        Some("linear") => matching_solver::ObjectiveMode::Linear,
        Some("fisher") => matching_solver::ObjectiveMode::Fisher,
        Some("quasi-fisher") => matching_solver::ObjectiveMode::QuasiFisher,
        _ => matching_solver::ObjectiveMode::QuasiFisher,
    }
}

#[cfg(feature = "conic")]
fn parse_temperature(args: &[String]) -> f64 {
    get_arg_value(args, "--temperature")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0)
}

#[cfg(feature = "conic")]
pub fn build_conic_config(args: &[String]) -> matching_solver::ConicConfig {
    matching_solver::ConicConfig {
        mode: parse_objective_mode(args),
        temperature: parse_temperature(args),
        ..Default::default()
    }
}

pub fn parse_batches(args: &[String]) -> usize {
    get_arg_value(args, "--batches")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1) // Default to 1 batch
}

pub fn get_arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

pub fn create_milp_solver(milp_timeout: Option<f64>, mm_mode: MmBudgetMode) -> MilpSolver {
    let timeout = milp_timeout.unwrap_or(5.0);
    MilpSolver::with_config(MilpConfig {
        timeout_secs: Some(timeout),
        gap_tolerance: 0.0,
        mm_budget_mode: mm_mode,
    })
}

/// Expand a solver choice into individual choices for comparison.
// The `All` arm builds its vec with cfg-gated pushes per solver feature,
// which vec![] cannot express.
#[allow(clippy::vec_init_then_push)]
pub fn expand_solver_choices(choice: &SolverChoice) -> Vec<SolverChoice> {
    match choice {
        SolverChoice::All => {
            let mut choices = Vec::new();
            choices.push(SolverChoice::Milp);
            #[cfg(feature = "lp")]
            choices.push(SolverChoice::Lp);
            #[cfg(feature = "lp")]
            choices.push(SolverChoice::Eg);
            #[cfg(feature = "conic")]
            choices.push(SolverChoice::Conic);
            #[cfg(feature = "lp")]
            #[cfg(feature = "lp")]
            choices.push(SolverChoice::DecomposedLp);
            #[cfg(feature = "lp")]
            choices.push(SolverChoice::DecomposedEg);
            #[cfg(feature = "conic")]
            choices.push(SolverChoice::DecomposedConic);
            #[cfg(feature = "lp")]
            choices.push(SolverChoice::IterLp);
            #[cfg(feature = "lp")]
            choices.push(SolverChoice::DecomposedIterLp);
            choices
        }
        other => vec![other.clone()],
    }
}

/// Get the display name for a solver choice.
pub fn solver_display_name(choice: &SolverChoice, milp_timeout: Option<f64>) -> String {
    match choice {
        SolverChoice::Milp => {
            if milp_timeout.is_some() {
                "MILP (time-limited)".to_string()
            } else {
                "MILP".to_string()
            }
        }
        #[cfg(feature = "lp")]
        SolverChoice::Lp => "LP".to_string(),
        #[cfg(feature = "lp")]
        SolverChoice::Eg => "EG (Fisher)".to_string(),
        #[cfg(feature = "conic")]
        SolverChoice::Conic => "Conic (EG)".to_string(),
        #[cfg(feature = "lp")]
        #[cfg(feature = "lp")]
        SolverChoice::DecomposedLp => "Decomposed(LP)".to_string(),
        #[cfg(feature = "lp")]
        SolverChoice::DecomposedEg => "Decomposed(EG)".to_string(),
        #[cfg(feature = "conic")]
        SolverChoice::DecomposedConic => "Decomposed(Conic)".to_string(),
        #[cfg(feature = "lp")]
        SolverChoice::IterLp => "IterLP".to_string(),
        #[cfg(feature = "lp")]
        SolverChoice::DecomposedIterLp => "Decomposed(IterLP)".to_string(),
        SolverChoice::All => "All".to_string(),
    }
}
