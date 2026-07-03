//! Shared `Problem` builders and assertion helpers for solver unit tests.
//!
//! The LP-family solvers (LP, EG, IterLP, Conic) each exercised the same core
//! scenarios — single-market crossing, minting, group minting, unprofitable
//! trades, MM budgets — with byte-identical `Problem` builders. Those builders
//! live here so every solver's `#[cfg(test)]` module can reuse them. Genuinely
//! unique scenarios (Conic fisher-mode, IterLP tight-budget/price-shift, budget
//! sweeps) stay next to the solver they characterize.
//!
//! Gated on `feature = "lp"` because every LP-family solver requires it.

use matching_engine::{
    outcome_sell, simple_no_buy, simple_yes_buy, MarketGroup, MmConstraint, MmId, MmSide, Problem,
    NANOS_PER_DOLLAR,
};

use crate::PipelineResult;

/// One binary market: symmetric YES/NO sellers at 50c plus a YES buyer at 60c.
///
/// The buyer crosses the YES seller, producing positive welfare and fills.
pub(crate) fn single_market_problem() -> Problem {
    let mut problem = Problem::new("single_market");
    let market = problem.markets.add_binary("market");

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
    problem.orders.push(simple_yes_buy(
        &problem.markets,
        1,
        market,
        600_000_000,
        100,
    ));
    problem
}

/// YES buyer at 60c + NO buyer at 50c on one market: fills via minting
/// (prices sum to $1.10 > $1, so minting is profitable).
pub(crate) fn minting_problem() -> Problem {
    let mut problem = Problem::new("minting");
    let market = problem.markets.add_binary("market");

    problem.orders.push(simple_yes_buy(
        &problem.markets,
        1,
        market,
        600_000_000,
        100,
    ));
    problem
        .orders
        .push(simple_no_buy(&problem.markets, 2, market, 500_000_000, 100));
    problem
}

/// Three markets in one group with YES buyers at 40c/35c/30c (sum > $1):
/// fills via group minting (negrisk).
pub(crate) fn group_minting_problem() -> Problem {
    let mut problem = Problem::new("group_mint");
    let m0 = problem.markets.add_binary("A");
    let m1 = problem.markets.add_binary("B");
    let m2 = problem.markets.add_binary("C");

    let mut group = MarketGroup::new("Election");
    group.add_market(m0);
    group.add_market(m1);
    group.add_market(m2);
    problem.add_market_group(group);

    problem
        .orders
        .push(simple_yes_buy(&problem.markets, 1, m0, 400_000_000, 100));
    problem
        .orders
        .push(simple_yes_buy(&problem.markets, 2, m1, 350_000_000, 100));
    problem
        .orders
        .push(simple_yes_buy(&problem.markets, 3, m2, 300_000_000, 100));
    problem
}

/// YES buyer at 30c + NO buyer at 30c: sum = 60c < $1, so minting is
/// unprofitable and nothing should fill.
pub(crate) fn no_profitable_trades_problem() -> Problem {
    let mut problem = Problem::new("no_profit");
    let market = problem.markets.add_binary("market");

    problem.orders.push(simple_yes_buy(
        &problem.markets,
        1,
        market,
        300_000_000,
        100,
    ));
    problem
        .orders
        .push(simple_no_buy(&problem.markets, 2, market, 300_000_000, 100));
    problem
}

/// YES buyer at 60c (500) paired via minting with an MM buying NO at 50c
/// (1000) under a $50 budget. Order id `200` is the MM (`BuyNo`).
pub(crate) fn mm_budget_problem() -> Problem {
    let mut problem = Problem::new("mm_budget");
    let market = problem.markets.add_binary("market");

    problem.orders.push(simple_yes_buy(
        &problem.markets,
        1,
        market,
        600_000_000,
        500,
    ));

    let mm_order = simple_no_buy(&problem.markets, 200, market, 500_000_000, 1000);
    problem.orders.push(mm_order);

    let mut mm = MmConstraint::new(MmId(1), 50 * NANOS_PER_DOLLAR);
    mm.add_order(200, MmSide::BuyNo);
    problem.mm_constraints.push(mm);
    problem
}

/// A retail YES/NO minting pair plus a zero-budget MM (order id `200`,
/// `BuyNo`) that must not be filled.
pub(crate) fn zero_budget_mm_problem() -> Problem {
    let mut problem = Problem::new("zero_budget");
    let market = problem.markets.add_binary("market");

    problem.orders.push(simple_yes_buy(
        &problem.markets,
        1,
        market,
        600_000_000,
        100,
    ));
    problem.orders.push(simple_no_buy(
        &problem.markets,
        100,
        market,
        500_000_000,
        100,
    ));

    let mm_order = simple_no_buy(&problem.markets, 200, market, 500_000_000, 1000);
    problem.orders.push(mm_order);

    let mut mm = MmConstraint::new(MmId(1), 0);
    mm.add_order(200, MmSide::BuyNo);
    problem.mm_constraints.push(mm);
    problem
}

/// Two YES buyers (60c/55c) and two competing MMs buying NO — MM `200`
/// at 45c budget $100, MM `300` at 50c budget $50.
pub(crate) fn multiple_mms_problem() -> Problem {
    let mut problem = Problem::new("multi_mm");
    let market = problem.markets.add_binary("market");

    problem.orders.push(simple_yes_buy(
        &problem.markets,
        1,
        market,
        600_000_000,
        1000,
    ));
    problem.orders.push(simple_yes_buy(
        &problem.markets,
        2,
        market,
        550_000_000,
        1000,
    ));

    let mm1_order = simple_no_buy(&problem.markets, 200, market, 450_000_000, 2000);
    problem.orders.push(mm1_order);
    let mut mm1 = MmConstraint::new(MmId(1), 100 * NANOS_PER_DOLLAR);
    mm1.add_order(200, MmSide::BuyNo);
    problem.mm_constraints.push(mm1);

    let mm2_order = simple_no_buy(&problem.markets, 300, market, 500_000_000, 2000);
    problem.orders.push(mm2_order);
    let mut mm2 = MmConstraint::new(MmId(2), 50 * NANOS_PER_DOLLAR);
    mm2.add_order(300, MmSide::BuyNo);
    problem.mm_constraints.push(mm2);
    problem
}

/// Assert a `BuyNo` MM's capital usage stays within `budget_dollars` (with the
/// 1% rounding tolerance every solver's MM-budget test uses). A missing fill
/// (MM not filled at all) trivially satisfies the budget.
pub(crate) fn assert_buy_no_within_budget(
    result: &PipelineResult,
    order_id: u64,
    budget_dollars: u64,
) {
    if let Some(fill) = result.result.fills.iter().find(|f| f.order_id == order_id) {
        let capital = MmSide::BuyNo.capital_needed(fill.fill_price, fill.fill_qty);
        let budget = budget_dollars * NANOS_PER_DOLLAR;
        assert!(
            capital <= budget + NANOS_PER_DOLLAR / 100,
            "MM capital {} should not exceed budget {}",
            capital,
            budget
        );
    }
}

/// Assert a zero-budget MM order got no fill (absent or zero quantity).
pub(crate) fn assert_mm_not_filled(result: &PipelineResult, order_id: u64) {
    let mm_fill = result.result.fills.iter().find(|f| f.order_id == order_id);
    assert!(
        mm_fill.is_none() || mm_fill.unwrap().fill_qty == 0,
        "zero-budget MM should not be filled"
    );
}
