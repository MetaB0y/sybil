use std::collections::HashMap;

use matching_engine::{MarketGroup, MarketId, MarketSet, MmId, Nanos};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::account::{AccountId, AccountStore};
use crate::agent::informed::InformedTrader;
use crate::agent::market_maker::MarketMakerAgent;
use crate::agent::noise::NoiseTrader;
use crate::agent::{Agent, MarketView};
use crate::config::SimulationConfig;
use crate::metrics::{self, AgentPnL, BatchMetrics};
use crate::sequencer::{BatchSequencer, OrderSubmission};
use crate::settlement;

pub struct SimulationRunner {
    sequencer: BatchSequencer,
    agents: Vec<Box<dyn Agent>>,
    agent_info: Vec<(String, AccountId, i64)>, // (name, account_id, initial_balance)
    markets: MarketSet,
    market_groups: Vec<MarketGroup>,
    true_probs: HashMap<MarketId, f64>,
    price_history: Vec<HashMap<MarketId, Vec<Nanos>>>,
    batch_metrics: Vec<BatchMetrics>,
}

/// Results of the full simulation.
pub struct SimulationResult {
    pub batch_metrics: Vec<BatchMetrics>,
    pub agent_pnl: Vec<AgentPnL>,
    pub resolved_pnl: Vec<AgentPnL>,
    pub final_price_error: f64,
    pub true_probs: HashMap<MarketId, f64>,
    pub price_history: Vec<HashMap<MarketId, Vec<Nanos>>>,
}

impl SimulationRunner {
    /// Create a simulation from configuration.
    pub fn from_config(config: &SimulationConfig) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
        let mut accounts = AccountStore::new();

        // Create markets
        let mut markets = MarketSet::new();
        let mut market_ids = Vec::new();
        for i in 0..config.num_markets {
            let id = markets.add_binary(format!("Market {}", i));
            market_ids.push(id);
        }

        // Generate true probabilities
        let mut true_probs = HashMap::new();
        for &mid in &market_ids {
            use rand::Rng;
            let p: f64 = rng.gen_range(0.1..0.9);
            true_probs.insert(mid, p);
        }

        let mut agents: Vec<Box<dyn Agent>> = Vec::new();
        let mut agent_info: Vec<(String, AccountId, i64)> = Vec::new();

        // Create informed traders
        for i in 0..config.num_informed {
            let account_id = accounts.create_account(config.initial_balance);
            let name = format!("Informed-{}", i);
            let agent = InformedTrader::new(
                name.clone(),
                account_id,
                markets.clone(),
                true_probs.clone(),
                config.informed_min_edge,
                config.informed_max_qty,
                config.informed_max_position,
            );
            agent_info.push((name, account_id, config.initial_balance));
            agents.push(Box::new(agent));
        }

        // Create noise traders
        for i in 0..config.num_noise {
            let account_id = accounts.create_account(config.initial_balance);
            let name = format!("Noise-{}", i);
            use rand::Rng;
            let seed: u64 = rng.gen();
            let agent_rng = Box::new(ChaCha8Rng::seed_from_u64(seed));
            let agent = NoiseTrader::new(
                name.clone(),
                account_id,
                markets.clone(),
                config.noise_activity_rate,
                config.noise_max_qty,
                config.noise_price_noise,
                agent_rng,
            );
            agent_info.push((name, account_id, config.initial_balance));
            agents.push(Box::new(agent));
        }

        // Create market makers
        for i in 0..config.num_mm {
            let account_id = accounts.create_account(config.initial_balance);
            let name = format!("MM-{}", i);
            let agent = MarketMakerAgent::new(
                name.clone(),
                account_id,
                MmId::new(i as u64),
                markets.clone(),
                config.mm_half_spread,
                config.mm_qty_per_side,
                config.mm_budget,
                config.mm_skew_factor,
            );
            agent_info.push((name, account_id, config.initial_balance));
            agents.push(Box::new(agent));
        }

        let sequencer = BatchSequencer::new(accounts);

        // No market groups in basic binary market setup
        let market_groups = Vec::new();

        Self {
            sequencer,
            agents,
            agent_info,
            markets,
            market_groups,
            true_probs,
            price_history: Vec::new(),
            batch_metrics: Vec::new(),
        }
    }

    /// Run the full simulation.
    pub fn run(&mut self, num_batches: usize) -> SimulationResult {
        for batch in 0..num_batches {
            self.run_single_batch(batch);
        }

        // Compute pre-resolution PnL
        let last_prices = self
            .price_history
            .last()
            .cloned()
            .unwrap_or_default();

        let agent_pnl = metrics::compute_agent_pnl(
            &self.agent_info,
            &self.sequencer.accounts,
            &last_prices,
        );

        let final_price_error = if let Some(last) = self.price_history.last() {
            metrics::price_convergence(last, &self.true_probs)
        } else {
            1.0
        };

        // Resolve markets: use true probability to determine winners
        // (probability > 0.5 means YES wins, for simulation purposes)
        for (&market_id, &prob) in &self.true_probs {
            let winning_outcome = if prob >= 0.5 { 0u8 } else { 1u8 };
            settlement::resolve_market(&mut self.sequencer.accounts, market_id, winning_outcome);
        }

        let resolved_pnl = metrics::compute_resolved_pnl(
            &self.agent_info,
            &self.sequencer.accounts,
        );

        SimulationResult {
            batch_metrics: self.batch_metrics.clone(),
            agent_pnl,
            resolved_pnl,
            final_price_error,
            true_probs: self.true_probs.clone(),
            price_history: self.price_history.clone(),
        }
    }

    fn run_single_batch(&mut self, batch: usize) {
        // Build market view
        let last_prices = self
            .price_history
            .last()
            .cloned()
            .unwrap_or_default();

        let market_view = MarketView {
            batch,
            markets: self
                .markets
                .iter()
                .map(|m| (m.id, m.name.clone()))
                .collect(),
            last_prices,
            market_groups: self.market_groups.clone(),
        };

        // Collect submissions from all agents
        let mut submissions: Vec<OrderSubmission> = Vec::new();

        for agent in &mut self.agents {
            let account_id = agent.account_id();
            let account = self.sequencer.accounts.get(account_id).unwrap();
            let sub = agent.submit_orders(&market_view, account);

            submissions.push(OrderSubmission {
                account_id,
                orders: sub.orders,
                mm_constraint: sub.mm_constraint,
            });
        }

        // Run the batch
        let result = self
            .sequencer
            .run_batch(submissions, &self.markets, &self.market_groups);

        // Record metrics
        let batch_metrics = BatchMetrics {
            batch,
            total_welfare: result.total_welfare,
            total_volume: result.total_volume,
            orders_submitted: result.orders_submitted,
            orders_filled: result.orders_filled,
            rejections: result.rejections.len(),
            clearing_prices: result.clearing_prices.clone(),
        };

        self.batch_metrics.push(batch_metrics);
        self.price_history.push(result.clearing_prices);
    }
}
