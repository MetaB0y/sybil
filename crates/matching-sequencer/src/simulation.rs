use std::collections::{HashMap, HashSet};

use matching_engine::{MarketGroup, MarketId, MarketSet, MmId, Nanos};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::account::{AccountId, AccountStore};
use crate::agent::informed::InformedTrader;
use crate::agent::market_maker::MarketMakerAgent;
use crate::agent::noise::NoiseTrader;
use crate::agent::{Agent, MarketView};
use crate::metrics::{self, AgentPnL, BatchMetrics};
use crate::scenario::{
    EventMarketMap, NewsItem, NewsVisibility, PublicBeliefs, Scenario,
};
use crate::sequencer::{batch_result_from_block, BlockSequencer, OrderSubmission};
use crate::settlement;

pub struct SimulationRunner {
    sequencer: BlockSequencer,
    agents: Vec<Box<dyn Agent>>,
    agent_info: Vec<(String, AccountId, i64)>, // (name, account_id, initial_balance)
    true_probs: HashMap<MarketId, f64>,
    price_history: Vec<HashMap<MarketId, Vec<Nanos>>>,
    batch_metrics: Vec<BatchMetrics>,
    event_map: EventMarketMap,
    public_beliefs: PublicBeliefs,
    pending_news: Vec<NewsItem>,
    /// (batch, event_index, winner_index)
    pending_resolutions: Vec<(usize, usize, usize)>,
    resolved_markets: HashSet<MarketId>,
    scenario: Scenario,
}

/// Results of the full simulation.
pub struct SimulationResult {
    pub batch_metrics: Vec<BatchMetrics>,
    pub agent_pnl: Vec<AgentPnL>,
    pub resolved_pnl: Vec<AgentPnL>,
    pub final_price_error: f64,
    pub true_probs: HashMap<MarketId, f64>,
    pub price_history: Vec<HashMap<MarketId, Vec<Nanos>>>,
    pub scenario: Scenario,
    pub event_map: EventMarketMap,
}

impl SimulationRunner {
    /// Create a simulation from a scenario definition.
    pub fn from_scenario(scenario: &Scenario) -> Self {
        use rand::Rng;

        let mut rng = ChaCha8Rng::seed_from_u64(scenario.seed);
        let mut accounts = AccountStore::new();
        let mut markets = MarketSet::new();
        let mut market_groups = Vec::new();
        let mut true_probs = HashMap::new();
        let mut event_map = EventMarketMap::new();

        // Create markets for each event
        for (event_idx, event) in scenario.events.iter().enumerate() {
            let n = event.outcomes.len();
            let mut event_market_ids = Vec::new();

            if n == 2 {
                // Binary event: 1 market, no MarketGroup
                let name = format!("{}: {}?", event.name, event.outcomes[0].name);
                let mid = markets.add_binary(name);
                event_market_ids.push(mid);
                // true_prob for YES (outcome 0)
                true_probs.insert(mid, event.true_probs[0]);
                event_map.market_to_event.insert(mid, (event_idx, 0));
            } else {
                // N-outcome event: N binary markets + 1 MarketGroup
                let mut group = MarketGroup::new(&event.name);
                for (outcome_idx, outcome) in event.outcomes.iter().enumerate() {
                    let name = format!("{}: {}", event.name, outcome.name);
                    let mid = markets.add_binary(name);
                    event_market_ids.push(mid);
                    true_probs.insert(mid, event.true_probs[outcome_idx]);
                    event_map
                        .market_to_event
                        .insert(mid, (event_idx, outcome_idx));
                    group.add_market(mid);
                }
                market_groups.push(group);
            }

            event_map.event_markets.push(event_market_ids);
        }

        // Initialize public beliefs at uniform priors
        let public_beliefs = PublicBeliefs::from_events(&scenario.events, &event_map);

        // Build pending resolutions from events
        let mut pending_resolutions: Vec<(usize, usize, usize)> = Vec::new();
        for (event_idx, event) in scenario.events.iter().enumerate() {
            if let Some(batch) = event.resolve_at_batch {
                pending_resolutions.push((batch, event_idx, event.winner));
            }
        }
        pending_resolutions.sort_by_key(|&(b, _, _)| b);

        let mut agents: Vec<Box<dyn Agent>> = Vec::new();
        let mut agent_info: Vec<(String, AccountId, i64)> = Vec::new();

        // Create informed traders (they know the true probabilities)
        for i in 0..scenario.num_informed {
            let account_id = accounts.create_account(scenario.initial_balance);
            let name = format!("Informed-{}", i);
            let agent = InformedTrader::new(
                name.clone(),
                account_id,
                markets.clone(),
                true_probs.clone(),
                scenario.informed_min_edge,
                scenario.informed_max_qty,
                scenario.informed_max_position,
            );
            agent_info.push((name, account_id, scenario.initial_balance));
            agents.push(Box::new(agent));
        }

        // Create noise traders
        for i in 0..scenario.num_noise {
            let account_id = accounts.create_account(scenario.initial_balance);
            let name = format!("Noise-{}", i);
            let seed: u64 = rng.random();
            let agent_rng = Box::new(ChaCha8Rng::seed_from_u64(seed));
            let agent = NoiseTrader::new(
                name.clone(),
                account_id,
                markets.clone(),
                scenario.noise_activity_rate,
                scenario.noise_max_qty,
                scenario.noise_price_noise,
                agent_rng,
            );
            agent_info.push((name, account_id, scenario.initial_balance));
            agents.push(Box::new(agent));
        }

        // Create market makers
        for i in 0..scenario.num_mm {
            let account_id = accounts.create_account(scenario.initial_balance);
            let name = format!("MM-{}", i);
            let agent = MarketMakerAgent::new(
                name.clone(),
                account_id,
                MmId::new(i as u64),
                markets.clone(),
                scenario.mm_half_spread,
                scenario.mm_qty_per_side,
                scenario.mm_budget,
                scenario.mm_skew_factor,
            );
            agent_info.push((name, account_id, scenario.initial_balance));
            agents.push(Box::new(agent));
        }

        let sequencer = BlockSequencer::new(accounts, markets, market_groups);

        Self {
            sequencer,
            agents,
            agent_info,
            true_probs,
            price_history: Vec::new(),
            batch_metrics: Vec::new(),
            event_map,
            public_beliefs,
            pending_news: scenario.news.clone(),
            pending_resolutions,
            resolved_markets: HashSet::new(),
            scenario: scenario.clone(),
        }
    }

    /// Dispatch news items that arrive at the given batch.
    /// Public news updates PublicBeliefs; InformedOnly news does not.
    fn dispatch_news(&mut self, batch: usize) {
        let event_map = self.event_map.clone();

        let arriving: Vec<NewsItem> = self
            .pending_news
            .iter()
            .filter(|n| n.batch == batch)
            .cloned()
            .collect();

        for news in &arriving {
            if news.visibility == NewsVisibility::Public {
                self.public_beliefs.update(news.event_index, &news.updated_probs, &event_map);
            }
            // InformedOnly news: no public belief update.
            // Informed traders already know true probs, so the information
            // asymmetry is implicitly maintained.
        }

        self.pending_news.retain(|n| n.batch != batch);
    }

    /// Resolve events scheduled at the given batch.
    /// Settles positions and removes resolved markets from active trading.
    fn resolve_events_at(&mut self, batch: usize) {
        let event_map = self.event_map.clone();

        let to_resolve: Vec<(usize, usize)> = self
            .pending_resolutions
            .iter()
            .filter(|&&(b, _, _)| b == batch)
            .map(|&(_, event_idx, winner)| (event_idx, winner))
            .collect();

        for (event_idx, winner_outcome) in &to_resolve {
            let market_ids = &event_map.event_markets[*event_idx];

            for (outcome_idx, &mid) in market_ids.iter().enumerate() {
                // The winning outcome's market resolves YES; others resolve NO
                let winning = if market_ids.len() == 1 {
                    // Binary event: winner 0 = YES wins, winner 1 = NO wins
                    if *winner_outcome == 0 { 0u8 } else { 1u8 }
                } else {
                    // Multi-outcome: the market matching the winner resolves YES
                    if outcome_idx == *winner_outcome { 0u8 } else { 1u8 }
                };
                settlement::resolve_market(&mut self.sequencer.accounts, mid, winning);
                self.resolved_markets.insert(mid);
            }

            // Remove from public beliefs
            self.public_beliefs.remove_markets(market_ids);

            // Remove market group if this was a multi-outcome event
            let market_set: HashSet<MarketId> = market_ids.iter().copied().collect();
            self.sequencer.market_groups_mut()
                .retain(|g| !g.markets.iter().any(|m| market_set.contains(m)));
        }

        self.pending_resolutions.retain(|&(b, _, _)| b != batch);
    }

    /// Run the full simulation.
    pub fn run(&mut self, num_batches: usize) -> SimulationResult {
        for batch in 0..num_batches {
            // Dispatch news and resolve events before running the batch
            self.dispatch_news(batch);
            self.resolve_events_at(batch);
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

        // Only compute price error for non-resolved markets
        let active_true_probs: HashMap<MarketId, f64> = self
            .true_probs
            .iter()
            .filter(|(mid, _)| !self.resolved_markets.contains(mid))
            .map(|(&mid, &p)| (mid, p))
            .collect();

        let final_price_error = if let Some(last) = self.price_history.last() {
            metrics::price_convergence(last, &active_true_probs)
        } else {
            1.0
        };

        // Resolve remaining markets at end of simulation
        let scenario = self.scenario.clone();
        let event_map = self.event_map.clone();
        for (event_idx, event) in scenario.events.iter().enumerate() {
            let market_ids = &event_map.event_markets[event_idx];
            // Skip already-resolved events
            if market_ids.iter().any(|m| self.resolved_markets.contains(m)) {
                continue;
            }
            for (outcome_idx, &mid) in market_ids.iter().enumerate() {
                let winning = if market_ids.len() == 1 {
                    if event.winner == 0 { 0u8 } else { 1u8 }
                } else {
                    if outcome_idx == event.winner { 0u8 } else { 1u8 }
                };
                settlement::resolve_market(
                    &mut self.sequencer.accounts,
                    mid,
                    winning,
                );
            }
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
            scenario: self.scenario.clone(),
            event_map: self.event_map.clone(),
        }
    }

    fn run_single_batch(&mut self, batch: usize) {
        // Build market view, filtering out resolved markets
        let last_prices = self
            .price_history
            .last()
            .cloned()
            .unwrap_or_default();

        let active_markets: Vec<_> = self
            .sequencer
            .markets()
            .iter()
            .filter(|m| !self.resolved_markets.contains(&m.id))
            .map(|m| (m.id, m.name.clone()))
            .collect();

        let market_view = MarketView {
            batch,
            markets: active_markets,
            last_prices,
            market_groups: self.sequencer.market_groups().to_vec(),
            public_beliefs: Some(self.public_beliefs.as_map().clone()),
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

        // Produce the block
        let (block, pipeline_result) = self.sequencer.produce_block(submissions, 0);
        let result = batch_result_from_block(&block, pipeline_result);

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
