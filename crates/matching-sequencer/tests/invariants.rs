//! Property-based and metamorphic tests for BlockSequencer invariants.
//!
//! These tests hard-assert the economic invariants that the sequencer currently
//! only checks with `eprintln` warnings (sequencer.rs:650-715).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use matching_engine::{outcome_buy, MarketId, MarketSet, NANOS_PER_DOLLAR};
use matching_sequencer::{
    AccountId, AccountStore, AdminOracle, BlockSequencer, OrderSubmission,
};
use proptest::prelude::*;

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
        0..n_markets,                                       // market
        NANOS_PER_DOLLAR / 10..9 * NANOS_PER_DOLLAR / 10,  // yes_price (10%-90%)
        NANOS_PER_DOLLAR / 100..NANOS_PER_DOLLAR / 5,        // 1-20% surplus guarantees positive welfare
        1u64..50,                                           // quantity
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
    (0..n_accounts, 0..n_markets, 1..NANOS_PER_DOLLAR, 1u64..50, 0u8..2).prop_map(
        move |(acct, market_idx, price, qty, outcome)| {
            let mid = MarketId::new(market_idx);
            OrderSubmission {
                account_id: AccountId(acct),
                orders: vec![outcome_buy(&ms, 0, mid, outcome, price, qty)],
                mm_constraint: None,
            }
        },
    )
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
        ms.add_binary(&format!("Market {}", i));
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
    let seq = BlockSequencer::new(accounts, markets.clone(), vec![], oracle);
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
        assert_eq!(bp1.block.total_welfare, 0);

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
    /// must produce identical clearing prices and total welfare.
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

        assert_eq!(
            bp1.block.clearing_prices, bp2.block.clearing_prices,
            "Shuffled submissions produced different clearing prices"
        );
        assert_eq!(
            bp1.block.total_welfare, bp2.block.total_welfare,
            "Shuffled submissions produced different welfare"
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

        // Resolve
        let _ = seq.resolve_market(m0, payout, 2000);

        // Post-resolution checks
        let post_balance: i64 = seq.accounts.iter().map(|(_, a)| a.balance).sum();
        let balance_delta = post_balance - pre_balance;

        // Expected payout: YES * yes_payout + NO * no_payout
        let no_payout = NANOS_PER_DOLLAR - payout;
        let expected_delta =
            (pre_yes as i128 * payout as i128 + pre_no as i128 * no_payout as i128) as i64;

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

    /// Doubling all quantities should double welfare and not change clearing prices.
    /// (Catches overflow/rounding bugs.)
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

        // Clearing prices should be the same
        assert_eq!(
            bp1.block.clearing_prices, bp2.block.clearing_prices,
            "Doubled quantities changed clearing prices"
        );

        // Welfare MUST double — no silent pass when welfare is 0.
        // With crossing pairs, we should always get trades.
        assert!(
            bp1.block.total_welfare > 0,
            "Base batch produced zero welfare — strategy isn't generating crossing orders"
        );
        assert_eq!(
            bp2.block.total_welfare,
            bp1.block.total_welfare * 2,
            "Doubled quantities didn't double welfare: base={} doubled={}",
            bp1.block.total_welfare,
            bp2.block.total_welfare
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

        // Total balance can only change by the value locked in positions.
        // Each minted YES+NO pair costs $1.  Global YES == Global NO (position balance),
        // so total locked capital = global_yes * NANOS_PER_DOLLAR summed over markets.
        let post_balance: i64 = seq.accounts.iter().map(|(_, a)| a.balance).sum();
        let mut total_position_value: i64 = 0;
        for market in markets.iter() {
            let mut global_yes: i64 = 0;
            for (_, account) in seq.accounts.iter() {
                global_yes += account.position(market.id, 0);
            }
            // global_yes == global_no already asserted by assert_position_balance
            total_position_value += global_yes * NANOS_PER_DOLLAR as i64;
        }
        // Money out of balances = money locked in positions
        assert_eq!(
            pre_balance - post_balance,
            total_position_value,
            "Balance leak across 3 blocks: pre={} post={} positions={}",
            pre_balance, post_balance, total_position_value
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
    assert!(bp.block.total_welfare > 0);
}
