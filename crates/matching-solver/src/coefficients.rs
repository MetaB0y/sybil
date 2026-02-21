//! Generalized order coefficient computation for LP/MILP formulations.
//!
//! Decomposes order payoff vectors into per-market marginal contributions
//! (c_YES, c_NO, alpha, beta). This is the bridge between arbitrary payoff
//! vectors and per-market balance/pricing constraints.

use std::collections::HashMap;

use matching_engine::{MarketId, Order, NANOS_PER_DOLLAR};

/// Per-market marginal contribution coefficients for an order.
///
/// Decomposition mirrors `settle_generic`: for each market m, we compute
/// the average payoff when m=YES vs m=NO across all states.
pub struct OrderCoefficients {
    /// Average payoff when market m outcome = YES (outcome 0)
    pub c_yes: HashMap<MarketId, f64>,
    /// Average payoff when market m outcome = NO (outcome 1)
    pub c_no: HashMap<MarketId, f64>,
    /// alpha_m = c_YES_m - c_NO_m (price sensitivity per market)
    pub alpha: HashMap<MarketId, f64>,
    /// beta = NANOS_PER_DOLLAR * sum(c_NO_m) (price-independent offset)
    pub beta: f64,
}

/// Compute per-market marginal contribution coefficients from payoff vector.
///
/// For each market m (at index m_idx in the order's market list):
/// - stride = 1 << m_idx (binary markets)
/// - For each state s, `(s / stride) % 2` gives the outcome for market m
/// - c_YES_m = average of payoffs where market m = YES (outcome 0)
/// - c_NO_m = average of payoffs where market m = NO (outcome 1)
pub fn precompute_coefficients(order: &Order) -> OrderCoefficients {
    let num_markets = order.num_markets as usize;
    let num_states = order.num_states as usize;
    let nanos_f = NANOS_PER_DOLLAR as f64;

    let mut c_yes = HashMap::new();
    let mut c_no = HashMap::new();
    let mut alpha = HashMap::new();
    let mut beta_sum = 0.0;

    for m_idx in 0..num_markets {
        let market = order.markets[m_idx];
        if market.is_none() {
            continue;
        }

        let stride = 1usize << m_idx;

        let mut yes_sum: f64 = 0.0;
        let mut yes_count: usize = 0;
        let mut no_sum: f64 = 0.0;
        let mut no_count: usize = 0;

        for s in 0..num_states {
            let outcome_for_market = (s / stride) % 2;
            let payoff = order.payoffs[s] as f64;
            if outcome_for_market == 0 {
                yes_sum += payoff;
                yes_count += 1;
            } else {
                no_sum += payoff;
                no_count += 1;
            }
        }

        let c_y = if yes_count > 0 {
            yes_sum / yes_count as f64
        } else {
            0.0
        };
        let c_n = if no_count > 0 {
            no_sum / no_count as f64
        } else {
            0.0
        };

        c_yes.insert(market, c_y);
        c_no.insert(market, c_n);
        alpha.insert(market, c_y - c_n);
        beta_sum += c_n;
    }

    OrderCoefficients {
        c_yes,
        c_no,
        alpha,
        beta: nanos_f * beta_sum,
    }
}

/// Determine the sign for an order in the welfare objective.
///
/// Uses `is_seller()` for consistency with the verifier and welfare calculation.
/// - Buyer (no negative payoffs) -> +1.0
/// - Seller (any negative payoff) -> -1.0
pub fn order_sign(order: &Order) -> f64 {
    if order.is_seller() {
        -1.0
    } else {
        1.0
    }
}
