#[cfg(any(feature = "lp", feature = "conic", feature = "milp"))]
mod conformance {
    use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

    use matching_engine::{
        compute_fill_settlement, derive_minting, notional_nanos, outcome_buy, outcome_sell,
        shares_to_qty, MarketId, MarketSet, MmConstraint, MmId, MmSide, Nanos, Order, Problem, Qty,
        MAX_ORDER_QTY, NANOS_PER_DOLLAR,
    };
    use matching_solver::{PipelineResult, Solver};
    use proptest::prelude::*;
    use proptest::test_runner::{Config as ProptestConfig, RngSeed, TestCaseError, TestRunner};
    use sybil_verifier::{
        verify_match, verify_orders, verify_settlement, AccountSnapshot, BlockWitness,
        StateSidecarSnapshot, WitnessBlockHeader, WitnessOrder,
    };

    const DEFAULT_CASES: u32 = 64;
    const MINT_ACCOUNT_ID: u64 = u64::MAX;
    const BASE_BUY_LIMIT: Nanos = 600_000_000;
    const BASE_SELL_LIMIT: Nanos = 400_000_000;
    const BLOCK_HEIGHT: u64 = 1;

    #[derive(Clone, Copy, Debug)]
    enum Direction {
        BuyYes,
        BuyNo,
        SellYes,
        SellNo,
    }

    #[derive(Clone, Debug)]
    struct OrderSpec {
        account_id: u64,
        market_idx: u32,
        direction: Direction,
        price: Nanos,
        qty: Qty,
        expires_at_block: Option<u64>,
        is_mm: bool,
    }

    #[derive(Clone, Debug)]
    struct GeneratedCase {
        problem: Problem,
        order_accounts: BTreeMap<u64, u64>,
    }

    #[derive(Clone, Debug, Default)]
    struct AccountState {
        balance: i64,
        positions: BTreeMap<(MarketId, u8), i64>,
    }

    type AccountStates = BTreeMap<u64, AccountState>;
    type SettlementStates = (AccountStates, AccountStates, i64);

    fn arb_price() -> impl Strategy<Value = Nanos> {
        prop_oneof![Just(0), Just(NANOS_PER_DOLLAR), 1..NANOS_PER_DOLLAR,]
    }

    fn arb_qty() -> impl Strategy<Value = Qty> {
        prop_oneof![
            1u64..=5_000,
            (1u64..=1_000).prop_map(shares_to_qty),
            Just(MAX_ORDER_QTY),
        ]
    }

    fn arb_direction() -> impl Strategy<Value = Direction> {
        prop_oneof![
            Just(Direction::BuyYes),
            Just(Direction::BuyNo),
            Just(Direction::SellYes),
            Just(Direction::SellNo),
        ]
    }

    fn arb_expiry() -> impl Strategy<Value = Option<u64>> {
        prop_oneof![Just(None), Just(Some(1)), Just(Some(2)), Just(Some(100))]
    }

    fn arb_order_spec(market_count: u32) -> impl Strategy<Value = OrderSpec> {
        (
            1u64..=6,
            0..market_count,
            arb_direction(),
            arb_price(),
            arb_qty(),
            arb_expiry(),
            any::<bool>(),
        )
            .prop_map(
                |(account_id, market_idx, direction, price, qty, expires_at_block, is_mm)| {
                    OrderSpec {
                        account_id,
                        market_idx,
                        direction,
                        price,
                        qty,
                        expires_at_block,
                        is_mm,
                    }
                },
            )
    }

    fn arb_case() -> impl Strategy<Value = GeneratedCase> {
        (1u32..=4)
            .prop_flat_map(|market_count| {
                (
                    Just(market_count),
                    arb_qty(),
                    0u8..=3,
                    prop::collection::vec(arb_order_spec(market_count), 0..=8),
                )
            })
            .prop_map(|(market_count, base_qty, mm_mode, random_specs)| {
                build_case(market_count, base_qty, mm_mode, random_specs)
            })
    }

    fn build_case(
        market_count: u32,
        base_qty: Qty,
        mm_mode: u8,
        random_specs: Vec<OrderSpec>,
    ) -> GeneratedCase {
        let mut problem = Problem::new(format!(
            "solver_conformance_m{market_count}_extra{}",
            random_specs.len()
        ));
        let markets: Vec<MarketId> = (0..market_count)
            .map(|idx| problem.markets.add_binary(format!("m{idx}")))
            .collect();
        let mut order_accounts = BTreeMap::new();
        let mut mm_orders = Vec::new();
        let mut next_order_id = 1u64;

        let mut push_order = |problem: &mut Problem,
                              order_accounts: &mut BTreeMap<u64, u64>,
                              mm_orders: &mut Vec<(u64, MmSide)>,
                              direction: Direction,
                              market: MarketId,
                              account_id: u64,
                              price: Nanos,
                              qty: Qty,
                              expires_at_block: Option<u64>,
                              is_mm: bool| {
            let order_id = next_order_id;
            next_order_id += 1;
            let mut order = make_order(&problem.markets, order_id, market, direction, price, qty);
            order.expires_at_block = expires_at_block;
            problem.orders.push(order);
            order_accounts.insert(order_id, account_id);
            if is_mm {
                mm_orders.push((order_id, mm_side(direction)));
            }
            order_id
        };

        let base_market = markets[0];
        push_order(
            &mut problem,
            &mut order_accounts,
            &mut mm_orders,
            Direction::BuyYes,
            base_market,
            1,
            BASE_BUY_LIMIT,
            base_qty,
            None,
            false,
        );
        push_order(
            &mut problem,
            &mut order_accounts,
            &mut mm_orders,
            Direction::BuyNo,
            base_market,
            2,
            BASE_BUY_LIMIT,
            base_qty,
            Some(2),
            false,
        );
        push_order(
            &mut problem,
            &mut order_accounts,
            &mut mm_orders,
            Direction::SellYes,
            base_market,
            3,
            BASE_SELL_LIMIT,
            base_qty,
            Some(100),
            mm_mode != 0,
        );
        push_order(
            &mut problem,
            &mut order_accounts,
            &mut mm_orders,
            Direction::SellNo,
            base_market,
            4,
            BASE_SELL_LIMIT,
            base_qty,
            None,
            mm_mode != 0,
        );

        for spec in random_specs {
            let market = markets[spec.market_idx as usize];
            push_order(
                &mut problem,
                &mut order_accounts,
                &mut mm_orders,
                spec.direction,
                market,
                spec.account_id,
                spec.price,
                spec.qty,
                spec.expires_at_block,
                mm_mode != 0 && spec.is_mm,
            );
        }

        if mm_mode != 0 && !mm_orders.is_empty() {
            let full_notional: u64 = mm_orders
                .iter()
                .filter_map(|(order_id, _)| {
                    problem.orders.iter().find(|order| order.id == *order_id)
                })
                .map(|order| notional_nanos(NANOS_PER_DOLLAR, order.max_fill))
                .sum();
            let max_capital = match mm_mode {
                1 => full_notional,
                2 => full_notional / 2,
                _ => 0,
            };
            let mut constraint = MmConstraint::new(MmId::new(1), max_capital);
            for (order_id, side) in mm_orders {
                constraint.add_order(order_id, side);
            }
            problem.mm_constraints.push(constraint);
        }

        GeneratedCase {
            problem,
            order_accounts,
        }
    }

    fn make_order(
        markets: &MarketSet,
        id: u64,
        market: MarketId,
        direction: Direction,
        price: Nanos,
        qty: Qty,
    ) -> Order {
        match direction {
            Direction::BuyYes => outcome_buy(markets, id, market, 0, price, qty),
            Direction::BuyNo => outcome_buy(markets, id, market, 1, price, qty),
            Direction::SellYes => outcome_sell(markets, id, market, 0, price, qty),
            Direction::SellNo => outcome_sell(markets, id, market, 1, price, qty),
        }
    }

    fn mm_side(direction: Direction) -> MmSide {
        match direction {
            Direction::BuyYes => MmSide::BuyYes,
            Direction::BuyNo => MmSide::BuyNo,
            Direction::SellYes => MmSide::SellYes,
            Direction::SellNo => MmSide::SellNo,
        }
    }

    fn proptest_config() -> ProptestConfig {
        let mut config = ProptestConfig::default();
        if std::env::var_os("PROPTEST_CASES").is_none() {
            config.cases = DEFAULT_CASES;
        }
        if std::env::var_os("PROPTEST_RNG_SEED").is_none() {
            config.rng_seed = RngSeed::Fixed(0x5eed_0197);
        }
        config.source_file = Some(file!());
        config
    }

    fn run_solver_conformance(solver: &dyn Solver) {
        let cases = proptest_config().cases;
        let mut runner = TestRunner::new(proptest_config());
        let result = runner.run(&arb_case(), |case| check_solver_case(solver, &case));
        if let Err(error) = result {
            panic!(
                "{} failed solver conformance after {cases} configured cases: {error}",
                solver.name()
            );
        }
        eprintln!(
            "{} solver conformance passed with {cases} configured proptest cases",
            solver.name()
        );
    }

    fn check_solver_case(solver: &dyn Solver, case: &GeneratedCase) -> Result<(), TestCaseError> {
        prop_assert!(
            case.problem.validate().is_ok(),
            "generator produced invalid problem: {:?}",
            case.problem.validate().err()
        );
        assert_order_shapes(&case.problem)?;

        let pipeline = solver.solve(&case.problem);
        prop_assert!(
            !pipeline.result.fills.is_empty(),
            "{} returned no fills for a scenario with guaranteed crossing orders",
            solver.name()
        );
        assert_fill_totals(&pipeline)?;
        assert_fill_limits(&case.problem, &pipeline)?;

        let clearing_prices = pipeline
            .price_discovery
            .as_ref()
            .map(|price_discovery| price_discovery.prices.clone())
            .unwrap_or_default();
        assert_clearing_prices_cover_fills(&case.problem, &pipeline, &clearing_prices)?;

        let (pre_accounts, post_accounts, expected_balance_delta) =
            derive_account_states(case, &pipeline, &clearing_prices)?;
        assert_balance_delta(&pre_accounts, &post_accounts, expected_balance_delta)?;
        assert_position_balance(&post_accounts, &case.problem.markets)?;

        let witness = build_witness(
            case,
            &pipeline,
            clearing_prices,
            snapshots(&pre_accounts),
            snapshots(&post_accounts),
        );

        let match_result = verify_match(&witness, true);
        prop_assert!(
            match_result.valid,
            "{} sybil_verifier::verify_match(strict diagnostics) failed: {}",
            solver.name(),
            format_violations(&match_result.violations)
        );

        let settlement_result = verify_settlement(&witness);
        prop_assert!(
            settlement_result.valid,
            "{} sybil_verifier::verify_settlement failed: {}",
            solver.name(),
            format_violations(&settlement_result.violations)
        );

        let order_result = verify_orders(&witness);
        prop_assert!(
            order_result.valid,
            "{} sybil_verifier::verify_orders failed: {}",
            solver.name(),
            format_violations(&order_result.violations)
        );

        Ok(())
    }

    fn assert_order_shapes(problem: &Problem) -> Result<(), TestCaseError> {
        for order in &problem.orders {
            prop_assert!(
                order.validate_binary_one_hot().is_ok(),
                "order {} violates SYB-181 one-hot shape: {:?}",
                order.id,
                order.validate_binary_one_hot().err()
            );
            prop_assert!(
                order.max_fill <= MAX_ORDER_QTY,
                "order {} max_fill {} exceeds MAX_ORDER_QTY {}",
                order.id,
                order.max_fill,
                MAX_ORDER_QTY
            );
            prop_assert!(
                order
                    .expires_at_block
                    .is_none_or(|height| height >= BLOCK_HEIGHT),
                "order {} expires before the witness block height",
                order.id
            );
        }
        Ok(())
    }

    fn assert_fill_totals(pipeline: &PipelineResult) -> Result<(), TestCaseError> {
        let total_qty: u64 = pipeline.result.fills.iter().map(|fill| fill.fill_qty).sum();
        let orders_filled = pipeline
            .result
            .fills
            .iter()
            .filter(|fill| fill.fill_qty > 0)
            .count();
        prop_assert_eq!(
            pipeline.result.total_quantity_filled,
            total_qty,
            "reported total_quantity_filled disagrees with fills"
        );
        prop_assert_eq!(
            pipeline.result.orders_filled,
            orders_filled,
            "reported orders_filled disagrees with fills"
        );
        Ok(())
    }

    fn assert_fill_limits(
        problem: &Problem,
        pipeline: &PipelineResult,
    ) -> Result<(), TestCaseError> {
        let order_map: HashMap<u64, &Order> = problem
            .orders
            .iter()
            .map(|order| (order.id, order))
            .collect();
        let mut seen = HashSet::new();
        for fill in &pipeline.result.fills {
            prop_assert!(
                seen.insert(fill.order_id),
                "duplicate fill for order {}",
                fill.order_id
            );
            let Some(order) = order_map.get(&fill.order_id) else {
                return Err(TestCaseError::fail(format!(
                    "fill references unknown order {}",
                    fill.order_id
                )));
            };
            prop_assert!(fill.fill_qty > 0, "solver emitted a zero quantity fill");
            prop_assert!(
                fill.fill_qty <= order.max_fill,
                "order {} fill_qty {} exceeds max_fill {}",
                fill.order_id,
                fill.fill_qty,
                order.max_fill
            );
            prop_assert!(
                fill.fill_qty <= MAX_ORDER_QTY,
                "order {} fill_qty {} exceeds MAX_ORDER_QTY {}",
                fill.order_id,
                fill.fill_qty,
                MAX_ORDER_QTY
            );
            prop_assert!(
                fill.fill_price <= NANOS_PER_DOLLAR,
                "order {} fill_price {} exceeds NANOS_PER_DOLLAR",
                fill.order_id,
                fill.fill_price
            );
            let price_ok = if order.is_seller() {
                fill.fill_price >= order.limit_price
            } else {
                fill.fill_price <= order.limit_price
            };
            prop_assert!(
                price_ok,
                "order {} filled outside limit: price={} limit={} seller={}",
                fill.order_id,
                fill.fill_price,
                order.limit_price,
                order.is_seller()
            );
        }
        Ok(())
    }

    fn assert_clearing_prices_cover_fills(
        problem: &Problem,
        pipeline: &PipelineResult,
        clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
    ) -> Result<(), TestCaseError> {
        let order_map: HashMap<u64, &Order> = problem
            .orders
            .iter()
            .map(|order| (order.id, order))
            .collect();
        for prices in clearing_prices.values() {
            prop_assert_eq!(
                prices.len(),
                2,
                "binary market price vector must have two entries"
            );
            prop_assert_eq!(
                prices[0] + prices[1],
                NANOS_PER_DOLLAR,
                "YES/NO clearing prices must sum to one dollar"
            );
        }
        for fill in &pipeline.result.fills {
            let Some(order) = order_map.get(&fill.order_id) else {
                continue;
            };
            let market = order.markets[0];
            prop_assert!(
                clearing_prices.contains_key(&market),
                "filled order {} has no clearing price for market {:?}",
                fill.order_id,
                market
            );
        }
        Ok(())
    }

    fn derive_account_states(
        case: &GeneratedCase,
        pipeline: &PipelineResult,
        clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
    ) -> Result<SettlementStates, TestCaseError> {
        let mut pre_accounts = initial_accounts(case);
        balance_existing_positions_with_mint(&mut pre_accounts, &case.problem.markets);
        assert_position_balance(&pre_accounts, &case.problem.markets)?;

        let mut post_accounts = pre_accounts.clone();
        let order_map: HashMap<u64, &Order> = case
            .problem
            .orders
            .iter()
            .map(|order| (order.id, order))
            .collect();
        let mut expected_balance_delta = 0i64;

        for fill in &pipeline.result.fills {
            let Some(order) = order_map.get(&fill.order_id) else {
                return Err(TestCaseError::fail(format!(
                    "fill references unknown order {}",
                    fill.order_id
                )));
            };
            let account_id = if fill.account_id != 0 {
                fill.account_id
            } else {
                *case.order_accounts.get(&fill.order_id).ok_or_else(|| {
                    TestCaseError::fail(format!("missing account for order {}", fill.order_id))
                })?
            };
            let Some(delta) = compute_fill_settlement(order, fill) else {
                continue;
            };
            let account = post_accounts.entry(account_id).or_default();
            account.balance += delta.balance_delta;
            expected_balance_delta += delta.balance_delta;
            for (market, outcome, qty_delta) in delta.position_deltas {
                *account.positions.entry((market, outcome)).or_insert(0) += qty_delta;
            }
        }

        let totals = market_totals(&post_accounts, &case.problem.markets);
        let mint_adjustments = derive_minting(&totals, clearing_prices);
        for adjustment in &mint_adjustments {
            prop_assert!(
                clearing_prices
                    .get(&adjustment.market_id)
                    .and_then(|prices| prices.get(adjustment.outcome as usize))
                    .is_some(),
                "minting adjustment for market {:?} outcome {} had no clearing price",
                adjustment.market_id,
                adjustment.outcome
            );
        }
        let mint = post_accounts.entry(MINT_ACCOUNT_ID).or_default();
        for adjustment in mint_adjustments {
            *mint
                .positions
                .entry((adjustment.market_id, adjustment.outcome))
                .or_insert(0) += adjustment.position_delta;
            mint.balance += adjustment.balance_delta;
            expected_balance_delta += adjustment.balance_delta;
        }

        Ok((pre_accounts, post_accounts, expected_balance_delta))
    }

    fn initial_accounts(case: &GeneratedCase) -> BTreeMap<u64, AccountState> {
        let mut accounts = BTreeMap::new();
        for account_id in case.order_accounts.values().copied() {
            accounts
                .entry(account_id)
                .or_insert_with(AccountState::default);
        }
        accounts
            .entry(MINT_ACCOUNT_ID)
            .or_insert_with(AccountState::default);

        for order in &case.problem.orders {
            let account_id = case.order_accounts[&order.id];
            let account = accounts.entry(account_id).or_default();
            account.balance +=
                notional_nanos(NANOS_PER_DOLLAR, order.max_fill) as i64 + NANOS_PER_DOLLAR as i64;

            if order.is_seller() {
                for (outcome, payoff) in order
                    .payoffs
                    .iter()
                    .take(order.num_states as usize)
                    .copied()
                    .enumerate()
                {
                    if payoff < 0 {
                        *account
                            .positions
                            .entry((order.markets[0], outcome as u8))
                            .or_insert(0) += (-(payoff as i64)) * order.max_fill as i64;
                    }
                }
            }
        }

        accounts
    }

    fn balance_existing_positions_with_mint(
        accounts: &mut BTreeMap<u64, AccountState>,
        markets: &MarketSet,
    ) {
        for (market, total_yes, total_no) in market_totals(accounts, markets) {
            let diff = total_yes - total_no;
            if diff == 0 {
                continue;
            }
            let mint = accounts.entry(MINT_ACCOUNT_ID).or_default();
            if diff > 0 {
                *mint.positions.entry((market, 0)).or_insert(0) -= diff;
            } else {
                *mint.positions.entry((market, 1)).or_insert(0) += diff;
            }
        }
    }

    fn market_totals(
        accounts: &BTreeMap<u64, AccountState>,
        markets: &MarketSet,
    ) -> Vec<(MarketId, i64, i64)> {
        markets
            .iter()
            .map(|market| {
                let total_yes = accounts
                    .values()
                    .map(|account| account.positions.get(&(market.id, 0)).copied().unwrap_or(0))
                    .sum();
                let total_no = accounts
                    .values()
                    .map(|account| account.positions.get(&(market.id, 1)).copied().unwrap_or(0))
                    .sum();
                (market.id, total_yes, total_no)
            })
            .collect()
    }

    fn assert_balance_delta(
        pre_accounts: &BTreeMap<u64, AccountState>,
        post_accounts: &BTreeMap<u64, AccountState>,
        expected_balance_delta: i64,
    ) -> Result<(), TestCaseError> {
        let pre_total: i64 = pre_accounts.values().map(|account| account.balance).sum();
        let post_total: i64 = post_accounts.values().map(|account| account.balance).sum();
        prop_assert_eq!(
            post_total - pre_total,
            expected_balance_delta,
            "settlement balance delta must equal shared helper derivation"
        );
        Ok(())
    }

    fn assert_position_balance(
        accounts: &BTreeMap<u64, AccountState>,
        markets: &MarketSet,
    ) -> Result<(), TestCaseError> {
        for market in markets.iter() {
            let total_yes: i64 = accounts
                .values()
                .map(|account| account.positions.get(&(market.id, 0)).copied().unwrap_or(0))
                .sum();
            let total_no: i64 = accounts
                .values()
                .map(|account| account.positions.get(&(market.id, 1)).copied().unwrap_or(0))
                .sum();
            prop_assert_eq!(
                total_yes,
                total_no,
                "market {:?} position imbalance after MINT: YES={} NO={}",
                market.id,
                total_yes,
                total_no
            );
        }
        Ok(())
    }

    fn snapshots(accounts: &BTreeMap<u64, AccountState>) -> Vec<AccountSnapshot> {
        accounts
            .iter()
            .map(|(&id, account)| AccountSnapshot {
                id,
                balance: account.balance,
                total_deposited: account.balance.max(0),
                positions: account
                    .positions
                    .iter()
                    .filter_map(|(&(market, outcome), &qty)| {
                        (qty != 0).then_some((market, outcome, qty))
                    })
                    .collect(),
                events_digest: [0; 32],
            })
            .collect()
    }

    fn build_witness(
        case: &GeneratedCase,
        pipeline: &PipelineResult,
        clearing_prices: HashMap<MarketId, Vec<Nanos>>,
        pre_state: Vec<AccountSnapshot>,
        post_state: Vec<AccountSnapshot>,
    ) -> BlockWitness {
        let mm_order_ids: BTreeSet<u64> = case
            .problem
            .mm_constraints
            .iter()
            .flat_map(|constraint| constraint.order_ids.iter().copied())
            .collect();
        let orders = case
            .problem
            .orders
            .iter()
            .map(|order| WitnessOrder {
                order: order.clone(),
                account_id: case.order_accounts[&order.id],
                is_mm: mm_order_ids.contains(&order.id),
            })
            .collect();

        BlockWitness {
            header: WitnessBlockHeader {
                height: BLOCK_HEIGHT,
                parent_hash: [0; 32],
                state_root: [0; 32],
                events_root: [0; 32],
                order_count: case.problem.orders.len() as u32,
                fill_count: pipeline.result.fills.len() as u32,
                timestamp_ms: 0,
            },
            previous_header: None,
            orders,
            rejections: vec![],
            system_events: vec![],
            fills: pipeline.result.fills.clone(),
            clearing_prices,
            total_welfare: pipeline.result.total_welfare,
            minting_cost: pipeline.result.minting_cost,
            mm_constraints: case.problem.mm_constraints.clone(),
            market_groups: case.problem.market_groups.clone(),
            pre_state: pre_state.clone(),
            post_system_state: pre_state,
            post_state,
            state_sidecar: StateSidecarSnapshot::default(),
            resolved_markets: vec![],
        }
    }

    fn format_violations(violations: &[sybil_verifier::Violation]) -> String {
        violations
            .iter()
            .map(|violation| format!("{:?}: {}", violation.kind, violation.details))
            .collect::<Vec<_>>()
            .join("; ")
    }

    #[cfg(feature = "lp")]
    #[test]
    fn lp_solver_conformance() {
        let solver = matching_solver::LpSolver::new();
        run_solver_conformance(&solver);
    }

    #[cfg(feature = "lp")]
    #[test]
    #[ignore = "SYB-197 finding: EG can return no fills on a crossing MM case; see proptest regression seed f76d7bb8408f0a6335e0fe4bee7efc8aba362ac49bef219f716a35052d1c08df"]
    fn eg_solver_conformance() {
        // Minimized finding: one binary market with the base crossing
        // BuyYes/BuyNo/SellYes/SellNo orders at qty=1, plus a tight MM
        // constraint over sell-side orders. EG returns an empty fill set even
        // though the generated scenario has guaranteed positive-surplus crosses.
        let solver = matching_solver::EgSolver::new();
        run_solver_conformance(&solver);
    }

    #[cfg(feature = "lp")]
    #[test]
    #[ignore = "SYB-197 finding: IterLP can exceed an MM budget after integer rounding; see proptest regression seed 005286340a146b8c787d144df98ea580774bd8d476e8b6ec62fb93fd0369c8af"]
    fn iter_lp_solver_conformance() {
        // Minimized finding: one binary market with an MM constraint budget of
        // 500000001000000 nanos. Strict match verification computes MM capital
        // used as 500000001500000 nanos, an overflow of 500000 nanos.
        let solver = matching_solver::IterLpSolver::new();
        run_solver_conformance(&solver);
    }

    #[cfg(feature = "lp")]
    #[test]
    fn decomposed_lp_solver_conformance() {
        // DecomposedSolver is an exported Solver implementation under `lp`.
        // The shared generator intentionally includes multi-market batches and
        // MM constraints so this test covers both independent components and
        // mirror-descent budget coordination.
        let solver = matching_solver::DecomposedSolver::new(matching_solver::LpSolver::new());
        run_solver_conformance(&solver);
    }

    #[cfg(feature = "milp")]
    #[test]
    #[ignore = "SYB-197 finding: MILP fails strict verify_match with WelfareMismatch (negative minting cost - the SYB-167 gross-vs-net welfare discrepancy); minimal case: single-market MM batch"]
    fn milp_solver_conformance() {
        let solver = matching_solver::MilpSolver::with_config(matching_solver::MilpConfig {
            timeout_secs: Some(3.0),
            gap_tolerance: 0.05,
            mm_budget_mode: matching_solver::MmBudgetMode::Exact,
        });
        run_solver_conformance(&solver);
    }

    #[cfg(feature = "conic")]
    #[test]
    #[ignore = "SYB-197 finding: Conic can return no fills on a crossing case; see inline minimized case"]
    fn conic_solver_conformance() {
        // Minimized finding from `cargo test -p matching-solver --features conic`:
        // one binary market, base crossing orders at qty=41000, no MM
        // constraints, plus aggressive same-market YES sell/buy liquidity. Conic
        // returns an empty fill set despite the guaranteed positive-surplus
        // crosses.
        let solver = matching_solver::ConicSolver::with_config(matching_solver::ConicConfig {
            max_iter: 100,
            time_limit: 5.0,
            ..Default::default()
        });
        run_solver_conformance(&solver);
    }
}

#[cfg(not(any(feature = "lp", feature = "conic", feature = "milp")))]
#[test]
fn solver_conformance_no_solver_features_enabled() {
    eprintln!("matching-solver has no default solver feature enabled; no Solver impl is available for conformance");
}
