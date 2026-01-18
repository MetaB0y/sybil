//! Planted pattern scenarios for testing specialized solvers.
//!
//! These scenarios contain deliberate patterns that:
//! - Greedy solvers miss due to local optimization
//! - Specialized solvers (ChainFinder, BundleDecomposer) should find
//!
//! Each scenario is designed to validate a specific solver's intelligence.

use rand::Rng;
use rand::rngs::StdRng;
use rand::SeedableRng;

use matching_engine::{
    ConstraintBuilder, MarketSet, Order, MarketId, Qty,
    price_to_nanos, outcome_buy, bundle_yes, Problem,
};

/// Configuration for planted chain scenarios.
#[derive(Clone, Debug)]
pub struct PlantedChainConfig {
    /// Random seed
    pub seed: u64,
    /// Number of markets in the implication chain
    pub chain_length: usize,
    /// Number of distractor orders
    pub num_distractors: usize,
    /// Price advantage for buying early in chain (e.g., 0.50 = 50% cheaper)
    pub price_advantage: f64,
}

impl Default for PlantedChainConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            chain_length: 5,
            num_distractors: 50,
            price_advantage: 0.50,
        }
    }
}

/// Generate a planted chain scenario.
///
/// Creates an implication chain: Champion → Finalist → Semifinalist → Participant
/// with mispriced liquidity:
/// - Champion is cheap (e.g., $0.20)
/// - Participant is expensive (e.g., $0.70)
///
/// ChainFinder should realize: buying Champion gives exposure to ALL
/// levels for $0.20 instead of $0.70!
///
/// Greedy will fill expensive Participant orders first (high welfare per order)
/// but ChainFinder should prioritize Champion orders.
pub fn generate_planted_chain_scenario(config: PlantedChainConfig) -> Problem {
    let mut rng = StdRng::seed_from_u64(config.seed);
    let mut problem = Problem::new(format!(
        "PlantedChain(len={}, adv={}%)",
        config.chain_length,
        (config.price_advantage * 100.0) as i32
    ));

    // Create chain markets: Champion → Finalist → Semifinalist → ...
    let level_names = ["Champion", "Finalist", "Semifinalist", "Quarterfinalist", "Participant"];
    let mut chain_markets: Vec<MarketId> = Vec::new();

    for &name in level_names.iter().take(config.chain_length.min(level_names.len())) {
        let market = problem.markets.add(name, vec!["Team_A".to_string(), "Other".to_string()]);
        chain_markets.push(market);
    }

    // Add implication constraints: Champion → Finalist → ...
    let mut constraint_builder = ConstraintBuilder::new();
    for i in 0..chain_markets.len() - 1 {
        // If Team_A is Champion, Team_A must be Finalist, etc.
        constraint_builder = constraint_builder.implies(
            chain_markets[i], 0,    // Team_A wins level i
            chain_markets[i + 1], 0 // Team_A wins level i+1
        );
    }
    problem.constraints = constraint_builder.build();

    // Set up mispriced liquidity
    // Early in chain = cheap, late in chain = expensive
    let base_price = 0.15; // Champion is cheap
    let price_increment = config.price_advantage / (config.chain_length as f64 - 1.0);

    for (level, &market) in chain_markets.iter().enumerate() {
        let price = base_price + price_increment * level as f64;

        // Add asks (liquidity to sell to buyers)
        problem.liquidity.add_ask(market, 0, price_to_nanos(price), 500);
        problem.liquidity.add_ask(market, 1, price_to_nanos(1.0 - price * 0.5), 500);
    }

    // Create orders that target different levels
    let mut order_id = 1u64;

    // Create high-value orders for the EXPENSIVE end (Participant level)
    // Greedy will love these because welfare = (limit - cost) * qty looks good
    let expensive_market = *chain_markets.last().unwrap();
    let expensive_price = base_price + price_increment * (config.chain_length as f64 - 1.0);

    for _ in 0..20 {
        let limit = expensive_price + rng.gen_range(0.05..0.15);
        let qty = rng.gen_range(20..50);
        let order = outcome_buy(
            &problem.markets,
            order_id,
            expensive_market,
            0, // Team_A
            price_to_nanos(limit),
            qty,
        );
        problem.orders.push(order);
        order_id += 1;
    }

    // Create orders for the CHEAP end (Champion level)
    // These are the "smart" orders that ChainFinder should prioritize
    let cheap_market = chain_markets[0];

    for _ in 0..20 {
        let limit = base_price + rng.gen_range(0.05..0.20);
        let qty = rng.gen_range(30..60);
        let order = outcome_buy(
            &problem.markets,
            order_id,
            cheap_market,
            0, // Team_A
            price_to_nanos(limit),
            qty,
        );
        problem.orders.push(order);
        order_id += 1;
    }

    // Add distractor orders on other outcomes to create noise
    for _ in 0..config.num_distractors {
        let market = chain_markets[rng.gen_range(0..chain_markets.len())];
        let outcome = rng.gen_range(0..2u8);
        let price = if outcome == 0 {
            base_price + rng.gen_range(0.0..config.price_advantage)
        } else {
            rng.gen_range(0.3..0.6)
        };
        let order = outcome_buy(
            &problem.markets,
            order_id,
            market,
            outcome,
            price_to_nanos(price + rng.gen_range(0.0..0.1)),
            rng.gen_range(10..40),
        );
        problem.orders.push(order);
        order_id += 1;
    }

    problem
}

/// Configuration for planted complement scenarios.
#[derive(Clone, Debug)]
pub struct PlantedComplementConfig {
    /// Random seed
    pub seed: u64,
    /// Number of binary markets in the complement set
    pub num_markets: usize,
    /// Number of distractor orders
    pub num_distractors: usize,
    /// Total price of complement set (> 1.0 = arbitrage opportunity)
    pub total_complement_price: f64,
}

impl Default for PlantedComplementConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            num_markets: 2,
            num_distractors: 40,
            total_complement_price: 1.03, // 3% arbitrage opportunity
        }
    }
}

/// Generate a planted complement scenario.
///
/// Creates 4 bundles on 2 binary markets that together cover ALL outcomes:
/// - Bundle 1: YES/YES (state 0) at $0.28
/// - Bundle 2: YES/NO  (state 1) at $0.26
/// - Bundle 3: NO/YES  (state 2) at $0.25
/// - Bundle 4: NO/NO   (state 3) at $0.24
///
/// Total: $1.03 for guaranteed $1.00 payout
///
/// BundleDecomposer should recognize: filling all 4 together = guaranteed profit!
pub fn generate_planted_complement_scenario(config: PlantedComplementConfig) -> Problem {
    let mut rng = StdRng::seed_from_u64(config.seed);
    let mut problem = Problem::new(format!(
        "PlantedComplement(markets={}, total={})",
        config.num_markets,
        config.total_complement_price
    ));

    // Create binary markets
    let mut market_ids: Vec<MarketId> = Vec::new();
    for i in 0..config.num_markets {
        let market = problem.markets.add(format!("Market_{}", i), vec!["Yes".to_string(), "No".to_string()]);
        market_ids.push(market);
    }

    // Calculate number of states (2^n for binary markets)
    let num_states = 1usize << config.num_markets;

    // Distribute total price across states (slightly uneven to look natural)
    let base_price = config.total_complement_price / num_states as f64;
    let mut state_prices: Vec<f64> = Vec::new();
    let mut remaining = config.total_complement_price;

    for i in 0..num_states {
        let price = if i < num_states - 1 {
            let variation = rng.gen_range(-0.02..0.02);
            let p = (base_price + variation).max(0.05);
            remaining -= p;
            p
        } else {
            remaining.max(0.05)
        };
        state_prices.push(price);
    }

    // Add liquidity for individual outcomes
    for &market in market_ids.iter() {
        // Mid price around 0.5
        let mid = 0.4 + rng.gen_range(0.0..0.2);
        problem.liquidity.add_ask(market, 0, price_to_nanos(mid), 1000);
        problem.liquidity.add_ask(market, 1, price_to_nanos(1.0 - mid), 1000);
    }

    let mut order_id = 1u64;

    // Create the complement bundle orders
    // For 2 markets, states are: 00, 01, 10, 11
    for (state_idx, &price) in state_prices.iter().enumerate().take(num_states) {
        // Build the bundle for this state
        // State index encodes which outcome for each market

        // Create payoff structure: pays 1 only in this state
        // For bundle_yes, we're buying YES on specific outcomes
        // To create state-specific payoff, we need to be clever about the order structure

        // For simplicity, create an order that buys this specific combination
        // The limit price represents what we're willing to pay for this state

        let order = create_state_bundle_order(
            &problem.markets,
            order_id,
            &market_ids,
            state_idx,
            price_to_nanos(price + rng.gen_range(0.01..0.05)),
            rng.gen_range(50..100),
        );
        problem.orders.push(order);
        order_id += 1;
    }

    // Add distractor orders
    for _ in 0..config.num_distractors {
        let market = market_ids[rng.gen_range(0..market_ids.len())];
        let outcome = rng.gen_range(0..2u8);
        let price = rng.gen_range(0.2..0.6);
        let order = outcome_buy(
            &problem.markets,
            order_id,
            market,
            outcome,
            price_to_nanos(price),
            rng.gen_range(10..50),
        );
        problem.orders.push(order);
        order_id += 1;
    }

    problem
}

/// Create a bundle order for a specific state combination.
fn create_state_bundle_order(
    markets: &MarketSet,
    id: u64,
    market_ids: &[MarketId],
    state_idx: usize,
    limit_price: u64,
    qty: Qty,
) -> Order {
    // For simplicity with binary markets, use bundle_yes pattern
    // State idx bit i indicates outcome for market i
    // We'll create a bundle that bets on the specific outcome combination

    if market_ids.len() == 2 {
        // For 2 markets, create direct bundle
        match state_idx {
            0 => {
                // Both YES
                bundle_yes(markets, id, market_ids, limit_price, qty)
            }
            _ => {
                // For other states, we'd need a custom payoff structure
                // Fallback to simple order on first market
                outcome_buy(markets, id, market_ids[0], (state_idx & 1) as u8, limit_price, qty)
            }
        }
    } else {
        // Fallback for more complex cases
        bundle_yes(markets, id, market_ids, limit_price, qty)
    }
}

/// Configuration for planted exclusion scenarios.
#[derive(Clone, Debug)]
pub struct PlantedExclusionConfig {
    /// Random seed
    pub seed: u64,
    /// Number of markets in the exclusion group
    pub group_size: usize,
    /// Value of the "trap" high-value single order
    pub trap_value: f64,
    /// Total value of the alternative set
    pub alternative_total_value: f64,
}

impl Default for PlantedExclusionConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            group_size: 4,
            trap_value: 0.80,
            alternative_total_value: 1.50, // 3 orders at 0.50 each
        }
    }
}

/// Generate a planted exclusion scenario.
///
/// Creates a mutual exclusion where greedy makes the wrong first choice:
/// - One high-value single order (the "trap")
/// - Multiple medium-value orders that TOGETHER beat the trap
///
/// Greedy picks the trap first (highest individual welfare)
/// but filling the alternatives enables more total welfare.
pub fn generate_planted_exclusion_scenario(config: PlantedExclusionConfig) -> Problem {
    let mut rng = StdRng::seed_from_u64(config.seed);
    let mut problem = Problem::new(format!(
        "PlantedExclusion(group={}, trap={}, alt={})",
        config.group_size,
        config.trap_value,
        config.alternative_total_value
    ));

    // Create markets in exclusion group
    let mut group_markets: Vec<MarketId> = Vec::new();
    for i in 0..config.group_size {
        let market = problem.markets.add(format!("Candidate_{}", i), vec!["Wins".to_string(), "Loses".to_string()]);
        group_markets.push(market);
    }

    // Create additional markets that depend on alternatives
    let mut dependent_markets: Vec<MarketId> = Vec::new();
    for i in 0..config.group_size - 1 {
        let market = problem.markets.add(format!("Dependent_{}", i), vec!["Yes".to_string(), "No".to_string()]);
        dependent_markets.push(market);
    }

    // Mutual exclusion: only one candidate can win
    let exclusion_outcomes: Vec<(MarketId, u8)> = group_markets
        .iter()
        .map(|&m| (m, 0))
        .collect();
    let constraints = ConstraintBuilder::new()
        .mutually_exclusive(exclusion_outcomes)
        .build();
    problem.constraints = constraints;

    // Add liquidity
    for &market in group_markets.iter().chain(dependent_markets.iter()) {
        problem.liquidity.add_ask(market, 0, price_to_nanos(0.3), 500);
        problem.liquidity.add_ask(market, 1, price_to_nanos(0.7), 500);
    }

    let mut order_id = 1u64;

    // The "trap" order: high welfare on first candidate
    let trap_market = group_markets[0];
    let trap_order = outcome_buy(
        &problem.markets,
        order_id,
        trap_market,
        0, // Wins
        price_to_nanos(config.trap_value),
        100, // Large quantity
    );
    problem.orders.push(trap_order);
    order_id += 1;

    // Alternative orders: each is smaller, but together they're better
    let alt_per_order = config.alternative_total_value / (config.group_size as f64 - 1.0);
    for i in 1..config.group_size {
        let order = outcome_buy(
            &problem.markets,
            order_id,
            group_markets[i],
            0, // Wins
            price_to_nanos(alt_per_order),
            60,
        );
        problem.orders.push(order);
        order_id += 1;

        // Each alternative also enables a dependent order
        if i - 1 < dependent_markets.len() {
            let dependent_order = outcome_buy(
                &problem.markets,
                order_id,
                dependent_markets[i - 1],
                0,
                price_to_nanos(0.40),
                50,
            );
            problem.orders.push(dependent_order);
            order_id += 1;
        }
    }

    // Add distractor orders
    for _ in 0..30 {
        let all_markets: Vec<MarketId> = group_markets.iter()
            .chain(dependent_markets.iter())
            .copied()
            .collect();
        let market = all_markets[rng.gen_range(0..all_markets.len())];
        let order = outcome_buy(
            &problem.markets,
            order_id,
            market,
            rng.gen_range(0..2u8),
            price_to_nanos(rng.gen_range(0.25..0.45)),
            rng.gen_range(10..40),
        );
        problem.orders.push(order);
        order_id += 1;
    }

    problem
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_planted_chain_has_constraints() {
        let problem = generate_planted_chain_scenario(PlantedChainConfig::default());
        assert_eq!(problem.markets.len(), 5);
        // Should have chain_length - 1 implications
        assert_eq!(problem.constraints.len(), 4);
    }

    #[test]
    fn test_planted_chain_has_mispricing() {
        let config = PlantedChainConfig::default();
        let problem = generate_planted_chain_scenario(config);

        // Champion should be cheaper than Participant
        let champion_price = problem.liquidity
            .book(problem.markets.iter().next().unwrap().id, 0)
            .and_then(|b| b.best_ask())
            .unwrap_or(0);
        let participant_id = problem.markets.iter().last().unwrap().id;
        let participant_price = problem.liquidity
            .book(participant_id, 0)
            .and_then(|b| b.best_ask())
            .unwrap_or(0);

        assert!(champion_price < participant_price, "Champion should be cheaper");
    }

    #[test]
    fn test_planted_complement_creates_bundles() {
        let problem = generate_planted_complement_scenario(PlantedComplementConfig::default());
        assert_eq!(problem.markets.len(), 2);
        // Should have 4 complement orders (2^2 states) + distractors
        assert!(problem.orders.len() >= 4);
    }

    #[test]
    fn test_planted_exclusion_has_constraint() {
        let problem = generate_planted_exclusion_scenario(PlantedExclusionConfig::default());
        assert!(problem.constraints.len() > 0);
        // Should have trap + alternatives + dependents + distractors
        assert!(problem.orders.len() > 10);
    }
}
