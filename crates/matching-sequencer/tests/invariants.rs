//! Property-based and metamorphic tests for BlockSequencer invariants.
//!
//! These tests hard-assert the economic invariants that the sequencer currently
//! only checks with `eprintln` warnings (sequencer.rs:650-715).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use matching_engine::{
    compute_fill_settlement, derive_minting, net_welfare, outcome_buy, signed_notional_nanos,
    MarketId, MarketSet, NANOS_PER_DOLLAR,
};
use matching_sequencer::{AccountId, AccountStore, AdminOracle, BlockSequencer, OrderSubmission};
use proptest::prelude::*;
use sybil_verifier::BlockWitness;

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Generate a crossing pair: BuyYes at `yes_price`, BuyNo at `1 - yes_price + surplus`.
/// This guarantees the orders can match (yes_price + no_price >= $1).
fn arb_crossing_pair(
    n_accounts: u64,
    n_markets: u32,
    markets: MarketSet,
) -> impl Strategy<Value = Vec<OrderSubmission>> {
    let ms = markets.clone();
    (
        0..n_markets,                                     // market
        NANOS_PER_DOLLAR / 10..9 * NANOS_PER_DOLLAR / 10, // yes_price (10%-90%)
        NANOS_PER_DOLLAR / 100..NANOS_PER_DOLLAR / 5, // 1-20% surplus guarantees positive welfare
        1u64..50,                                     // quantity
        (0..n_accounts, 0..n_accounts).prop_filter("distinct accounts", |(a, b)| a != b),
    )
        .prop_map(move |(market_idx, yes_price, surplus, qty, (a1, a2))| {
            let mid = MarketId::new(market_idx);
            // NO price = complement + surplus => guaranteed crossing
            let no_price = (NANOS_PER_DOLLAR - yes_price + surplus).min(NANOS_PER_DOLLAR);
            let buyer_yes = OrderSubmission {
                account_id: AccountId(a1),
                orders: vec![outcome_buy(&ms, 0, mid, 0, yes_price, qty)],
                mm_constraint: None,
            };
            let buyer_no = OrderSubmission {
                account_id: AccountId(a2),
                orders: vec![outcome_buy(&ms, 0, mid, 1, no_price, qty)],
                mm_constraint: None,
            };
            vec![buyer_yes, buyer_no]
        })
}

/// Generate a batch of 1-4 crossing pairs (guaranteed trades) plus 0-4 random solo orders.
fn arb_trading_batch(
    n_accounts: u64,
    n_markets: u32,
    markets: MarketSet,
) -> impl Strategy<Value = Vec<OrderSubmission>> {
    let ms1 = markets.clone();
    let ms2 = markets.clone();
    (
        prop::collection::vec(arb_crossing_pair(n_accounts, n_markets, ms1), 1..=4),
        prop::collection::vec(arb_solo_order(n_accounts, n_markets, ms2), 0..=4),
    )
        .prop_map(|(pairs, solos)| {
            let mut all: Vec<OrderSubmission> = pairs.into_iter().flatten().collect();
            all.extend(solos);
            all
        })
}

/// Generate a single random buy order (may or may not cross with anything).
fn arb_solo_order(
    n_accounts: u64,
    n_markets: u32,
    markets: MarketSet,
) -> impl Strategy<Value = OrderSubmission> {
    let ms = markets.clone();
    (
        0..n_accounts,
        0..n_markets,
        1..NANOS_PER_DOLLAR,
        1u64..50,
        0u8..2,
    )
        .prop_map(move |(acct, market_idx, price, qty, outcome)| {
            let mid = MarketId::new(market_idx);
            OrderSubmission {
                account_id: AccountId(acct),
                orders: vec![outcome_buy(&ms, 0, mid, outcome, price, qty)],
                mm_constraint: None,
            }
        })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const N_ACCOUNTS: u64 = 4;
const N_MARKETS: u32 = 3;
const INITIAL_BALANCE: i64 = 1000 * NANOS_PER_DOLLAR as i64;

fn make_markets() -> MarketSet {
    let mut ms = MarketSet::new();
    for i in 0..N_MARKETS {
        ms.add_binary(format!("Market {}", i));
    }
    ms
}

fn make_sequencer() -> (BlockSequencer, MarketSet) {
    let mut accounts = AccountStore::new();
    for _ in 0..N_ACCOUNTS {
        accounts.create_account(INITIAL_BALANCE);
    }
    let markets = make_markets();
    let oracle = Arc::new(AdminOracle::new());
    let seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        oracle,
        matching_sequencer::SequencerConfig::default(),
    );
    (seq, markets)
}

/// Assert position balance: for every market, total YES == total NO.
fn assert_position_balance(seq: &BlockSequencer, markets: &MarketSet) {
    for market in markets.iter() {
        let mut total_yes: i64 = 0;
        let mut total_no: i64 = 0;
        for (_, account) in seq.accounts.iter() {
            total_yes += account.position(market.id, 0);
            total_no += account.position(market.id, 1);
        }
        assert_eq!(
            total_yes, total_no,
            "Position imbalance in market {:?}: YES={} NO={}",
            market.id, total_yes, total_no
        );
    }
}

fn expected_resolution_delta(seq: &BlockSequencer, market: MarketId, yes_payout_nanos: u64) -> i64 {
    let no_payout_nanos = NANOS_PER_DOLLAR - yes_payout_nanos;
    seq.accounts
        .iter()
        .map(|(_, account)| {
            signed_notional_nanos(yes_payout_nanos, account.position(market, 0))
                + signed_notional_nanos(no_payout_nanos, account.position(market, 1))
        })
        .sum()
}

fn recompute_total_welfare(witness: &BlockWitness) -> i64 {
    let order_map: HashMap<u64, _> = witness
        .orders
        .iter()
        .map(|witness_order| (witness_order.order.id, &witness_order.order))
        .collect();

    let gross_welfare = witness
        .fills
        .iter()
        .filter_map(|fill| {
            order_map
                .get(&fill.order_id)
                .map(|order| order.gross_welfare_contribution(fill.fill_qty))
        })
        .sum();
    net_welfare(gross_welfare, witness.minting_cost)
}

fn raw_welfare_numerator(witness: &BlockWitness) -> i128 {
    let order_map: HashMap<u64, _> = witness
        .orders
        .iter()
        .map(|witness_order| (witness_order.order.id, &witness_order.order))
        .collect();

    witness
        .fills
        .iter()
        .filter_map(|fill| {
            order_map.get(&fill.order_id).map(|order| {
                let surplus_per_unit = if order.is_seller() {
                    fill.fill_price as i128 - order.limit_price as i128
                } else {
                    order.limit_price as i128 - fill.fill_price as i128
                };
                surplus_per_unit * fill.fill_qty as i128
            })
        })
        .sum()
}

fn expected_block_balance_delta(witness: &BlockWitness) -> i64 {
    let order_map: HashMap<u64, _> = witness
        .orders
        .iter()
        .map(|witness_order| (witness_order.order.id, &witness_order.order))
        .collect();
    let order_account: HashMap<u64, u64> = witness
        .orders
        .iter()
        .map(|witness_order| (witness_order.order.id, witness_order.account_id))
        .collect();
    let mut positions_by_account: HashMap<u64, HashMap<(MarketId, u8), i64>> = witness
        .post_system_state
        .iter()
        .map(|account| {
            (
                account.id,
                account
                    .positions
                    .iter()
                    .map(|&(market, outcome, qty)| ((market, outcome), qty))
                    .collect(),
            )
        })
        .collect();

    let mut balance_delta = 0;
    for fill in &witness.fills {
        let Some(order) = order_map.get(&fill.order_id) else {
            continue;
        };
        let account_id = if fill.account_id != 0 {
            fill.account_id
        } else {
            *order_account
                .get(&fill.order_id)
                .expect("witness fill must reference a witness order")
        };
        let Some(delta) = compute_fill_settlement(order, fill) else {
            continue;
        };

        balance_delta += delta.balance_delta;
        let positions = positions_by_account.entry(account_id).or_default();
        for (market, outcome, qty_delta) in delta.position_deltas {
            *positions.entry((market, outcome)).or_insert(0) += qty_delta;
        }
    }

    let markets: HashSet<MarketId> = positions_by_account
        .values()
        .flat_map(|positions| positions.keys().map(|(market, _)| *market))
        .collect();
    let market_totals: Vec<(MarketId, i64, i64)> = markets
        .into_iter()
        .map(|market| {
            let total_yes = positions_by_account
                .values()
                .map(|positions| positions.get(&(market, 0)).copied().unwrap_or(0))
                .sum();
            let total_no = positions_by_account
                .values()
                .map(|positions| positions.get(&(market, 1)).copied().unwrap_or(0))
                .sum();
            (market, total_yes, total_no)
        })
        .collect();

    let mint_delta: i64 = derive_minting(&market_totals, &witness.clearing_prices)
        .iter()
        .map(|adjustment| adjustment.balance_delta)
        .sum();
    balance_delta + mint_delta
}

// Counters to verify tests are actually exercising trades
static TRADES_SEEN: AtomicU64 = AtomicU64::new(0);
static CASES_WITH_TRADES: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Property tests
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// After produce_block(), for every market: sum(YES) == sum(NO).
    /// Uses crossing pairs to guarantee trades actually happen.
    #[test]
    fn position_balance_after_block(
        submissions in arb_trading_batch(N_ACCOUNTS, N_MARKETS, make_markets()),
    ) {
        let (mut seq, markets) = make_sequencer();
        let bp = seq.produce_block(submissions, 1000);

        let n_fills = bp.block.fills.iter().filter(|f| f.fill_qty > 0).count();
        if n_fills > 0 {
            TRADES_SEEN.fetch_add(n_fills as u64, Ordering::Relaxed);
            CASES_WITH_TRADES.fetch_add(1, Ordering::Relaxed);
        }

        assert_position_balance(&seq, &markets);
    }

    /// Empty blocks are no-ops: state root and balances don't change.
    #[test]
    fn empty_block_is_noop(_dummy in 0u8..1) {
        let (mut seq, _) = make_sequencer();

        let pre_balance: i64 = seq.accounts.iter().map(|(_, a)| a.balance).sum();
        let bp1 = seq.produce_block(vec![], 1000);
        let post_balance: i64 = seq.accounts.iter().map(|(_, a)| a.balance).sum();

        assert_eq!(pre_balance, post_balance, "Empty block changed balances");
        assert!(bp1.block.fills.is_empty(), "Empty block produced fills");
        assert_eq!(bp1.analytics.total_welfare, 0);

        // Second empty block should produce the same state root
        let state_root_after_first = bp1.block.header.state_root;
        let bp2 = seq.produce_block(vec![], 2000);
        assert_eq!(
            state_root_after_first, bp2.block.header.state_root,
            "Empty blocks changed state root"
        );
    }

    /// Block chaining: block[n].parent_hash == hash(block[n-1].header), genesis parent is [0; 32].
    #[test]
    fn block_chaining(
        submissions in arb_trading_batch(N_ACCOUNTS, N_MARKETS, make_markets()),
    ) {
        let (mut seq, _) = make_sequencer();

        // Genesis block
        let bp0 = seq.produce_block(vec![], 1000);
        assert_eq!(bp0.block.header.parent_hash, [0u8; 32], "Genesis parent != zeros");

        // Second block with trades
        let bp1 = seq.produce_block(submissions, 2000);
        let expected_parent = matching_sequencer::block::hash_header(&bp0.block.header);
        assert_eq!(
            bp1.block.header.parent_hash, expected_parent,
            "Block 1 parent hash doesn't match hash of block 0 header"
        );
    }

    /// State root determinism: same scenario twice produces identical state roots.
    #[test]
    fn state_root_determinism(
        submissions in arb_trading_batch(N_ACCOUNTS, N_MARKETS, make_markets()),
    ) {
        // Run 1
        let (mut seq1, _) = make_sequencer();
        let bp1 = seq1.produce_block(submissions.clone(), 1000);

        // Run 2 (same scenario)
        let (mut seq2, _) = make_sequencer();
        let bp2 = seq2.produce_block(submissions, 1000);

        assert_eq!(
            bp1.block.header.state_root, bp2.block.header.state_root,
            "Identical scenarios produced different state roots"
        );
    }

    /// Batch commutativity (FBA property): shuffling submissions within a batch
    /// must preserve unscaled welfare. Reported nanos may differ by deterministic
    /// floor-rounding dust because fractional share-units are floored per fill.
    /// Clearing prices may differ when the LP has degenerate duals (multiple
    /// optimal price vectors with equal welfare), which is expected for markets
    /// with sparse order flow.
    #[test]
    fn batch_commutativity(
        submissions in arb_trading_batch(N_ACCOUNTS, N_MARKETS, make_markets()),
        seed in any::<u64>(),
    ) {
        use rand::seq::SliceRandom;
        use rand::SeedableRng;

        // Run with original order
        let (mut seq1, _) = make_sequencer();
        let bp1 = seq1.produce_block(submissions.clone(), 1000);

        // Run with shuffled order
        let mut shuffled = submissions;
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(seed);
        shuffled.shuffle(&mut rng);

        let (mut seq2, _) = make_sequencer();
        let bp2 = seq2.produce_block(shuffled, 1000);

        assert_eq!(bp1.analytics.total_welfare, recompute_total_welfare(&bp1.witness));
        assert_eq!(bp2.analytics.total_welfare, recompute_total_welfare(&bp2.witness));
        assert_eq!(
            raw_welfare_numerator(&bp1.witness),
            raw_welfare_numerator(&bp2.witness),
            "Shuffled submissions changed unscaled welfare"
        );
        let welfare_delta = (bp1.analytics.total_welfare - bp2.analytics.total_welfare).abs();
        let max_rounding_delta = bp1.witness.fills.len().max(bp2.witness.fills.len()) as i64;
        assert!(
            welfare_delta <= max_rounding_delta,
            "Shuffled submissions produced welfare outside rounding envelope: left={} right={} delta={} max_delta={}",
            bp1.analytics.total_welfare,
            bp2.analytics.total_welfare,
            welfare_delta,
            max_rounding_delta
        );
    }

    /// Resolution conservation: after resolving, balance delta matches position payouts,
    /// and positions are zeroed.
    #[test]
    fn resolution_conservation(
        submissions in arb_trading_batch(N_ACCOUNTS, N_MARKETS, make_markets()),
        payout in 0..=NANOS_PER_DOLLAR,
    ) {
        let (mut seq, _) = make_sequencer();
        // First produce a block to generate positions
        seq.produce_block(submissions, 1000);

        let m0 = MarketId::new(0);

        // Record pre-resolution state
        let mut pre_yes: i64 = 0;
        let mut pre_no: i64 = 0;
        let pre_balance: i64 = seq.accounts.iter().map(|(_, a)| a.balance).sum();
        for (_, account) in seq.accounts.iter() {
            pre_yes += account.position(m0, 0);
            pre_no += account.position(m0, 1);
        }
        let expected_delta = expected_resolution_delta(&seq, m0, payout);

        // Resolve
        let _ = seq.resolve_market(m0, payout, 2000);

        // Post-resolution checks
        let post_balance: i64 = seq.accounts.iter().map(|(_, a)| a.balance).sum();
        let balance_delta = post_balance - pre_balance;

        assert_eq!(
            balance_delta, expected_delta,
            "Resolution conservation violated: delta={} expected={} (yes={}, no={}, payout={})",
            balance_delta, expected_delta, pre_yes, pre_no, payout
        );

        // Positions should be zeroed
        for (_, account) in seq.accounts.iter() {
            assert_eq!(account.position(m0, 0), 0, "YES position not zeroed after resolution");
            assert_eq!(account.position(m0, 1), 0, "NO position not zeroed after resolution");
        }
    }
}

// ---------------------------------------------------------------------------
// Metamorphic: quantity scaling
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Doubling all quantities should preserve clearing prices, double fill quantities,
    /// and produce the exact welfare obtained by recomputing with doubled share-units.
    /// Integer floor division may add at most one nano of rounding delta per fill.
    #[test]
    fn quantity_scaling(
        base_submissions in arb_trading_batch(N_ACCOUNTS, N_MARKETS, make_markets()),
    ) {
        // Run with base quantities
        let (mut seq1, _) = make_sequencer();
        let bp1 = seq1.produce_block(base_submissions.clone(), 1000);

        // Double all quantities
        let doubled: Vec<OrderSubmission> = base_submissions
            .into_iter()
            .map(|sub| OrderSubmission {
                account_id: sub.account_id,
                orders: sub
                    .orders
                    .into_iter()
                    .map(|mut o| {
                        o.max_fill = o.max_fill.saturating_mul(2);
                        o
                    })
                    .collect(),
                mm_constraint: None,
            })
            .collect();

        let (mut seq2, _) = make_sequencer();
        let bp2 = seq2.produce_block(doubled, 1000);

        assert_eq!(bp1.analytics.total_welfare, recompute_total_welfare(&bp1.witness));
        assert_eq!(bp2.analytics.total_welfare, recompute_total_welfare(&bp2.witness));

        // Clearing prices should be the same
        assert_eq!(
            bp1.block.clearing_prices, bp2.block.clearing_prices,
            "Doubled quantities changed clearing prices"
        );

        // No silent pass when welfare is 0. With crossing pairs, we should
        // always get trades.
        assert!(
            bp1.analytics.total_welfare > 0,
            "Base batch produced zero welfare — strategy isn't generating crossing orders"
        );
        let base_fills: HashMap<u64, _> = bp1
            .witness
            .fills
            .iter()
            .map(|fill| (fill.order_id, fill))
            .collect();
        assert_eq!(
            base_fills.len(),
            bp1.witness.fills.len(),
            "Base batch produced multiple fills for the same order"
        );
        assert_eq!(
            base_fills.len(),
            bp2.witness.fills.len(),
            "Doubled quantities changed the filled order set"
        );

        for doubled_fill in &bp2.witness.fills {
            let base_fill = base_fills
                .get(&doubled_fill.order_id)
                .expect("Doubled batch filled an order not filled in the base batch");
            assert_eq!(
                doubled_fill.fill_price, base_fill.fill_price,
                "Doubled quantities changed fill price for order {}",
                doubled_fill.order_id
            );
            assert_eq!(
                doubled_fill.fill_qty,
                base_fill
                    .fill_qty
                    .checked_mul(2)
                    .expect("test quantities should double without overflow"),
                "Doubled quantities did not double fill quantity for order {}",
                doubled_fill.order_id
            );
        }

        let expected_doubled_welfare = recompute_total_welfare(&bp2.witness);
        assert_eq!(
            bp2.analytics.total_welfare,
            expected_doubled_welfare,
            "Doubled quantities did not match shared net welfare recomputation: expected={} doubled={}",
            expected_doubled_welfare,
            bp2.analytics.total_welfare
        );
        let rounding_delta = bp2.analytics.total_welfare - bp1.analytics.total_welfare * 2;
        let max_rounding_delta = (bp1.witness.fills.len() as i64).saturating_mul(2);
        assert!(
            rounding_delta.abs() <= max_rounding_delta,
            "Doubled welfare has impossible truncation delta: base={} doubled={} delta={} fills={}",
            bp1.analytics.total_welfare,
            bp2.analytics.total_welfare,
            rounding_delta,
            bp1.witness.fills.len()
        );
    }
}

// ---------------------------------------------------------------------------
// Multi-block invariants
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Run 3 consecutive blocks, assert invariants hold after each one.
    /// Exercises pending order re-validation and TTL.
    #[test]
    fn multi_block_invariants(
        batch1 in arb_trading_batch(N_ACCOUNTS, N_MARKETS, make_markets()),
        batch2 in arb_trading_batch(N_ACCOUNTS, N_MARKETS, make_markets()),
        batch3 in arb_trading_batch(N_ACCOUNTS, N_MARKETS, make_markets()),
    ) {
        let (mut seq, markets) = make_sequencer();

        let pre_balance: i64 = seq.accounts.iter().map(|(_, a)| a.balance).sum();

        let bp1 = seq.produce_block(batch1, 1000);
        assert_position_balance(&seq, &markets);

        let bp2 = seq.produce_block(batch2, 2000);
        assert_position_balance(&seq, &markets);

        // Block chaining
        assert_eq!(
            bp2.block.header.parent_hash,
            matching_sequencer::block::hash_header(&bp1.block.header)
        );

        let bp3 = seq.produce_block(batch3, 3000);
        assert_position_balance(&seq, &markets);
        assert_eq!(
            bp3.block.header.parent_hash,
            matching_sequencer::block::hash_header(&bp2.block.header)
        );

        let post_balance: i64 = seq.accounts.iter().map(|(_, a)| a.balance).sum();
        let expected_balance_delta: i64 = [&bp1, &bp2, &bp3]
            .iter()
            .map(|bp| expected_block_balance_delta(&bp.witness))
            .sum();

        // Balance movement must match the exact settlement deltas applied by
        // fills and MINT adjustments. Fractional quantities intentionally floor
        // each price*quantity conversion, so aggregate face value is not exact.
        assert_eq!(
            post_balance - pre_balance,
            expected_balance_delta,
            "Balance leak across 3 blocks: pre={} post={} expected_delta={}",
            pre_balance, post_balance, expected_balance_delta
        );
    }
}

/// Print trade coverage stats when the test binary exits.
/// This is NOT a test — it's a diagnostic.
#[test]
fn z_print_trade_coverage() {
    // Name starts with z_ so it runs last (tests are alphabetical within a binary).
    // This test always passes — it just prints stats from the atomic counters.
    let trades = TRADES_SEEN.load(Ordering::Relaxed);
    let cases = CASES_WITH_TRADES.load(Ordering::Relaxed);
    eprintln!(
        "\n=== TRADE COVERAGE: {} fills across {} cases with trades ===\n",
        trades, cases
    );
    // If we're getting zero trades, the strategies are broken
    // (but don't assert here — the counters only track position_balance_after_block)
}

/// Regression: crossing pair + unfilled solo order on the same market must still produce fills.
/// Previously, the DualMaster's iteration 2 would overwrite valid prices from iteration 1
/// with default 50/50 prices when only the unfilled solo order remained.
#[test]
fn crossing_pair_with_solo_order_still_matches() {
    let (mut seq, markets) = make_sequencer();
    let m0 = MarketId::new(0);

    let buyer_yes = OrderSubmission {
        account_id: AccountId(0),
        orders: vec![outcome_buy(&markets, 0, m0, 0, 100_000_000, 1)],
        mm_constraint: None,
    };
    let buyer_no = OrderSubmission {
        account_id: AccountId(1),
        orders: vec![outcome_buy(&markets, 0, m0, 1, 910_000_000, 1)],
        mm_constraint: None,
    };
    let solo = OrderSubmission {
        account_id: AccountId(2),
        orders: vec![outcome_buy(&markets, 0, m0, 0, 1, 1)],
        mm_constraint: None,
    };

    let bp = seq.produce_block(vec![buyer_yes, buyer_no, solo], 1000);
    assert!(
        !bp.block.fills.is_empty(),
        "Crossing pair must still match when an unfilled solo order is present"
    );
    assert!(bp.analytics.total_welfare > 0);
}

/// B7: per-market welfare. For a single-market trade, the sum of
/// `welfare_by_market` values equals `total_welfare` exactly. For
/// multi-market orders, sum-of-per-market over-counts (each active market
/// gets the full welfare contribution); `total_welfare` stays
/// authoritative.
#[test]
fn welfare_by_market_single_market_sums_to_total() {
    let (mut seq, markets) = make_sequencer();
    let m0 = MarketId::new(0);

    let buyer_yes = OrderSubmission {
        account_id: AccountId(0),
        orders: vec![outcome_buy(&markets, 0, m0, 0, 600_000_000, 1)],
        mm_constraint: None,
    };
    let buyer_no = OrderSubmission {
        account_id: AccountId(1),
        orders: vec![outcome_buy(&markets, 0, m0, 1, 600_000_000, 1)],
        mm_constraint: None,
    };

    let bp = seq.produce_block(vec![buyer_yes, buyer_no], 1000);
    assert!(
        !bp.block.fills.is_empty(),
        "expected fills from crossing buys"
    );
    assert!(bp.analytics.total_welfare > 0);
    let sum: i64 = bp.analytics.welfare_by_market.values().sum();
    assert_eq!(
        sum, bp.analytics.total_welfare,
        "single-market sum-of-per-market should equal total_welfare \
         (no multi-market over-counting in this scenario)"
    );
    // Only m0 had fills, so it's the only key in welfare_by_market.
    assert_eq!(bp.analytics.welfare_by_market.len(), 1);
    assert!(bp.analytics.welfare_by_market.contains_key(&m0));
}
