//! Optimality metrics for the matching system.

use std::fmt;

/// Metrics for a single matching instance.
#[derive(Clone, Debug)]
pub struct OptimalityMetrics {
    pub achieved_welfare: i64,
    pub lp_upper_bound: Option<i64>,
    pub optimality_gap: Option<f64>,
    pub orders_filled: usize,
    pub unfilled_due_to_liquidity: usize,
    pub unfilled_due_to_aon: usize,
    pub total_orders: usize,
}

impl OptimalityMetrics {
    pub fn from_results(
        achieved_welfare: i64,
        lp_bound: i64,
        orders_filled: usize,
        unfilled_liquidity: usize,
        unfilled_aon: usize,
        total_orders: usize,
    ) -> Self {
        let gap = if lp_bound > 0 {
            Some((lp_bound - achieved_welfare) as f64 / lp_bound as f64)
        } else {
            None
        };

        Self {
            achieved_welfare,
            lp_upper_bound: Some(lp_bound),
            optimality_gap: gap,
            orders_filled,
            unfilled_due_to_liquidity: unfilled_liquidity,
            unfilled_due_to_aon: unfilled_aon,
            total_orders,
        }
    }

    pub fn from_greedy_only(
        achieved_welfare: i64,
        orders_filled: usize,
        unfilled_liquidity: usize,
        unfilled_aon: usize,
        total_orders: usize,
    ) -> Self {
        Self {
            achieved_welfare,
            lp_upper_bound: None,
            optimality_gap: None,
            orders_filled,
            unfilled_due_to_liquidity: unfilled_liquidity,
            unfilled_due_to_aon: unfilled_aon,
            total_orders,
        }
    }

    pub fn fill_rate(&self) -> f64 {
        if self.total_orders > 0 {
            self.orders_filled as f64 / self.total_orders as f64
        } else {
            0.0
        }
    }

    pub fn achievement_ratio(&self) -> Option<f64> {
        self.lp_upper_bound.map(|bound| {
            if bound > 0 {
                self.achieved_welfare as f64 / bound as f64
            } else {
                1.0
            }
        })
    }
}

impl fmt::Display for OptimalityMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Optimality Metrics:")?;
        writeln!(f, "  Achieved welfare: {}", self.achieved_welfare)?;

        if let Some(bound) = self.lp_upper_bound {
            writeln!(f, "  LP upper bound: {}", bound)?;
        }

        if let Some(gap) = self.optimality_gap {
            writeln!(f, "  Optimality gap: {:.2}%", gap * 100.0)?;
        }

        if let Some(ratio) = self.achievement_ratio() {
            writeln!(f, "  Achievement ratio: {:.2}%", ratio * 100.0)?;
        }

        writeln!(f, "  Fill rate: {:.2}% ({}/{})",
            self.fill_rate() * 100.0,
            self.orders_filled,
            self.total_orders)?;
        writeln!(f, "  Unfilled (liquidity): {}", self.unfilled_due_to_liquidity)?;
        writeln!(f, "  Unfilled (AON): {}", self.unfilled_due_to_aon)
    }
}

/// Aggregate metrics across multiple simulation runs.
#[derive(Clone, Debug, Default)]
pub struct AggregateMetrics {
    pub num_instances: usize,
    pub total_welfare: i64,
    pub total_lp_bound: i64,
    pub instances_with_lp: usize,
    pub total_gap: f64,
    pub min_gap: Option<f64>,
    pub max_gap: Option<f64>,
    pub total_fill_rate: f64,
    pub total_filled: usize,
    pub total_orders: usize,
}

impl AggregateMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, metrics: &OptimalityMetrics) {
        self.num_instances += 1;
        self.total_welfare += metrics.achieved_welfare;
        self.total_fill_rate += metrics.fill_rate();
        self.total_filled += metrics.orders_filled;
        self.total_orders += metrics.total_orders;

        if let (Some(bound), Some(gap)) = (metrics.lp_upper_bound, metrics.optimality_gap) {
            self.total_lp_bound += bound;
            self.total_gap += gap;
            self.instances_with_lp += 1;

            match self.min_gap {
                None => self.min_gap = Some(gap),
                Some(min) => self.min_gap = Some(min.min(gap)),
            }
            match self.max_gap {
                None => self.max_gap = Some(gap),
                Some(max) => self.max_gap = Some(max.max(gap)),
            }
        }
    }

    pub fn mean_welfare(&self) -> f64 {
        if self.num_instances > 0 {
            self.total_welfare as f64 / self.num_instances as f64
        } else {
            0.0
        }
    }

    pub fn mean_gap(&self) -> Option<f64> {
        if self.instances_with_lp > 0 {
            Some(self.total_gap / self.instances_with_lp as f64)
        } else {
            None
        }
    }

    pub fn mean_fill_rate(&self) -> f64 {
        if self.num_instances > 0 {
            self.total_fill_rate / self.num_instances as f64
        } else {
            0.0
        }
    }

    pub fn overall_achievement(&self) -> Option<f64> {
        if self.total_lp_bound > 0 {
            Some(self.total_welfare as f64 / self.total_lp_bound as f64)
        } else {
            None
        }
    }
}

impl fmt::Display for AggregateMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Aggregate Metrics ({} instances):", self.num_instances)?;
        writeln!(f, "  Mean welfare: {:.2}", self.mean_welfare())?;

        if let Some(gap) = self.mean_gap() {
            writeln!(f, "  Mean optimality gap: {:.2}%", gap * 100.0)?;
        }
        if let (Some(min), Some(max)) = (self.min_gap, self.max_gap) {
            writeln!(f, "  Gap range: {:.2}% - {:.2}%", min * 100.0, max * 100.0)?;
        }
        if let Some(achievement) = self.overall_achievement() {
            writeln!(f, "  Overall achievement: {:.2}%", achievement * 100.0)?;
        }

        writeln!(f, "  Mean fill rate: {:.2}%", self.mean_fill_rate() * 100.0)?;
        writeln!(f, "  Total orders: {} (filled: {})",
            self.total_orders, self.total_filled)
    }
}

/// Comparison between different scenarios or configurations.
#[derive(Clone, Debug)]
pub struct ScenarioComparison {
    pub name: String,
    pub metrics: AggregateMetrics,
}

impl ScenarioComparison {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            metrics: AggregateMetrics::new(),
        }
    }

    pub fn add(&mut self, m: &OptimalityMetrics) {
        self.metrics.add(m);
    }
}

/// Print a comparison table of multiple scenarios.
pub fn print_comparison_table(scenarios: &[ScenarioComparison]) {
    println!("+------------------------+----------+----------+----------+----------+");
    println!("| Scenario               | Welfare  | Gap      | Fill %   | Orders   |");
    println!("+------------------------+----------+----------+----------+----------+");

    for s in scenarios {
        let gap_str = s.metrics.mean_gap()
            .map(|g| format!("{:.1}%", g * 100.0))
            .unwrap_or("-".to_string());

        println!("| {:<22} | {:>8.0} | {:>8} | {:>7.1}% | {:>8} |",
            truncate(&s.name, 22),
            s.metrics.mean_welfare(),
            gap_str,
            s.metrics.mean_fill_rate() * 100.0,
            s.metrics.total_orders,
        );
    }

    println!("+------------------------+----------+----------+----------+----------+");
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
