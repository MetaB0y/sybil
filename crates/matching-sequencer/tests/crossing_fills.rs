//! Regression guard for the "zero fills" incident (SYB-242).
//!
//! On the live devnet ~6,500 orders were placed but only ~24 matched —
//! effectively zero fills. The root cause was order-flow density plus all-IOC
//! flow with no crossing counterparty landing in the same batch. These tests
//! pin down the *matching semantics* so a future regression that silently
//! stops producing fills is caught in CI rather than in production.
//!
//! This is an Eisenberg–Gale / Fisher-market matching engine (see
//! `design/eg-conic.typ`), NOT LMSR side-constraints. In a binary prediction
//! market a **BuyYes at price p** and a **BuyNo at price q** on the *same*
//! market cross when `p + q >= $1`: together they mint a complete set (1 YES +
//! 1 NO), which is redeemable for exactly $1, so any surplus `p + q - 1` is
//! realizable welfare. When `p + q < $1` there is no way to fund the mint and
//! the pair must NOT trade.
//!
//! Everything here runs entirely in-process against `BlockSequencer::produce_block`
//! (no HTTP, no actor/mailbox), mirroring the harness in `tests/invariants.rs`.

use std::sync::Arc;

use matching_engine::{outcome_buy, MarketId, MarketSet, Qty, NANOS_PER_DOLLAR};
use matching_sequencer::{
    AccountId, AccountStore, AdminOracle, BlockProduction, BlockSequencer, OrderSubmission,
    SequencerConfig,
};

const INITIAL_BALANCE: i64 = 1_000 * NANOS_PER_DOLLAR as i64;
const QTY: u64 = 10;

/// YES is outcome index 0, NO is outcome index 1 for a binary market.
const YES: u8 = 0;
const NO: u8 = 1;

/// Build a one-market sequencer with two funded accounts (a YES buyer and a NO
/// buyer). Returns the sequencer, the market id, and the two account ids.
fn setup() -> (BlockSequencer, MarketId, AccountId, AccountId) {
    let mut accounts = AccountStore::new();
    let buyer_yes = accounts.create_account(INITIAL_BALANCE);
    let buyer_no = accounts.create_account(INITIAL_BALANCE);

    let mut markets = MarketSet::new();
    let market = markets.add_binary("Will it fill?");

    let oracle = Arc::new(AdminOracle::new());
    let seq = BlockSequencer::with_default_solver(
        accounts,
        markets,
        vec![],
        oracle,
        SequencerConfig::default(),
    );
    (seq, market, buyer_yes, buyer_no)
}

/// A BuyYes @ `yes_price` from `buyer_yes` and a BuyNo @ `no_price` from
/// `buyer_no`, each for `QTY` shares of `market`.
fn crossing_pair(
    market: MarketId,
    buyer_yes: AccountId,
    buyer_no: AccountId,
    yes_price: u64,
    no_price: u64,
) -> Vec<OrderSubmission> {
    // `outcome_buy(markets, id, market, outcome, limit_price, qty)`. The order
    // ids are local batch ids; the sequencer reassigns canonical ids.
    let ms = {
        let mut ms = MarketSet::new();
        ms.add_binary("Will it fill?");
        ms
    };
    vec![
        OrderSubmission {
            account_id: buyer_yes,
            orders: vec![outcome_buy(&ms, 0, market, YES, yes_price, QTY)],
            mm_constraint: None,
        },
        OrderSubmission {
            account_id: buyer_no,
            orders: vec![outcome_buy(&ms, 0, market, NO, no_price, QTY)],
            mm_constraint: None,
        },
    ]
}

fn matched_qty(bp: &BlockProduction) -> u64 {
    bp.block
        .fills
        .iter()
        .filter(|f| f.fill_qty > Qty::ZERO)
        .map(|f| f.fill_qty.0)
        .sum()
}

/// Total cash held by all accounts.
fn total_balance(seq: &BlockSequencer) -> i64 {
    seq.accounts.iter().map(|(_, a)| a.balance).sum()
}

/// Total YES and total NO shares held across all accounts for `market`.
fn total_positions(seq: &BlockSequencer, market: MarketId) -> (i64, i64) {
    let mut yes = 0;
    let mut no = 0;
    for (_, account) in seq.accounts.iter() {
        yes += account.position(market, YES);
        no += account.position(market, NO);
    }
    (yes, no)
}

/// Positive case: BuyYes @ 0.55 + BuyNo @ 0.55 (sum = $1.10 >= $1) MUST fill.
///
/// Asserts the exact EG-matching contract:
///  * fills are produced (`fill_count`/matched > 0) — the anti-regression core;
///  * the block-level fill counters agree with the per-fill quantities;
///  * clearing prices are strictly inside (0, $1) and the complementary
///    outcomes sum to $1 — i.e. `p_YES + p_NO = N`, which holds "when minting
///    is active, by stationarity of the mint variable" (design/eg-conic.typ);
///  * positive welfare is realized (the $0.10 crossing surplus);
///  * value is conserved: the cash the buyers spend equals the face value of
///    the complete sets minted, so nothing is created out of thin air.
#[test]
fn crossing_orders_produce_fills() {
    let (mut seq, market, buyer_yes, buyer_no) = setup();
    let cash_before = total_balance(&seq);

    let p = 55 * NANOS_PER_DOLLAR / 100; // $0.55
    let q = 55 * NANOS_PER_DOLLAR / 100; // $0.55
    assert!(p + q >= NANOS_PER_DOLLAR, "test precondition: pair crosses");

    let bp = seq.produce_block(crossing_pair(market, buyer_yes, buyer_no, p, q), 1_000);

    // 1. Fills actually happened — this is the regression the ticket guards.
    let matched = matched_qty(&bp);
    assert!(
        matched > 0,
        "crossing BuyYes@0.55 + BuyNo@0.55 produced ZERO fills (the SYB-242 regression)"
    );
    assert!(
        bp.block.header.fill_count > 0,
        "header fill_count must be > 0"
    );
    // Fully crossing pair of equal size: both legs clear completely.
    assert_eq!(
        matched,
        2 * QTY,
        "both legs should fill their full {} shares",
        QTY
    );

    // 2. Clearing prices: complementary outcomes, strictly inside (0,$1), sum $1.
    let prices = bp
        .block
        .clearing_prices
        .get(&market)
        .expect("a matched market must publish clearing prices");
    let p_yes = prices[YES as usize].0;
    let p_no = prices[NO as usize].0;
    assert!(
        p_yes > 0 && p_yes < NANOS_PER_DOLLAR,
        "YES clearing price {p_yes} not strictly inside (0, $1)"
    );
    assert!(
        p_no > 0 && p_no < NANOS_PER_DOLLAR,
        "NO clearing price {p_no} not strictly inside (0, $1)"
    );
    // p_YES + p_NO = N when minting is active (design/eg-conic.typ, l.135). Allow
    // a couple of nanos for deterministic price normalization/rounding dust.
    let sum = p_yes + p_no;
    assert!(
        sum.abs_diff(NANOS_PER_DOLLAR) <= 2,
        "complementary clearing prices must sum to $1: {p_yes} + {p_no} = {sum}"
    );

    // 3. Positive welfare: the $0.10 crossing surplus is realized.
    assert!(
        bp.analytics.total_welfare > 0,
        "a crossing trade must realize positive welfare, got {}",
        bp.analytics.total_welfare
    );

    // 4. Conservation — no value created out of thin air. Crossing buyers mint
    //    complete sets, so total YES == total NO (equal-and-opposite legs), and
    //    the cash removed from the buyers is exactly the block's minting cost:
    //    the removed cash funds the newly minted complete-set liability, it is
    //    not created or destroyed. `matched` share-legs form `matched/2` sets.
    let (total_yes, total_no) = total_positions(&seq, market);
    assert_eq!(
        total_yes, total_no,
        "minting must keep YES and NO in balance"
    );
    assert_eq!(
        total_yes,
        (matched / 2) as i64,
        "minted sets (per outcome) == matched share-legs / 2"
    );

    let cash_after = total_balance(&seq);
    let cash_removed = cash_before - cash_after;
    assert!(cash_removed > 0, "buyers must have paid for their fills");
    assert_eq!(
        cash_removed, bp.witness.minting_cost,
        "value conservation: cash removed from buyers must equal the minting cost"
    );
    // The realized welfare is exactly the crossing surplus fraction (p+q-$1)/$1
    // of the minting cost — the buyers keep, as unspent cash, the amount by
    // which their combined limit beat the $1 mint. Nothing is fabricated.
    assert_eq!(
        bp.analytics.total_welfare,
        cash_removed * (p + q - NANOS_PER_DOLLAR) as i64 / NANOS_PER_DOLLAR as i64,
        "realized welfare must equal the crossing surplus over the mint cost"
    );
}

/// Negative case: BuyYes @ 0.45 + BuyNo @ 0.45 (sum = $0.90 < $1) MUST NOT fill.
///
/// There is no way to fund the $1 complete-set mint from $0.90 of bids, so the
/// EG program leaves both orders resting: zero fills, no clearing prices, no
/// welfare, and balances untouched. If this ever starts producing fills the
/// engine is minting value from nothing.
#[test]
fn non_crossing_orders_produce_zero_fills() {
    let (mut seq, market, buyer_yes, buyer_no) = setup();
    let cash_before = total_balance(&seq);

    let p = 45 * NANOS_PER_DOLLAR / 100; // $0.45
    let q = 45 * NANOS_PER_DOLLAR / 100; // $0.45
    assert!(
        p + q < NANOS_PER_DOLLAR,
        "test precondition: pair does NOT cross"
    );

    let bp = seq.produce_block(crossing_pair(market, buyer_yes, buyer_no, p, q), 1_000);

    assert_eq!(
        matched_qty(&bp),
        0,
        "non-crossing BuyYes@0.45 + BuyNo@0.45 must produce ZERO fills"
    );
    assert_eq!(bp.block.header.fill_count, 0, "header fill_count must be 0");
    assert_eq!(
        bp.analytics.total_welfare, 0,
        "no trade means no welfare is realized"
    );

    let (total_yes, total_no) = total_positions(&seq, market);
    assert_eq!((total_yes, total_no), (0, 0), "no positions may be created");
    assert_eq!(
        total_balance(&seq),
        cash_before,
        "balances must be untouched when nothing trades"
    );
}
