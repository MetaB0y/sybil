use matching_engine::NANOS_PER_DOLLAR;

/// Configuration for the simulation.
#[derive(Clone, Debug)]
pub struct SimulationConfig {
    /// Random seed
    pub seed: u64,
    /// Number of binary markets
    pub num_markets: usize,
    /// Number of informed traders
    pub num_informed: usize,
    /// Number of noise traders
    pub num_noise: usize,
    /// Number of market makers
    pub num_mm: usize,
    /// Number of batches to run
    pub num_batches: usize,
    /// Initial balance per agent (in nanos)
    pub initial_balance: i64,
    /// Noise trader activity rate (probability of trading per market per batch)
    pub noise_activity_rate: f64,
    /// Noise trader max quantity per order
    pub noise_max_qty: u64,
    /// Noise trader price noise range (in nanos)
    pub noise_price_noise: u64,
    /// Informed trader minimum edge to trade
    pub informed_min_edge: f64,
    /// Informed trader max quantity per order
    pub informed_max_qty: u64,
    /// Informed trader max position per market
    pub informed_max_position: i64,
    /// MM half-spread (in nanos)
    pub mm_half_spread: u64,
    /// MM quantity per side per market
    pub mm_qty_per_side: u64,
    /// MM capital budget (in nanos)
    pub mm_budget: u64,
    /// MM inventory skew factor
    pub mm_skew_factor: f64,
    /// Whether to print verbose output
    pub verbose: bool,
}

impl SimulationConfig {
    pub fn quick() -> Self {
        Self {
            seed: 42,
            num_markets: 3,
            num_informed: 2,
            num_noise: 5,
            num_mm: 1,
            num_batches: 5,
            initial_balance: 1000 * NANOS_PER_DOLLAR as i64,
            noise_activity_rate: 0.6,
            noise_max_qty: 20,
            noise_price_noise: 50_000_000, // 5 cents
            informed_min_edge: 0.05,
            informed_max_qty: 30,
            informed_max_position: 200,
            mm_half_spread: 25_000_000, // 2.5 cents
            mm_qty_per_side: 50,
            mm_budget: 10_000 * NANOS_PER_DOLLAR,
            mm_skew_factor: 0.1,
            verbose: false,
        }
    }

    pub fn standard() -> Self {
        Self {
            num_markets: 10,
            num_informed: 5,
            num_noise: 20,
            num_mm: 2,
            num_batches: 20,
            initial_balance: 5000 * NANOS_PER_DOLLAR as i64,
            noise_max_qty: 30,
            mm_budget: 50_000 * NANOS_PER_DOLLAR,
            mm_qty_per_side: 100,
            ..Self::quick()
        }
    }

    pub fn stress() -> Self {
        Self {
            num_markets: 30,
            num_informed: 10,
            num_noise: 50,
            num_mm: 3,
            num_batches: 100,
            initial_balance: 10_000 * NANOS_PER_DOLLAR as i64,
            noise_max_qty: 50,
            mm_budget: 100_000 * NANOS_PER_DOLLAR,
            mm_qty_per_side: 200,
            ..Self::quick()
        }
    }
}
