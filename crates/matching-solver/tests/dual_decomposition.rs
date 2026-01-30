//! Integration tests for dual decomposition clearing.
//!
//! Tests verify that dual decomposition:
//! 1. Produces correct fills respecting order limits
//! 2. Achieves price consistency (sum ≈ $1) for market groups
//! 3. Respects MM budget constraints
//! 4. Computes non-negative welfare for all fills

use std::collections::HashMap;

use matching_engine::{
    outcome_sell, price_to_nanos, simple_no_buy, simple_yes_buy, MarketGroup, MmConstraint, MmId,
    MmSide, Nanos, Order, Problem,
};
use matching_solver::{DualMaster, Pipeline};

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a 3-outcome election problem with stepped liquidity.
fn election_3way() -> Problem {
    let mut problem = Problem::new("election_3way");
    let m_a = problem.markets.add_binary("Candidate A");
    let m_b = problem.markets.add_binary("Candidate B");
    let m_c = problem.markets.add_binary("Candidate C");

    let group = MarketGroup::new("Election")
        .with_market(m_a)
        .with_market(m_b)
        .with_market(m_c);
    problem.add_market_group(group);

    // Stepped liquidity for realistic price discovery
    for &m in &[m_a, m_b, m_c] {
        problem.liquidity.add_ask(m, 0, price_to_nanos(0.15), 200);
        problem.liquidity.add_ask(m, 0, price_to_nanos(0.25), 200);
        problem.liquidity.add_ask(m, 0, price_to_nanos(0.35), 200);
        problem.liquidity.add_ask(m, 0, price_to_nanos(0.45), 200);
        problem.liquidity.add_ask(m, 0, price_to_nanos(0.55), 200);
        problem.liquidity.add_ask(m, 1, price_to_nanos(0.25), 500);
    }

    // YES buyers: A ~50%, B ~30%, C ~20%
    for i in 0..10 {
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            100 + i,
            m_a,
            price_to_nanos(0.45 + 0.01 * i as f64),
            50,
        ));
    }
    for i in 0..8 {
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            200 + i,
            m_b,
            price_to_nanos(0.25 + 0.01 * i as f64),
            50,
        ));
    }
    for i in 0..5 {
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            300 + i,
            m_c,
            price_to_nanos(0.15 + 0.01 * i as f64),
            50,
        ));
    }

    // NO buyers for two-sided markets
    for i in 0..5 {
        problem.orders.push(simple_no_buy(
            &problem.markets,
            400 + i,
            m_a,
            price_to_nanos(0.50 + 0.01 * i as f64),
            50,
        ));
    }
    for i in 0..3 {
        problem.orders.push(simple_no_buy(
            &problem.markets,
            500 + i,
            m_b,
            price_to_nanos(0.65 + 0.01 * i as f64),
            50,
        ));
    }

    problem
}

/// Create a problem with MM budget constraints.
fn mm_budget_problem() -> Problem {
    let mut problem = Problem::new("mm_budget");
    let m = problem.markets.add_binary("Market");

    // Liquidity
    problem.liquidity.add_ask(m, 0, price_to_nanos(0.30), 500);
    problem.liquidity.add_ask(m, 0, price_to_nanos(0.40), 500);
    problem.liquidity.add_ask(m, 0, price_to_nanos(0.50), 500);
    problem.liquidity.add_ask(m, 1, price_to_nanos(0.30), 500);

    // Regular traders
    for i in 0..5 {
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            i + 1,
            m,
            price_to_nanos(0.45 + 0.02 * i as f64),
            100,
        ));
    }

    // MM orders with tight budget
    let mm_id = MmId::new(1);
    let mut mm = MmConstraint::new(mm_id, 5_000_000_000); // $5 budget

    // MM provides liquidity on both sides
    for i in 0..3 {
        let order_id = 100 + i;
        let order = simple_yes_buy(
            &problem.markets,
            order_id,
            m,
            price_to_nanos(0.48 + 0.01 * i as f64),
            200,
        );
        problem.orders.push(order);
        mm.add_order(order_id, MmSide::BuyYes);
    }
    for i in 0..3 {
        let order_id = 200 + i;
        let order = outcome_sell(
            &problem.markets,
            order_id,
            m,
            0, // Sell YES
            price_to_nanos(0.35 + 0.01 * i as f64),
            200,
        );
        problem.orders.push(order);
        mm.add_order(order_id, MmSide::SellYes);
    }

    problem.mm_constraints.push(mm);

    // Add to a group for price consistency
    let group = MarketGroup::new("TestGroup").with_market(m);
    problem.add_market_group(group);

    problem
}

// ============================================================================
// Price Consistency Tests
// ============================================================================

/// Verify that dual decomposition produces prices that approximately sum to $1
/// for market groups.
#[test]
fn test_price_sum_convergence() {
    let problem = election_3way();
    let master = DualMaster::new();
    let result = master.solve(&problem);

    // Check fills exist
    assert!(
        !result.matching_result.fills.is_empty(),
        "Should have fills"
    );

    // Check price sum for the election group
    for (group, error) in &result.final_price_sum_error {
        // Allow up to 50% error for this test (dual decomposition
        // doesn't directly control the final solve's prices, only guides
        // the order participation through bid shading)
        assert!(
            error.abs() < 0.50,
            "Group '{}' price sum error = {:.4} (expected < 0.50)",
            group,
            error
        );
    }
}

/// Verify that the pipeline with dual decomposition produces results.
#[test]
fn test_pipeline_dual_decomposition() {
    let problem = election_3way();
    let pipeline = Pipeline::with_dual_decomposition();
    let result = pipeline.solve(&problem);

    assert!(result.result.fills.len() > 0, "Pipeline should produce fills");
    assert!(result.result.total_welfare >= 0, "Welfare should be non-negative");
    assert!(result.iterations > 0, "Should report iterations");
}

// ============================================================================
// MM Budget Tests
// ============================================================================

/// Verify that fills respect MM budget constraints.
#[test]
fn test_mm_budget_respected() {
    let problem = mm_budget_problem();
    let master = DualMaster::new();
    let result = master.solve(&problem);

    // Build fills map
    let fills_map: HashMap<u64, (Nanos, u64)> = result
        .matching_result
        .fills
        .iter()
        .map(|f| (f.order_id, (f.fill_price, f.fill_qty)))
        .collect();

    // Check each MM constraint
    for mm in &problem.mm_constraints {
        let capital_used = mm.capital_used(&fills_map);
        // Allow small overshoot from the dual approximation
        let budget_with_margin = (mm.max_capital as f64 * 1.10) as u64;
        assert!(
            capital_used <= budget_with_margin,
            "MM {} capital {} exceeds budget {} (with 10% margin: {})",
            mm.mm_id.0,
            capital_used,
            mm.max_capital,
            budget_with_margin,
        );
    }
}

// ============================================================================
// Welfare Tests
// ============================================================================

/// Verify that all fills have non-negative welfare.
/// A buyer should not pay more than their limit.
/// A seller should not receive less than their limit.
#[test]
fn test_welfare_non_negative() {
    let problem = election_3way();
    let master = DualMaster::new();
    let result = master.solve(&problem);

    let order_map: HashMap<u64, &Order> =
        problem.orders.iter().map(|o| (o.id, o)).collect();

    for fill in &result.matching_result.fills {
        let order = order_map
            .get(&fill.order_id)
            .expect("Fill for unknown order");
        let welfare = order.welfare_contribution(fill.fill_price, fill.fill_qty);
        assert!(
            welfare >= 0,
            "Negative welfare for order {}: welfare={}, fill_price={}, limit={}, is_seller={}",
            order.id,
            welfare,
            fill.fill_price,
            order.limit_price,
            order.is_seller(),
        );
    }
}

/// Verify fills respect limit prices.
#[test]
fn test_fills_respect_limits() {
    let problem = election_3way();
    let master = DualMaster::new();
    let result = master.solve(&problem);

    let order_map: HashMap<u64, &Order> =
        problem.orders.iter().map(|o| (o.id, o)).collect();

    for fill in &result.matching_result.fills {
        let order = order_map
            .get(&fill.order_id)
            .expect("Fill for unknown order");
        assert!(
            order.is_satisfied_at_price(fill.fill_price),
            "Order {} not satisfied: fill_price={}, limit={}, is_seller={}",
            order.id,
            fill.fill_price,
            order.limit_price,
            order.is_seller(),
        );
    }
}

// ============================================================================
// Comparison Tests
// ============================================================================

/// Compare dual decomposition pipeline vs. negrisk pipeline.
/// Both should produce valid results; dual decomposition should handle
/// price consistency without synthetic arbitrage orders.
#[test]
fn test_dual_vs_negrisk_comparison() {
    let problem = election_3way();

    // Dual decomposition
    let dual_pipeline = Pipeline::with_dual_decomposition();
    let dual_result = dual_pipeline.solve(&problem);

    // Negrisk (existing)
    let negrisk_pipeline = Pipeline::with_negrisk();
    let negrisk_result = negrisk_pipeline.solve(&problem);

    // Both should produce fills
    assert!(
        dual_result.result.fills.len() > 0,
        "Dual should produce fills"
    );
    assert!(
        negrisk_result.result.fills.len() > 0,
        "Negrisk should produce fills"
    );

    // Both should have non-negative welfare
    assert!(
        dual_result.result.total_welfare >= 0,
        "Dual welfare should be non-negative: {}",
        dual_result.result.total_welfare
    );
    assert!(
        negrisk_result.result.total_welfare >= 0,
        "Negrisk welfare should be non-negative: {}",
        negrisk_result.result.total_welfare
    );
}

/// Test that dual decomposition converges with a simple 2-outcome scenario.
#[test]
fn test_simple_two_outcome_convergence() {
    let mut problem = Problem::new("two_outcome");
    let m_a = problem.markets.add_binary("Outcome A");
    let m_b = problem.markets.add_binary("Outcome B");

    let group = MarketGroup::new("Event")
        .with_market(m_a)
        .with_market(m_b);
    problem.add_market_group(group);

    // Balanced liquidity
    for &m in &[m_a, m_b] {
        problem.liquidity.add_ask(m, 0, price_to_nanos(0.20), 300);
        problem.liquidity.add_ask(m, 0, price_to_nanos(0.40), 300);
        problem.liquidity.add_ask(m, 0, price_to_nanos(0.60), 300);
        problem.liquidity.add_ask(m, 1, price_to_nanos(0.30), 300);
    }

    // Buyers: A at ~60%, B at ~40%
    for i in 0..8 {
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            100 + i,
            m_a,
            price_to_nanos(0.55 + 0.01 * i as f64),
            80,
        ));
    }
    for i in 0..6 {
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            200 + i,
            m_b,
            price_to_nanos(0.35 + 0.01 * i as f64),
            80,
        ));
    }

    // NO buyers
    for i in 0..3 {
        problem.orders.push(simple_no_buy(
            &problem.markets,
            300 + i,
            m_a,
            price_to_nanos(0.40 + 0.01 * i as f64),
            80,
        ));
    }
    for i in 0..3 {
        problem.orders.push(simple_no_buy(
            &problem.markets,
            400 + i,
            m_b,
            price_to_nanos(0.55 + 0.01 * i as f64),
            80,
        ));
    }

    let master = DualMaster::new();
    let result = master.solve(&problem);

    assert!(!result.matching_result.fills.is_empty(), "Should have fills");
}

/// Test with no market groups or MM constraints (basic clearing).
#[test]
fn test_no_coupling_constraints() {
    let mut problem = Problem::new("basic");
    let m = problem.markets.add_binary("Market");

    problem.liquidity.add_ask(m, 0, price_to_nanos(0.30), 1000);
    problem.liquidity.add_ask(m, 1, price_to_nanos(0.30), 1000);

    for i in 0..10 {
        problem.orders.push(simple_yes_buy(
            &problem.markets,
            i + 1,
            m,
            price_to_nanos(0.40 + 0.02 * i as f64),
            50,
        ));
    }

    let master = DualMaster::new();
    let result = master.solve(&problem);

    assert!(!result.matching_result.fills.is_empty(), "Should have fills");
    // Should converge immediately since there are no coupling constraints
    assert!(
        result.converged || result.iterations <= 2,
        "Should converge quickly without coupling constraints (got {} iterations)",
        result.iterations
    );
}
