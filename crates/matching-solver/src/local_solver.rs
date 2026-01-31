//! Per-market clearing.
//!
//! # Market Clearing
//!
//! For each market, finds clearing prices by matching buy and sell orders.
//! Each outcome is cleared independently. All trades for an outcome execute
//! at the same clearing price (UCP).
//!
//! # Solver
//!
//! - [`LocalSolver::solve_market`]: Per-outcome clearing

use serde::Serialize;

use matching_engine::{
    Fill, MarketId, MarketSet, Nanos, Order, Qty, NANOS_PER_DOLLAR,
};

/// Solution for a single market.
#[derive(Clone, Debug, Serialize)]
pub struct MarketSolution {
    /// Market ID this solution is for
    pub market_id: MarketId,
    /// Clearing prices per outcome
    pub prices: Vec<Nanos>,
    /// Fills for orders in this market
    pub fills: Vec<Fill>,
    /// Total welfare achieved
    pub welfare: i64,
    /// Orders that couldn't be filled
    pub unfilled: Vec<u64>,
}

impl MarketSolution {
    /// Create an empty solution for a market.
    pub fn empty(market_id: MarketId, num_outcomes: usize) -> Self {
        // Default prices: uniform distribution
        let price_per_outcome = NANOS_PER_DOLLAR / num_outcomes as u64;
        let prices = vec![price_per_outcome as Nanos; num_outcomes];

        Self {
            market_id,
            prices,
            fills: Vec::new(),
            welfare: 0,
            unfilled: Vec::new(),
        }
    }

    /// Check if prices are properly normalized (sum to 1.0).
    pub fn is_normalized(&self) -> bool {
        let sum: Nanos = self.prices.iter().sum();
        // Allow small rounding error (within 1 nano)
        let diff = if sum > NANOS_PER_DOLLAR as Nanos {
            sum - NANOS_PER_DOLLAR as Nanos
        } else {
            NANOS_PER_DOLLAR as Nanos - sum
        };
        diff <= 1
    }

    /// Normalize prices to sum to 1.0.
    pub fn normalize_prices(&mut self) {
        let sum: Nanos = self.prices.iter().sum();
        if sum == 0 || sum == NANOS_PER_DOLLAR as Nanos {
            return;
        }

        // Scale all prices proportionally
        for price in &mut self.prices {
            *price = (*price as u128 * NANOS_PER_DOLLAR as u128 / sum as u128) as Nanos;
        }

        // Adjust last price to ensure exact sum
        let new_sum: Nanos = self.prices.iter().sum();
        if let Some(last) = self.prices.last_mut() {
            if new_sum < NANOS_PER_DOLLAR as Nanos {
                *last += NANOS_PER_DOLLAR as Nanos - new_sum;
            } else if new_sum > NANOS_PER_DOLLAR as Nanos {
                *last = last.saturating_sub(new_sum - NANOS_PER_DOLLAR as Nanos);
            }
        }
    }
}

/// Per-market clearing solver.
///
/// Solves a single market by matching buy and sell orders at a clearing price.
/// Each outcome is cleared independently.
pub struct LocalSolver;

impl LocalSolver {
    /// Create a new local solver.
    pub fn new() -> Self {
        Self
    }

    /// Solve a single market.
    ///
    /// For binary markets (2 outcomes), uses unified clearing where NO buyers
    /// are treated as YES sellers and vice versa. This ensures YES + NO = $1
    /// automatically — correct market mechanics, not price normalization.
    ///
    /// For non-binary markets, falls back to per-outcome clearing.
    pub fn solve_market(
        &self,
        market_id: MarketId,
        markets: &MarketSet,
        orders: &[Order],
    ) -> MarketSolution {
        let num_outcomes = markets.num_outcomes(market_id) as usize;

        // Filter to single-market orders for this market
        let market_orders: Vec<&Order> = orders
            .iter()
            .filter(|o| o.num_markets == 1 && o.markets[0] == market_id)
            .collect();

        if market_orders.is_empty() {
            return MarketSolution::empty(market_id, num_outcomes);
        }

        if num_outcomes == 2 {
            // Binary market: unified clearing ensures YES + NO = $1
            self.solve_binary_market_unified(market_id, &market_orders)
        } else {
            // All markets in our architecture are binary (multi-outcome events
            // are represented as a MarketGroup of binary markets). This fallback
            // exists only for safety — it should never be reached.
            self.solve_per_outcome(market_id, num_outcomes, &market_orders)
        }
    }

    /// Per-outcome clearing fallback for non-binary markets.
    fn solve_per_outcome(
        &self,
        market_id: MarketId,
        num_outcomes: usize,
        market_orders: &[&Order],
    ) -> MarketSolution {
        let mut prices = vec![0u64; num_outcomes];
        let mut all_fills = Vec::new();
        let mut total_welfare = 0i64;
        let mut all_unfilled = Vec::new();

        for outcome in 0..num_outcomes as u8 {
            let (price, fills, welfare, unfilled) =
                self.solve_outcome(market_id, outcome, market_orders);
            prices[outcome as usize] = price;
            all_fills.extend(fills);
            total_welfare += welfare;
            all_unfilled.extend(unfilled);
        }

        // Deduplicate unfilled: an order is truly unfilled only if it has no fills at all
        let filled_ids: std::collections::HashSet<u64> =
            all_fills.iter().map(|f| f.order_id).collect();
        all_unfilled.sort();
        all_unfilled.dedup();
        all_unfilled.retain(|id| !filled_ids.contains(id));

        MarketSolution {
            market_id,
            prices,
            fills: all_fills,
            welfare: total_welfare,
            unfilled: all_unfilled,
        }
    }

    /// Unified binary market clearing.
    ///
    /// In a binary market, buying NO at price Q is economically equivalent to
    /// selling YES at price ($1 - Q). This method merges both sides into a
    /// single YES supply/demand model:
    ///
    /// - YES demand: direct YES buyers + converted NO sellers
    /// - YES supply: liquidity book + direct YES sellers + converted NO buyers
    ///
    /// A single clearing price P_YES is found; P_NO = $1 - P_YES automatically.
    /// This is not normalization — it's correct market mechanics.
    fn solve_binary_market_unified(
        &self,
        market_id: MarketId,
        orders: &[&Order],
    ) -> MarketSolution {
        self.solve_binary_market_with_extra(market_id, orders, 0, 0)
    }

    /// Solve a binary market with extra demand/supply injected into the curves.
    ///
    /// This is the core method for unified binary clearing. Both the standard
    /// `solve_binary_market_unified` and `solve_market_with_extra_demand` call this.
    ///
    /// `extra_unified_demand` adds demand at price=$1 (highest priority YES buyer).
    /// `extra_unified_supply` adds supply at price=$0 (cheapest YES supplier).
    ///
    /// The extra demand/supply reserves capacity in the matched quantity —
    /// real orders fill only the remainder. The returned MarketSolution contains
    /// only real-order fills at the new clearing price.
    fn solve_binary_market_with_extra(
        &self,
        market_id: MarketId,
        orders: &[&Order],
        extra_unified_demand: Qty,
        extra_unified_supply: Qty,
    ) -> MarketSolution {
        // Classify orders by outcome exposure
        let mut yes_buyers: Vec<(&Order, Qty)> = Vec::new(); // payoff[0] > 0
        let mut no_buyers: Vec<(&Order, Qty)> = Vec::new();  // payoff[1] > 0
        let mut yes_sellers: Vec<(&Order, Qty)> = Vec::new(); // payoff[0] < 0
        let mut no_sellers: Vec<(&Order, Qty)> = Vec::new();  // payoff[1] < 0

        for &order in orders {
            if order.payoffs[0] > 0 {
                yes_buyers.push((order, order.max_fill));
            }
            if order.payoffs[1] > 0 {
                no_buyers.push((order, order.max_fill));
            }
            if order.payoffs[0] < 0 {
                yes_sellers.push((order, order.max_fill));
            }
            if order.payoffs[1] < 0 {
                no_sellers.push((order, order.max_fill));
            }
        }

        // Build unified YES demand curve (price, qty) sorted by price desc.
        // YES demand = direct YES buyers + NO sellers (selling NO ≡ buying YES at $1-limit)
        let mut demand_points: Vec<(Nanos, Qty)> = yes_buyers
            .iter()
            .map(|(o, q)| (o.limit_price, *q))
            .collect();
        for (order, qty) in &no_sellers {
            let converted_limit = NANOS_PER_DOLLAR.saturating_sub(order.limit_price);
            demand_points.push((converted_limit, *qty));
        }
        // Inject extra demand at price=$1 (highest priority)
        if extra_unified_demand > 0 {
            demand_points.push((NANOS_PER_DOLLAR, extra_unified_demand));
        }
        demand_points.sort_by(|a, b| b.0.cmp(&a.0));

        // Build unified YES supply curve (price, qty) sorted by price asc.
        // YES supply = direct YES sellers + NO buyers (buying NO ≡ selling YES at $1-limit)
        let mut supply_points: Vec<(Nanos, Qty)> = Vec::new();
        for (order, qty) in &yes_sellers {
            supply_points.push((order.limit_price, *qty));
        }
        for (order, qty) in &no_buyers {
            let converted_limit = NANOS_PER_DOLLAR.saturating_sub(order.limit_price);
            supply_points.push((converted_limit, *qty));
        }
        // Inject extra supply at price=$0 (cheapest)
        if extra_unified_supply > 0 {
            supply_points.push((0, extra_unified_supply));
        }
        supply_points.sort_by_key(|(price, _)| *price);

        // Find clearing price via supply-demand crossing
        let (clearing_price_yes, matched_qty) =
            Self::find_crossing(&demand_points, &supply_points);
        let clearing_price_no = NANOS_PER_DOLLAR.saturating_sub(clearing_price_yes);

        // Reserve capacity for extra demand/supply — real orders fill only the remainder.
        let demand_real_capacity = matched_qty.saturating_sub(extra_unified_demand);
        let supply_real_capacity = matched_qty.saturating_sub(extra_unified_supply);

        // Generate fills
        let mut fills = Vec::new();
        let mut welfare = 0i64;
        let mut unfilled = Vec::new();

        // Fill YES buyers at P_YES (most aggressive first)
        let mut yes_buyers_sorted = yes_buyers.clone();
        yes_buyers_sorted.sort_by(|a, b| b.0.limit_price.cmp(&a.0.limit_price));

        let mut demand_remaining = demand_real_capacity;
        for (order, max_qty) in &yes_buyers_sorted {
            if demand_remaining == 0 || order.limit_price < clearing_price_yes {
                unfilled.push(order.id);
                continue;
            }
            let fill_qty = (*max_qty).min(demand_remaining);
            if fill_qty >= order.min_fill {
                welfare += (order.limit_price as i64 - clearing_price_yes as i64) * fill_qty as i64;
                fills.push(Fill {
                    order_id: order.id,
                    fill_qty,
                    fill_price: clearing_price_yes,
                });
                demand_remaining = demand_remaining.saturating_sub(fill_qty);
            } else {
                unfilled.push(order.id);
            }
        }

        // Fill converted NO sellers (selling NO ≡ buying YES) at P_YES
        let mut no_sellers_sorted = no_sellers.clone();
        no_sellers_sorted.sort_by(|a, b| {
            // Sort by converted YES limit descending (most aggressive YES buyer first)
            let a_conv = NANOS_PER_DOLLAR.saturating_sub(a.0.limit_price);
            let b_conv = NANOS_PER_DOLLAR.saturating_sub(b.0.limit_price);
            b_conv.cmp(&a_conv)
        });
        for (order, max_qty) in &no_sellers_sorted {
            if demand_remaining == 0 {
                unfilled.push(order.id);
                continue;
            }
            let converted_limit = NANOS_PER_DOLLAR.saturating_sub(order.limit_price);
            if converted_limit < clearing_price_yes {
                unfilled.push(order.id);
                continue;
            }
            let fill_qty = (*max_qty).min(demand_remaining);
            if fill_qty >= order.min_fill {
                // NO seller welfare: clearing_NO - limit_NO (they receive more than their minimum)
                welfare += (clearing_price_no as i64 - order.limit_price as i64) * fill_qty as i64;
                fills.push(Fill {
                    order_id: order.id,
                    fill_qty,
                    fill_price: clearing_price_no,
                });
                demand_remaining = demand_remaining.saturating_sub(fill_qty);
            } else {
                unfilled.push(order.id);
            }
        }

        // Fill supply side: track how much supply comes from each source
        let mut supply_remaining = supply_real_capacity;

        // Direct YES sellers
        let mut yes_sellers_sorted = yes_sellers.clone();
        yes_sellers_sorted.sort_by_key(|(o, _)| o.limit_price);
        for (order, max_qty) in &yes_sellers_sorted {
            if supply_remaining == 0 {
                unfilled.push(order.id);
                continue;
            }
            if order.limit_price > clearing_price_yes {
                unfilled.push(order.id);
                continue;
            }
            let fill_qty = (*max_qty).min(supply_remaining);
            if fill_qty >= order.min_fill {
                welfare += (clearing_price_yes as i64 - order.limit_price as i64) * fill_qty as i64;
                fills.push(Fill {
                    order_id: order.id,
                    fill_qty,
                    fill_price: clearing_price_yes,
                });
                supply_remaining = supply_remaining.saturating_sub(fill_qty);
            } else {
                unfilled.push(order.id);
            }
        }

        // NO buyers acting as YES supply (buying NO ≡ selling YES at $1-limit)
        // Sort by converted YES price ascending (cheapest supply first)
        let mut no_buyers_sorted = no_buyers.clone();
        no_buyers_sorted.sort_by(|a, b| b.0.limit_price.cmp(&a.0.limit_price));
        for (order, max_qty) in &no_buyers_sorted {
            if supply_remaining == 0 {
                unfilled.push(order.id);
                continue;
            }
            // NO buyer willing if P_NO <= their NO limit, i.e., clearing_price_no <= limit
            if order.limit_price < clearing_price_no {
                unfilled.push(order.id);
                continue;
            }
            let fill_qty = (*max_qty).min(supply_remaining);
            if fill_qty >= order.min_fill {
                // NO buyer welfare: limit - P_NO
                welfare += (order.limit_price as i64 - clearing_price_no as i64) * fill_qty as i64;
                fills.push(Fill {
                    order_id: order.id,
                    fill_qty,
                    fill_price: clearing_price_no,
                });
                supply_remaining = supply_remaining.saturating_sub(fill_qty);
            } else {
                unfilled.push(order.id);
            }
        }

        // Deduplicate unfilled
        let filled_ids: std::collections::HashSet<u64> =
            fills.iter().map(|f| f.order_id).collect();
        unfilled.sort();
        unfilled.dedup();
        unfilled.retain(|id| !filled_ids.contains(id));

        MarketSolution {
            market_id,
            prices: vec![clearing_price_yes, clearing_price_no],
            fills,
            welfare,
            unfilled,
        }
    }

    /// Solve a single market with extra demand/supply from bundle orders.
    ///
    /// Instead of creating synthetic Order objects, this method injects bundle
    /// leg demand as numbers directly into the supply/demand curves. The returned
    /// MarketSolution contains only real-order fills at the new clearing price.
    ///
    /// # Outcome-to-unified mapping (used by caller):
    /// - Buy outcome 0 (YES) -> extra_unified_demand
    /// - Buy outcome 1 (NO)  -> extra_unified_supply (buying NO = supplying YES)
    /// - Sell outcome 0 (YES) -> extra_unified_supply
    /// - Sell outcome 1 (NO)  -> extra_unified_demand (selling NO = demanding YES)
    pub fn solve_market_with_extra_demand(
        &self,
        market_id: MarketId,
        markets: &MarketSet,
        orders: &[Order],
        extra_unified_demand: Qty,
        extra_unified_supply: Qty,
    ) -> Option<MarketSolution> {
        let num_outcomes = markets.num_outcomes(market_id) as usize;
        if num_outcomes != 2 {
            return None; // Only binary markets supported
        }

        // Filter to single-market orders for this market
        let market_orders: Vec<&Order> = orders
            .iter()
            .filter(|o| o.num_markets == 1 && o.markets[0] == market_id)
            .collect();

        if market_orders.is_empty() && extra_unified_demand == 0 && extra_unified_supply == 0 {
            return Some(MarketSolution::empty(market_id, num_outcomes));
        }

        let solution = self.solve_binary_market_with_extra(
            market_id,
            &market_orders,
            extra_unified_demand,
            extra_unified_supply,
        );

        Some(solution)
    }

    /// Find supply-demand crossing point.
    ///
    /// Returns (clearing_price, matched_quantity).
    fn find_crossing(
        demand: &[(Nanos, Qty)],
        supply: &[(Nanos, Qty)],
    ) -> (Nanos, Qty) {
        if demand.is_empty() || supply.is_empty() {
            return (NANOS_PER_DOLLAR / 2, 0);
        }

        // Build cumulative supply curve
        let mut cumulative_supply: Vec<(Nanos, Qty)> = Vec::new();
        let mut cum_qty: Qty = 0;
        for &(price, qty) in supply {
            cum_qty += qty;
            cumulative_supply.push((price, cum_qty));
        }

        // Build cumulative demand curve (sorted by price desc)
        let mut cumulative_demand: Vec<(Nanos, Qty)> = Vec::new();
        let mut cum_qty: Qty = 0;
        for &(price, qty) in demand {
            cum_qty += qty;
            cumulative_demand.push((price, cum_qty));
        }

        // Find the supply price that maximizes matched volume
        let mut best_price = supply[0].0;
        let mut best_qty: Qty = 0;

        for &(price, cum_supply) in &cumulative_supply {
            // Demand at this price = total demand from buyers willing to pay >= price
            let demand_at_price = cumulative_demand
                .iter()
                .filter(|(p, _)| *p >= price)
                .map(|(_, q)| *q)
                .max()
                .unwrap_or(0);

            let matched = demand_at_price.min(cum_supply);
            if matched > best_qty {
                best_qty = matched;
                best_price = price;
            }
        }

        (best_price, best_qty)
    }

    /// Solve for a single outcome within a market.
    ///
    /// Returns (clearing_price, fills, welfare, unfilled_order_ids).
    fn solve_outcome(
        &self,
        market_id: MarketId,
        outcome: u8,
        orders: &[&Order],
    ) -> (Nanos, Vec<Fill>, i64, Vec<u64>) {
        // Separate buyers and sellers for this outcome
        let mut buyers: Vec<(&Order, Qty)> = Vec::new();
        let mut sellers: Vec<(&Order, Qty)> = Vec::new();

        for order in orders {
            // Determine if this order is buying or selling this outcome
            // by looking at the payoff for the single-outcome state
            let payoff = order.payoffs[outcome as usize];

            if payoff > 0 {
                // Buying this outcome (positive payoff)
                buyers.push((order, order.max_fill));
            } else if payoff < 0 {
                // Selling this outcome (negative payoff)
                sellers.push((order, order.max_fill));
            }
            // payoff == 0 means order doesn't care about this outcome
        }

        // Sort buyers by limit price descending (most aggressive first)
        buyers.sort_by(|a, b| b.0.limit_price.cmp(&a.0.limit_price));

        // Sort sellers by limit price ascending (most aggressive first)
        sellers.sort_by(|a, b| a.0.limit_price.cmp(&b.0.limit_price));

        // Find clearing price by matching supply and demand
        let (clearing_price, matched_qty) =
            self.find_clearing_price(&buyers, &sellers, market_id, outcome);

        // Generate fills at clearing price
        let mut fills = Vec::new();
        let mut welfare: i64 = 0;
        let mut unfilled = Vec::new();
        let mut remaining = matched_qty;

        // Fill buyers
        for (order, max_qty) in &buyers {
            if remaining == 0 {
                unfilled.push(order.id);
                continue;
            }

            let fill_qty = (*max_qty).min(remaining);
            if fill_qty >= order.min_fill {
                let fill = Fill {
                    order_id: order.id,
                    fill_qty,
                    fill_price: clearing_price,
                };

                // Welfare = limit_price - clearing_price for buyers
                welfare += (order.limit_price as i64 - clearing_price as i64) * fill_qty as i64;

                fills.push(fill);
                remaining = remaining.saturating_sub(fill_qty);
            } else {
                unfilled.push(order.id);
            }
        }

        // Fill sellers - they provide supply that buyers consume
        // Sellers are filled if clearing_price >= their limit (they receive at least what they asked)
        let mut seller_remaining = matched_qty;
        for (order, max_qty) in &sellers {
            if seller_remaining == 0 {
                unfilled.push(order.id);
                continue;
            }

            // Seller is willing if clearing_price >= their limit price
            if clearing_price >= order.limit_price {
                let fill_qty = (*max_qty).min(seller_remaining);
                if fill_qty >= order.min_fill {
                    let fill = Fill {
                        order_id: order.id,
                        fill_qty,
                        fill_price: clearing_price,
                    };

                    // Seller welfare = clearing_price - limit_price (they receive more than minimum)
                    welfare += (clearing_price as i64 - order.limit_price as i64) * fill_qty as i64;

                    fills.push(fill);
                    seller_remaining = seller_remaining.saturating_sub(fill_qty);
                } else {
                    unfilled.push(order.id);
                }
            } else {
                unfilled.push(order.id);
            }
        }

        (clearing_price, fills, welfare, unfilled)
    }

    /// Find the clearing price for an outcome.
    ///
    /// Uses a simple supply-demand crossing algorithm.
    /// Supply comes from both the liquidity book AND sell orders.
    fn find_clearing_price(
        &self,
        buyers: &[(&Order, Qty)],
        sellers: &[(&Order, Qty)],
        market_id: MarketId,
        outcome: u8,
    ) -> (Nanos, Qty) {
        // Supply comes from sell orders only
        if sellers.is_empty() || buyers.is_empty() {
            return (NANOS_PER_DOLLAR / 2, 0); // Default to 50 cents if no supply or demand
        }

        // Build cumulative demand curve (price -> total qty demanded at or above price)
        let mut demand_at_price: Vec<(Nanos, Qty)> = Vec::new();
        let mut cumulative_demand: Qty = 0;

        for (order, qty) in buyers {
            cumulative_demand += qty;
            demand_at_price.push((order.limit_price, cumulative_demand));
        }

        // Build cumulative supply curve from sell orders
        let mut supply_points: Vec<(Nanos, Qty)> = Vec::new();

        // Add sell orders - their limit price is the minimum they'll accept
        for (order, qty) in sellers {
            supply_points.push((order.limit_price, *qty));
        }

        // Sort by price ascending (cheapest supply first)
        supply_points.sort_by_key(|(price, _)| *price);

        // Build cumulative supply curve
        let mut supply_at_price: Vec<(Nanos, Qty)> = Vec::new();
        let mut cumulative_supply: Qty = 0;

        for (price, qty) in supply_points {
            cumulative_supply += qty;
            supply_at_price.push((price, cumulative_supply));
        }

        if supply_at_price.is_empty() {
            return (NANOS_PER_DOLLAR / 2, 0);
        }

        // Find crossing point
        let mut clearing_price = supply_at_price[0].0;
        let mut clearing_qty: Qty = 0;

        for (price, supply) in &supply_at_price {
            // Find demand at this price
            let demand = demand_at_price
                .iter()
                .filter(|(p, _)| *p >= *price)
                .map(|(_, q)| *q)
                .max()
                .unwrap_or(0);

            let matched = demand.min(*supply);
            if matched > clearing_qty {
                clearing_qty = matched;
                clearing_price = *price;
            }
        }

        // Log for debugging (in tests)
        let _ = (market_id, outcome); // Suppress unused warnings

        (clearing_price, clearing_qty)
    }
}

impl Default for LocalSolver {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// PrecomputedMarket — fast crossing for repricing trials
// ============================================================================

/// Precomputed market data for fast crossing queries.
///
/// Built once from base orders per market. Supports O(S) clearing price
/// computation with arbitrary extra demand/supply, where S = number of supply
/// points. Avoids per-trial order classification, sorting, and fill generation.
pub(crate) struct PrecomputedMarket {
    /// Demand curve: (price, qty) sorted by price descending.
    demand_points: Vec<(Nanos, Qty)>,
    /// Prefix sums of demand qty: length = demand_points.len() + 1, starts with 0.
    demand_cum_qty: Vec<Qty>,
    /// Supply curve: (price, qty) sorted by price ascending.
    supply_points: Vec<(Nanos, Qty)>,
    /// Prefix sums of supply qty: length = supply_points.len() + 1, starts with 0.
    supply_cum_qty: Vec<Qty>,
    /// For each supply point i, total demand qty from points with price >= supply_points[i].price.
    demand_at_supply_price: Vec<Qty>,
}

impl PrecomputedMarket {
    /// Build precomputed market from single-market orders (binary market only).
    pub fn from_orders(orders: &[Order]) -> Self {
        let mut demand_points: Vec<(Nanos, Qty)> = Vec::new();
        let mut supply_points: Vec<(Nanos, Qty)> = Vec::new();

        for order in orders {
            // YES buyers → unified demand
            if order.payoffs[0] > 0 {
                demand_points.push((order.limit_price, order.max_fill));
            }
            // NO sellers → unified demand (selling NO ≡ buying YES at $1-limit)
            if order.payoffs[1] < 0 {
                let converted = NANOS_PER_DOLLAR.saturating_sub(order.limit_price);
                demand_points.push((converted, order.max_fill));
            }
            // YES sellers → unified supply
            if order.payoffs[0] < 0 {
                supply_points.push((order.limit_price, order.max_fill));
            }
            // NO buyers → unified supply (buying NO ≡ selling YES at $1-limit)
            if order.payoffs[1] > 0 {
                let converted = NANOS_PER_DOLLAR.saturating_sub(order.limit_price);
                supply_points.push((converted, order.max_fill));
            }
        }

        demand_points.sort_by(|a, b| b.0.cmp(&a.0));
        supply_points.sort_by_key(|(p, _)| *p);

        // Build prefix sums
        let demand_cum_qty = {
            let mut v = Vec::with_capacity(demand_points.len() + 1);
            v.push(0u64);
            let mut cum = 0u64;
            for &(_, q) in &demand_points {
                cum += q;
                v.push(cum);
            }
            v
        };
        let supply_cum_qty = {
            let mut v = Vec::with_capacity(supply_points.len() + 1);
            v.push(0u64);
            let mut cum = 0u64;
            for &(_, q) in &supply_points {
                cum += q;
                v.push(cum);
            }
            v
        };

        // Precompute demand at each supply price: for supply point i,
        // demand_at_supply_price[i] = total demand with price >= supply_points[i].price.
        // demand_points is sorted desc, so partition_point(|p| p >= target) gives count.
        let demand_at_supply_price: Vec<Qty> = supply_points
            .iter()
            .map(|&(sp, _)| {
                let j = demand_points.partition_point(|&(dp, _)| dp >= sp);
                demand_cum_qty[j]
            })
            .collect();

        Self {
            demand_points,
            demand_cum_qty,
            supply_points,
            supply_cum_qty,
            demand_at_supply_price,
        }
    }

    /// Find clearing price and matched quantity with extra demand/supply.
    ///
    /// Extra demand at price=$1, extra supply at price=$0.
    /// Returns (clearing_price, matched_qty). O(S) where S = supply points.
    pub fn crossing_with_extras(&self, extra_demand: Qty, extra_supply: Qty) -> (Nanos, Qty) {
        if self.supply_points.is_empty() && extra_supply == 0 {
            return (NANOS_PER_DOLLAR / 2, 0);
        }
        if self.demand_points.is_empty() && extra_demand == 0 {
            return (NANOS_PER_DOLLAR / 2, 0);
        }

        let mut best_price: Nanos = if self.supply_points.is_empty() {
            0
        } else {
            self.supply_points[0].0
        };
        let mut best_qty: Qty = 0;

        // Check each natural supply price point
        for (i, &(price, _)) in self.supply_points.iter().enumerate() {
            let cum_supply = self.supply_cum_qty[i + 1] + extra_supply;
            let demand = self.demand_at_supply_price[i] + extra_demand;
            let matched = cum_supply.min(demand);
            if matched > best_qty {
                best_qty = matched;
                best_price = price;
            }
        }

        // Check virtual supply point at price=0 (from extra_supply)
        if extra_supply > 0
            && (self.supply_points.is_empty() || self.supply_points[0].0 > 0)
        {
            let total_demand =
                self.demand_cum_qty.last().copied().unwrap_or(0) + extra_demand;
            let matched = extra_supply.min(total_demand);
            if matched > best_qty {
                best_qty = matched;
                best_price = 0;
            }
        }

        (best_price, best_qty)
    }

    /// Estimate welfare of real orders at given clearing price.
    ///
    /// Approximates the welfare by filling demand/supply curves greedily.
    /// Ignores min_fill constraints (slight overestimate in rare cases).
    pub fn estimate_welfare(
        &self,
        clearing_price: Nanos,
        matched_qty: Qty,
        extra_demand: Qty,
        extra_supply: Qty,
    ) -> i64 {
        let demand_real_cap = matched_qty.saturating_sub(extra_demand);
        let supply_real_cap = matched_qty.saturating_sub(extra_supply);

        let mut welfare: i64 = 0;

        // Demand side: fill from highest price down, up to capacity
        let mut remaining = demand_real_cap;
        for &(price, qty) in &self.demand_points {
            if remaining == 0 || price < clearing_price {
                break;
            }
            let fill = qty.min(remaining);
            welfare += (price as i64 - clearing_price as i64) * fill as i64;
            remaining -= fill;
        }

        // Supply side: fill from lowest price up, up to capacity
        remaining = supply_real_cap;
        for &(price, qty) in &self.supply_points {
            if remaining == 0 || price > clearing_price {
                break;
            }
            let fill = qty.min(remaining);
            welfare += (clearing_price as i64 - price as i64) * fill as i64;
            remaining -= fill;
        }

        welfare
    }
}

// ============================================================================
// PriceDiscoverer Trait Implementation
// ============================================================================

use crate::traits::{PriceDiscoverer, PriceDiscoveryResult};

impl PriceDiscoverer for LocalSolver {
    fn discover_prices(&self, problem: &matching_engine::Problem) -> PriceDiscoveryResult {
        let mut result = PriceDiscoveryResult::empty();

        for market in problem.markets.iter() {
            let solution = self.solve_market(market.id, &problem.markets, &problem.orders);

            result.add_market_solution(solution);
        }

        result
    }

    fn name(&self) -> &str {
        "LocalSolver"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use matching_engine::{outcome_sell, simple_yes_buy, Problem};

    fn create_test_problem() -> Problem {
        let mut problem = Problem::new("test");
        let market = problem.markets.add_binary("test_market");

        // Add sell orders (replacing liquidity)
        problem.orders.push(outcome_sell(
            &problem.markets,
            100,
            market,
            0,
            500_000_000,
            1000,
        ));
        problem.orders.push(outcome_sell(
            &problem.markets,
            101,
            market,
            1,
            500_000_000,
            1000,
        ));

        // Add some buy orders
        for i in 0..5 {
            problem.orders.push(simple_yes_buy(
                &problem.markets,
                i + 1,
                market,
                (550 + i * 10) as u64 * 1_000_000,
                100,
            ));
        }

        problem
    }

    #[test]
    fn test_local_solver_basic() {
        let problem = create_test_problem();
        let market_id = problem.markets.iter().next().unwrap().id;

        let solver = LocalSolver::new();
        let solution = solver.solve_market(market_id, &problem.markets, &problem.orders);

        assert_eq!(solution.market_id, market_id);
        assert_eq!(solution.prices.len(), 2); // Binary market
    }

    #[test]
    fn test_price_normalization() {
        let mut solution = MarketSolution::empty(MarketId::new(0), 3);
        solution.prices = vec![400_000_000, 400_000_000, 400_000_000]; // 1.2 total

        assert!(!solution.is_normalized());
        solution.normalize_prices();
        assert!(solution.is_normalized());

        let sum: Nanos = solution.prices.iter().sum();
        assert_eq!(sum, NANOS_PER_DOLLAR);
    }

    #[test]
    fn test_empty_market() {
        let mut problem = Problem::new("empty");
        let _market = problem.markets.add_binary("binary");

        let solver = LocalSolver::new();
        let solution = solver.solve_market(_market, &problem.markets, &[]);

        assert_eq!(solution.prices.len(), 2);
        assert!(solution.fills.is_empty());
    }

    /// Validate that fills respect limit prices.
    /// A fill at price P for a buy order with limit L must have P <= L.
    #[test]
    fn test_fills_respect_limit_prices() {
        let problem = create_test_problem();
        let market_id = problem.markets.iter().next().unwrap().id;

        let solver = LocalSolver::new();
        let solution = solver.solve_market(market_id, &problem.markets, &problem.orders);

        // Build order lookup
        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();

        for fill in &solution.fills {
            let order = order_map
                .get(&fill.order_id)
                .expect("Fill for unknown order");
            // For a buy order, fill price must not exceed limit
            if !order.is_seller() {
                assert!(
                    fill.fill_price <= order.limit_price,
                    "Fill price {} exceeds limit {} for order {}",
                    fill.fill_price,
                    order.limit_price,
                    order.id
                );
            }
        }
    }

    /// Binary markets: YES buyers match against YES sellers and NO buyers.
    #[test]
    fn test_binary_market_clearing() {
        let mut problem = Problem::new("binary");
        let market = problem.markets.add_binary("binary");

        // Add sell orders (replacing liquidity)
        problem.orders.push(outcome_sell(
            &problem.markets,
            100,
            market,
            0,
            400_000_000,
            1000,
        )); // YES sell at $0.40

        // Add buy orders
        problem.orders.push(matching_engine::outcome_buy(
            &problem.markets,
            1,
            market,
            0,
            500_000_000,
            100,
        ));

        let solver = LocalSolver::new();
        let solution = solver.solve_market(market, &problem.markets, &problem.orders);

        // Each outcome should have a clearing price
        assert_eq!(solution.prices.len(), 2);
        assert!(solution.prices[0] > 0, "YES price should be positive");
        assert!(!solution.fills.is_empty(), "Should have fills");
    }
}
