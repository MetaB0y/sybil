//! Typed command-line arguments and solver-selection helpers.

use std::fmt;

use clap::{Parser, ValueEnum};
use matching_scenarios::ScenarioConfig;
use matching_solver::{MilpConfig, MilpSolver, MmBudgetMode};

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Preset {
    Quick,
    Small,
    Medium,
    Large,
    Extreme,
    #[value(alias = "milp_killer")]
    MilpKiller,
}

impl Preset {
    fn config(self) -> ScenarioConfig {
        match self {
            Self::Quick => ScenarioConfig::quick(),
            Self::Small => ScenarioConfig::small(),
            Self::Medium => ScenarioConfig::medium(),
            Self::Large => ScenarioConfig::large(),
            Self::Extreme => ScenarioConfig::extreme(),
            Self::MilpKiller => ScenarioConfig::milp_killer(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum MmMode {
    #[default]
    Exact,
    McCormick,
    Ignore,
}

impl From<MmMode> for MmBudgetMode {
    fn from(value: MmMode) -> Self {
        match value {
            MmMode::Exact => Self::Exact,
            MmMode::McCormick => Self::McCormick,
            MmMode::Ignore => Self::Ignore,
        }
    }
}

#[cfg(feature = "conic")]
#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum ObjectiveMode {
    Linear,
    Fisher,
    #[default]
    QuasiFisher,
}

#[cfg(feature = "conic")]
impl From<ObjectiveMode> for matching_solver::ObjectiveMode {
    fn from(value: ObjectiveMode) -> Self {
        match value {
            ObjectiveMode::Linear => Self::Linear,
            ObjectiveMode::Fisher => Self::Fisher,
            ObjectiveMode::QuasiFisher => Self::QuasiFisher,
        }
    }
}

/// Generate synthetic matching scenarios and compare enabled solvers.
#[derive(Debug, Parser)]
#[command(version)]
pub struct Cli {
    /// Use a preset scenario configuration.
    #[arg(long, value_enum)]
    preset: Option<Preset>,

    /// Override the random seed.
    #[arg(long)]
    seed: Option<u64>,

    /// Override the number of binary markets.
    #[arg(long)]
    markets: Option<usize>,

    /// Override the number of generated orders.
    #[arg(long)]
    orders: Option<usize>,

    /// Override liquidity scarcity (0.0-1.0, lower is scarcer).
    #[arg(long)]
    scarcity: Option<f64>,

    /// Override the number of market makers.
    #[arg(long)]
    mms: Option<usize>,

    /// Override the synthetic MM capacity multiplier.
    #[arg(long)]
    mm_capacity_mult: Option<u64>,

    /// Solver to run, or `all` to compare every enabled solver.
    #[arg(long, value_enum, default_value_t)]
    pub solver: SolverChoice,

    /// MILP time limit in seconds.
    #[arg(long)]
    pub milp_timeout: Option<f64>,

    /// MM budget constraint mode used by the MILP solver.
    #[arg(long, value_enum, default_value_t)]
    mm_mode: MmMode,

    /// Conic objective mode.
    #[cfg(feature = "conic")]
    #[arg(long, value_enum, default_value_t)]
    objective_mode: ObjectiveMode,

    /// LMSR smoothing temperature (>0 is not yet implemented).
    #[cfg(feature = "conic")]
    #[arg(long, default_value_t = 0.0)]
    temperature: f64,

    /// Number of batches to run.
    #[arg(long, default_value_t = 1)]
    pub batches: usize,

    /// Show detailed step-by-step output.
    #[arg(short, long)]
    pub verbose: bool,

    /// Export a detailed pipeline snapshot as JSON.
    #[arg(long, value_name = "PATH")]
    pub export_json: Option<String>,

    /// Export solver comparison results as JSON.
    #[arg(long, value_name = "PATH")]
    pub export_comparison: Option<String>,

    /// Show ASCII convergence charts after the run.
    #[arg(long)]
    pub show_charts: bool,

    /// Scale every generated MM budget before solving.
    #[arg(long)]
    pub mm_budget_scale: Option<f64>,
}

impl Cli {
    pub fn scenario_config(&self) -> ScenarioConfig {
        let mut config = self
            .preset
            .map_or_else(ScenarioConfig::default, Preset::config);

        if let Some(seed) = self.seed {
            config.seed = seed;
        }
        if let Some(markets) = self.markets {
            config.num_markets = markets;
        }
        if let Some(orders) = self.orders {
            config.num_orders = orders;
        }
        if let Some(scarcity) = self.scarcity {
            config.liquidity_scarcity = scarcity;
        }
        if let Some(mms) = self.mms {
            config.num_mms = mms;
        }
        if let Some(multiplier) = self.mm_capacity_mult {
            config.mm_capacity_multiplier = multiplier;
        }

        config
    }

    pub fn mm_budget_mode(&self) -> MmBudgetMode {
        self.mm_mode.into()
    }

    #[cfg(feature = "conic")]
    pub fn conic_config(&self) -> matching_solver::ConicConfig {
        matching_solver::ConicConfig {
            mode: self.objective_mode.into(),
            temperature: self.temperature,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, ValueEnum)]
pub enum SolverChoice {
    Milp,
    #[cfg(feature = "lp")]
    Lp,
    #[cfg(feature = "lp")]
    Eg,
    #[cfg(feature = "conic")]
    Conic,
    #[cfg(feature = "lp")]
    #[value(alias = "decomposed")]
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

// The default follows the enabled solver features. `derive(Default)` cannot
// express the `Milp` fallback when this crate is built without `lp`.
#[allow(clippy::derivable_impls)]
impl Default for SolverChoice {
    fn default() -> Self {
        #[cfg(feature = "lp")]
        {
            Self::Lp
        }
        #[cfg(not(feature = "lp"))]
        {
            Self::Milp
        }
    }
}

impl fmt::Display for SolverChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self
            .to_possible_value()
            .expect("every solver choice has a clap value");
        formatter.write_str(value.get_name())
    }
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

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn preset_values_can_be_overridden() {
        let cli = Cli::try_parse_from([
            "matching-sim",
            "--preset",
            "quick",
            "--orders",
            "17",
            "--seed",
            "9",
        ])
        .unwrap();

        let config = cli.scenario_config();
        assert_eq!(config.num_orders, 17);
        assert_eq!(config.seed, 9);
        assert_eq!(config.num_markets, ScenarioConfig::quick().num_markets);
    }

    #[test]
    fn legacy_solver_and_preset_aliases_are_retained() {
        let cli = Cli::try_parse_from([
            "matching-sim",
            "--preset",
            "milp_killer",
            "--solver",
            "decomposed",
        ])
        .unwrap();

        assert_eq!(
            cli.scenario_config().num_orders,
            ScenarioConfig::milp_killer().num_orders
        );
        #[cfg(feature = "lp")]
        assert_eq!(cli.solver, SolverChoice::DecomposedLp);
    }

    #[test]
    fn invalid_numbers_fail_instead_of_silently_using_defaults() {
        let error = Cli::try_parse_from(["matching-sim", "--orders", "many"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }
}
