//! SYB-246: named economic property tests over random valid books/blocks.
//!
//! Property catalog (each is a distinct named proptest; formal statements in
//! the doc comment of each test):
//!
//! 1. `value_conservation_deposits_trades_withdrawals_resolution`
//!    external_value_in == Σ balances + complete_set_escrow + withdrawal_escrow
//!    (± floor-rounding dust, budgeted per settlement event).
//! 2. `no_arbitrage_complementary_clearing_prices`
//!    For every market where the block minted complete sets:
//!    p_YES + p_NO = $1 (± 2 nanos normalization dust) — the EG stationarity
//!    condition of the free mint variable (`design/eg-conic.typ` §Price
//!    Extraction; ADR-0001). Plus individual rationality: no fill outside its
//!    order's limit price.
//! 3. `strictly_crossing_book_produces_nonzero_volume`
//!    A book containing at least one strictly-crossing complementary pair
//!    (p_YES + p_NO ≥ $1 + surplus) matches non-zero volume with positive
//!    welfare.
//! 4. `arrival_order_shuffle_preserves_economic_outcome`
//!    Shuffling submission arrival order preserves the unscaled welfare
//!    objective exactly, and conservation + price coherence hold in both
//!    orderings. (Clearing prices/fill sets may differ under degenerate LP
//!    duals — see `invariants.rs::batch_commutativity` — so the *economic*
//!    outcome, not the byte-level block, is what is order-independent.)
//! 5. `zero_fill_block_is_economic_noop`
//!    A block whose book cannot cross produces zero fills and is an exact
//!    economic no-op: balances, positions, escrow and the conservation defect
//!    are all unchanged (no dust allowance — zero fills mean zero floors).
//! 6. `complete_set_mint_then_burn_round_trip_conserves_value`
//!    A generated complete set is minted through two complementary buys, then
//!    burned through sells from the resulting live holdings. Both blocks obey
//!    UCP and exact signed welfare; the full cycle restores aggregate cash and
//!    destroys every generated position.
//!
//! Falsifiability (SYB-246 acceptance): the deterministic
//! `falsifiability_*` tests feed post-hoc perturbed fills/prices from a real
//! block through the *same* checker functions used by the properties above
//! and assert the checkers reject them. The replay oracle deliberately does
//! not call production settlement or minting helpers, so shared bugs cannot
//! make the test agree with the implementation under test.
//!
//! Money model background (see `design/eg-conic.typ`, `design/mint-pnl.typ`,
//! ADR-0001, ADR-0004): crossing BuyYES/BuyNO orders mint complete sets; the
//! cash the buyers spend funds a $1-per-set redemption liability (the
//! "escrow" below). One-sided imbalance is absorbed by the MINT account
//! (`AccountId::MINT`), which shorts the imbalance at the clearing price.
//! Resolution converts positions back to cash at the payout prices, which
//! sum to $1 per complete set. Every price×quantity conversion floors
//! (ADR-0004: integer truth), so each settlement event can lose < 1 nano —
//! hence the explicit dust budget.

use matching_engine::{
    Fill, MarketId, MarketSet, NANOS_PER_DOLLAR, Nanos, Order, Qty, SHARE_SCALE, outcome_buy,
    outcome_sell, shares_to_qty,
};
use matching_sequencer::bridge::{account_key, append_deposit_frontier};
use matching_sequencer::{
    AccountId, AccountStore, BlockSequencer, BridgeWithdrawalL1Event, BridgeWithdrawalRequest,
    L1Deposit, L1WithdrawalStatus, OrderSubmission, SequencerConfig, WithdrawalLeaf,
};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use std::collections::{HashMap, HashSet};
use sybil_verifier::BlockWitness;

const N_ACCOUNTS: u64 = 4;
const N_MARKETS: u32 = 2;
const INITIAL_BALANCE: i64 = 1_000 * NANOS_PER_DOLLAR as i64;

/// Clearing-price normalization dust pinned by
/// `crossing_fills.rs::crossing_orders_produce_fills`: the published
/// complementary prices may differ from an exact $1 sum by at most 2 nanos.
const PRICE_COHERENCE_DUST: u64 = 2;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

fn make_markets() -> MarketSet {
    let mut ms = MarketSet::new();
    for i in 0..N_MARKETS {
        ms.add_binary(format!("Market {i}"));
    }
    ms
}

fn make_sequencer() -> (BlockSequencer, MarketSet) {
    let mut accounts = AccountStore::new();
    for _ in 0..N_ACCOUNTS {
        accounts.create_account(INITIAL_BALANCE);
    }
    let markets = make_markets();
    // Generated cases deliberately include dust values to probe arithmetic;
    // admission-floor policy has focused tests in the sequencer crate.
    let config = SequencerConfig {
        min_resting_order_notional_nanos: 0,
        ..SequencerConfig::default()
    };
    let seq = BlockSequencer::with_default_solver(accounts, markets.clone(), vec![], config);
    (seq, markets)
}

/// Total cash across ALL accounts, including the protocol MINT account.
fn total_balance(seq: &BlockSequencer) -> i64 {
    seq.accounts.iter().map(|(_, a)| a.balance).sum()
}

/// Per-market `(market, Σ YES, Σ NO)` across all accounts including MINT.
fn market_totals(seq: &BlockSequencer, markets: &MarketSet) -> Vec<(MarketId, i64, i64)> {
    markets
        .iter()
        .map(|market| {
            let mut yes = 0i64;
            let mut no = 0i64;
            for (_, account) in seq.accounts.iter() {
                yes += account.position(market.id, 0);
                no += account.position(market.id, 1);
            }
            (market.id, yes, no)
        })
        .collect()
}

fn nonzero_fill_count(witness: &BlockWitness) -> i64 {
    witness
        .fills
        .iter()
        .filter(|f| f.fill_qty > Qty::ZERO)
        .count() as i64
}

// ---------------------------------------------------------------------------
// Named economic checkers (pure functions, shared by the properties and the
// falsifiability tests)
// ---------------------------------------------------------------------------

/// Test-oracle notional arithmetic, intentionally independent of
/// `matching_engine::notional_nanos` and the production settlement helpers.
fn oracle_notional(price_nanos: u64, qty: u64) -> Result<i64, String> {
    let value = (price_nanos as u128)
        .checked_mul(qty as u128)
        .ok_or_else(|| format!("oracle notional overflow: price={price_nanos} qty={qty}"))?
        / SHARE_SCALE as u128;
    i64::try_from(value)
        .map_err(|_| format!("oracle notional does not fit i64: price={price_nanos} qty={qty}"))
}

/// Independently recognize the public one-market/one-hot order language.
/// Returns `(market, outcome, is_sell)`.
fn oracle_order_side(order: &Order) -> Result<(MarketId, u8, bool), String> {
    if order.num_markets != 1 || order.num_states != 2 {
        return Err(format!(
            "order {} is not a single binary order (markets={} states={})",
            order.id, order.num_markets, order.num_states
        ));
    }
    let side = match (order.payoffs[0], order.payoffs[1]) {
        (1, 0) => (0, false),
        (-1, 0) => (0, true),
        (0, 1) => (1, false),
        (0, -1) => (1, true),
        payoffs => {
            return Err(format!(
                "order {} is not exact one-hot: payoffs={payoffs:?}",
                order.id
            ));
        }
    };
    Ok((order.markets[0], side.0, side.1))
}

/// Value-conservation defect in nanos.
///
/// Formal statement: with
///   D = external value in (initial funding + L1 deposits),
///   W = withdrawal escrow (requested-and-not-refunded withdrawal amounts),
///   B = Σ balances over all accounts including MINT,
///   E = Σ_m $1 × complete_sets(m)  (the redemption liability of outstanding
///       complete sets; exact because $1/SHARE_SCALE is an integer),
/// conservation requires  D − W − B − E == 0  up to floor dust.
///
/// Returns `Err` if any market's YES/NO totals are imbalanced (a conservation
/// violation in itself — every YES must be backed by a NO), otherwise the
/// signed defect `D − W − B − E`.
fn value_conservation_defect(
    external_in: i64,
    withdrawal_escrow: i64,
    balance_total: i64,
    totals: &[(MarketId, i64, i64)],
) -> Result<i64, String> {
    let mut escrow_value = 0i64;
    for &(market, yes, no) in totals {
        if yes != no {
            return Err(format!(
                "market {market:?} position imbalance: YES={yes} NO={no}"
            ));
        }
        if yes < 0 {
            return Err(format!(
                "market {market:?} negative outstanding sets: {yes}"
            ));
        }
        escrow_value = escrow_value
            .checked_add(oracle_notional(NANOS_PER_DOLLAR, yes as u64)?)
            .ok_or_else(|| "complete-set escrow total overflowed i64".to_owned())?;
    }
    external_in
        .checked_sub(withdrawal_escrow)
        .and_then(|value| value.checked_sub(balance_total))
        .and_then(|value| value.checked_sub(escrow_value))
        .ok_or_else(|| "value-conservation defect overflowed i64".to_owned())
}

/// Assert conservation within an explicit dust budget.
fn check_value_conservation(
    external_in: i64,
    withdrawal_escrow: i64,
    balance_total: i64,
    totals: &[(MarketId, i64, i64)],
    dust_budget: i64,
) -> Result<(), String> {
    let defect = value_conservation_defect(external_in, withdrawal_escrow, balance_total, totals)?;
    if defect.abs() > dust_budget {
        return Err(format!(
            "value conservation violated: defect={defect} nanos exceeds dust budget \
             {dust_budget} (external_in={external_in} withdrawal_escrow={withdrawal_escrow} \
             balances={balance_total})"
        ));
    }
    Ok(())
}

/// No-arbitrage price coherence: every published binary price vector has
/// exactly two entries in [0, $1], and for every traded market the complementary
/// prices satisfy p_YES + p_NO = $1 up to `PRICE_COHERENCE_DUST` nanos —
/// the collateral identity for a complete set. If this failed, minting or
/// burning at the clearing vector would exchange something worth $1 for a
/// different amount of cash.
fn check_no_arbitrage_prices(
    clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
    traded_markets: &HashSet<MarketId>,
) -> Result<(), String> {
    for (market, prices) in clearing_prices {
        if prices.len() != 2 {
            return Err(format!(
                "market {market:?} published {} prices, want 2",
                prices.len()
            ));
        }
        for price in prices {
            if price.0 > NANOS_PER_DOLLAR {
                return Err(format!(
                    "market {market:?} price {price} exceeds one dollar"
                ));
            }
        }
        if traded_markets.contains(market) {
            let sum = prices[0].0 + prices[1].0;
            if sum.abs_diff(NANOS_PER_DOLLAR) > PRICE_COHERENCE_DUST {
                return Err(format!(
                    "no-arbitrage violated in market {market:?}: p_YES + p_NO = {} + {} = {sum}, \
                     want $1 ± {PRICE_COHERENCE_DUST}",
                    prices[0], prices[1]
                ));
            }
        }
    }
    for market in traded_markets {
        if !clearing_prices.contains_key(market) {
            return Err(format!(
                "market {market:?} traded but published no clearing prices"
            ));
        }
    }
    Ok(())
}

/// Fill feasibility and individual rationality. Every emitted fill is unique,
/// positive, bounded by the admitted order, priced at the market's published
/// UCP, and within the participant's limit.
fn check_fill_feasibility(
    orders: &[(Order, u64)],
    fills: &[Fill],
    clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
) -> Result<(), String> {
    let mut order_map = HashMap::new();
    for (order, account_id) in orders {
        if order_map.insert(order.id, (order, *account_id)).is_some() {
            return Err(format!("duplicate witness order id {}", order.id));
        }
    }
    let mut filled_orders = HashSet::new();
    for fill in fills {
        if fill.fill_qty == Qty::ZERO {
            return Err(format!(
                "order {} emitted a zero-quantity fill",
                fill.order_id
            ));
        }
        if !filled_orders.insert(fill.order_id) {
            return Err(format!("duplicate fill for order {}", fill.order_id));
        }
        let Some(&(order, account_id)) = order_map.get(&fill.order_id) else {
            return Err(format!("fill references unknown order {}", fill.order_id));
        };
        if fill.fill_qty > order.max_fill {
            return Err(format!(
                "order {} fill quantity {} exceeds max {}",
                fill.order_id, fill.fill_qty, order.max_fill
            ));
        }
        if fill.account_id != 0 && fill.account_id != account_id {
            return Err(format!(
                "order {} fill account {} disagrees with witness account {}",
                fill.order_id, fill.account_id, account_id
            ));
        }
        if fill.fill_price.0 > NANOS_PER_DOLLAR {
            return Err(format!(
                "order {} filled at {} > $1",
                fill.order_id, fill.fill_price
            ));
        }
        let (market, outcome, is_sell) = oracle_order_side(order)?;
        let ucp = clearing_prices
            .get(&market)
            .and_then(|prices| prices.get(outcome as usize))
            .ok_or_else(|| {
                format!(
                    "order {} has no clearing price for market {market:?} outcome {outcome}",
                    fill.order_id
                )
            })?;
        if fill.fill_price != *ucp {
            return Err(format!(
                "order {} fill price {} disagrees with UCP {}",
                fill.order_id, fill.fill_price, ucp
            ));
        }
        let ok = if order.is_seller() {
            fill.fill_price >= order.limit_price
        } else {
            fill.fill_price <= order.limit_price
        };
        if !ok {
            return Err(format!(
                "order {} filled outside limit: fill_price={} limit={} seller={}",
                fill.order_id,
                fill.fill_price,
                order.limit_price,
                order.is_seller()
            ));
        }
        if is_sell != order.is_seller() {
            return Err(format!(
                "order {} side classification disagrees with seller predicate",
                fill.order_id
            ));
        }
    }
    Ok(())
}

/// Independently sum participant surplus from fill prices and order limits.
fn oracle_fill_surplus(orders: &[(Order, u64)], fills: &[Fill]) -> Result<i64, String> {
    let order_map: HashMap<u64, &Order> =
        orders.iter().map(|(order, _)| (order.id, order)).collect();
    let mut total = 0i64;
    for fill in fills {
        let order = order_map
            .get(&fill.order_id)
            .ok_or_else(|| format!("fill references unknown order {}", fill.order_id))?;
        let (_, _, is_sell) = oracle_order_side(order)?;
        let favorable_delta = if is_sell {
            fill.fill_price.0.checked_sub(order.limit_price.0)
        } else {
            order.limit_price.0.checked_sub(fill.fill_price.0)
        }
        .ok_or_else(|| format!("order {} filled outside its limit", fill.order_id))?;
        total = total
            .checked_add(oracle_notional(favorable_delta, fill.fill_qty.0)?)
            .ok_or_else(|| "aggregate fill surplus overflowed i64".to_owned())?;
    }
    Ok(total)
}

/// Witness orders as `(Order, account_id)` pairs for the checkers above.
fn witness_orders(witness: &BlockWitness) -> Vec<(Order, u64)> {
    witness
        .orders
        .iter()
        .map(|wo| (wo.order.clone(), wo.account_id))
        .collect()
}

/// Markets in which a block contains a non-zero fill.
fn traded_markets(witness: &BlockWitness) -> HashSet<MarketId> {
    let order_market: HashMap<u64, MarketId> = witness
        .orders
        .iter()
        .map(|wo| (wo.order.id, wo.order.markets[0]))
        .collect();
    witness
        .fills
        .iter()
        .filter(|f| f.fill_qty > Qty::ZERO)
        .filter_map(|f| order_market.get(&f.order_id).copied())
        .collect()
}

// ---------------------------------------------------------------------------
// Post-hoc block replay (verifier-style; used by the falsifiability tests)
// ---------------------------------------------------------------------------

/// A snapshot of every account's balance and positions.
type LedgerSnapshot = HashMap<u64, (i64, HashMap<(MarketId, u8), i64>)>;

/// Aggregates a claimed block replay produces: `(Σ balances, market totals)`.
type ClaimedAggregates = (i64, Vec<(MarketId, i64, i64)>);

fn snapshot_ledger(seq: &BlockSequencer, markets: &MarketSet) -> LedgerSnapshot {
    seq.accounts
        .iter()
        .map(|(id, account)| {
            let mut positions = HashMap::new();
            for market in markets.iter() {
                for outcome in 0..2u8 {
                    let qty = account.position(market.id, outcome);
                    if qty != 0 {
                        positions.insert((market.id, outcome), qty);
                    }
                }
            }
            (id.0, (account.balance, positions))
        })
        .collect()
}

/// Re-derive the post-block ledger a *claimed* set of fills implies. This is a
/// deliberately narrow one-hot oracle with separately written arithmetic: it
/// does not call `compute_fill_settlement`, `derive_minting`, or the verifier.
fn replay_claimed_block(
    pre: &LedgerSnapshot,
    orders: &[(Order, u64)],
    fills: &[Fill],
    clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
    markets: &MarketSet,
) -> Result<ClaimedAggregates, String> {
    let order_map: HashMap<u64, (&Order, u64)> = orders
        .iter()
        .map(|(order, account)| (order.id, (order, *account)))
        .collect();
    let mut ledger = pre.clone();

    for fill in fills {
        if fill.fill_qty == Qty::ZERO {
            continue;
        }
        let Some(&(order, order_account)) = order_map.get(&fill.order_id) else {
            return Err(format!("fill references unknown order {}", fill.order_id));
        };
        let account_id = if fill.account_id != 0 {
            fill.account_id
        } else {
            order_account
        };
        let (market, outcome, is_sell) = oracle_order_side(order)?;
        let qty = i64::try_from(fill.fill_qty.0)
            .map_err(|_| format!("fill quantity {} does not fit i64", fill.fill_qty))?;
        let balance_delta = oracle_notional(fill.fill_price.0, fill.fill_qty.0)?;
        let entry = ledger.entry(account_id).or_default();
        entry.0 = entry
            .0
            .checked_add(if is_sell {
                balance_delta
            } else {
                -balance_delta
            })
            .ok_or_else(|| format!("account {account_id} balance overflow"))?;
        let position = entry.1.entry((market, outcome)).or_insert(0);
        *position = position
            .checked_add(if is_sell { -qty } else { qty })
            .ok_or_else(|| format!("account {account_id} position overflow"))?;
    }

    let totals: Vec<(MarketId, i64, i64)> = markets
        .iter()
        .map(|market| {
            let mut yes = 0i64;
            let mut no = 0i64;
            for (_, positions) in ledger.values() {
                yes += positions.get(&(market.id, 0)).copied().unwrap_or(0);
                no += positions.get(&(market.id, 1)).copied().unwrap_or(0);
            }
            (market.id, yes, no)
        })
        .collect();

    // Independently model the MINT counterparty: it shorts whichever outcome
    // is long in aggregate and receives that outcome's UCP.
    let mint = ledger.entry(AccountId::MINT.0).or_default();
    for &(market, yes, no) in &totals {
        let diff = yes
            .checked_sub(no)
            .ok_or_else(|| format!("market {market:?} position difference overflow"))?;
        if diff == 0 {
            continue;
        }
        let (outcome, position_delta) = if diff > 0 { (0, -diff) } else { (1, diff) };
        let price = clearing_prices
            .get(&market)
            .and_then(|prices| prices.get(outcome as usize))
            .ok_or_else(|| {
                format!("market {market:?} outcome {outcome} imbalance has no clearing price")
            })?;
        let balance_delta = oracle_notional(price.0, diff.unsigned_abs())?;
        mint.0 = mint
            .0
            .checked_add(balance_delta)
            .ok_or_else(|| "MINT balance overflow".to_owned())?;
        let position = mint.1.entry((market, outcome)).or_insert(0);
        *position = position
            .checked_add(position_delta)
            .ok_or_else(|| "MINT position overflow".to_owned())?;
    }

    let balance_total: i64 = ledger.values().map(|(balance, _)| balance).sum();
    let final_totals: Vec<(MarketId, i64, i64)> = markets
        .iter()
        .map(|market| {
            let mut yes = 0i64;
            let mut no = 0i64;
            for (_, positions) in ledger.values() {
                yes += positions.get(&(market.id, 0)).copied().unwrap_or(0);
                no += positions.get(&(market.id, 1)).copied().unwrap_or(0);
            }
            (market.id, yes, no)
        })
        .collect();
    Ok((balance_total, final_totals))
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// A guaranteed-crossing complementary pair: BuyYES at `yes_price`, BuyNO at
/// `min($1, $1 − yes_price + surplus)`, so `p_YES + p_NO ≥ $1 + min(surplus, yes_price)`.
fn arb_crossing_pair(markets: MarketSet) -> impl Strategy<Value = Vec<OrderSubmission>> {
    (
        0..N_MARKETS,
        NANOS_PER_DOLLAR / 10..9 * NANOS_PER_DOLLAR / 10,
        NANOS_PER_DOLLAR / 100..NANOS_PER_DOLLAR / 5,
        1u64..50,
        (0..N_ACCOUNTS, 0..N_ACCOUNTS).prop_filter("distinct accounts", |(a, b)| a != b),
    )
        .prop_map(move |(market_idx, yes_price, surplus, qty, (a1, a2))| {
            let mid = MarketId::new(market_idx);
            let no_price = (NANOS_PER_DOLLAR - yes_price + surplus).min(NANOS_PER_DOLLAR);
            vec![
                OrderSubmission {
                    account_id: AccountId(a1),
                    orders: vec![outcome_buy(&markets, 0, mid, 0, yes_price, qty)],
                    mm_constraint: None,
                },
                OrderSubmission {
                    account_id: AccountId(a2),
                    orders: vec![outcome_buy(&markets, 0, mid, 1, no_price, qty)],
                    mm_constraint: None,
                },
            ]
        })
}

/// A single random buy order that may or may not cross with anything.
fn arb_solo_order(markets: MarketSet) -> impl Strategy<Value = OrderSubmission> {
    (
        0..N_ACCOUNTS,
        0..N_MARKETS,
        1..NANOS_PER_DOLLAR,
        1u64..50,
        0u8..2,
    )
        .prop_map(
            move |(acct, market_idx, price, qty, outcome)| OrderSubmission {
                account_id: AccountId(acct),
                orders: vec![outcome_buy(
                    &markets,
                    0,
                    MarketId::new(market_idx),
                    outcome,
                    price,
                    qty,
                )],
                mm_constraint: None,
            },
        )
}

/// 1–4 crossing pairs plus 0–4 random solo orders.
fn arb_trading_batch(markets: MarketSet) -> impl Strategy<Value = Vec<OrderSubmission>> {
    (
        prop::collection::vec(arb_crossing_pair(markets.clone()), 1..=4),
        prop::collection::vec(arb_solo_order(markets), 0..=4),
    )
        .prop_map(|(pairs, solos)| {
            let mut all: Vec<OrderSubmission> = pairs.into_iter().flatten().collect();
            all.extend(solos);
            all
        })
}

/// A book that cannot cross: buys only, and per market every YES limit plus
/// every NO limit sums below $0.90 < $1, so no complete set can be funded.
fn arb_noncrossing_book(markets: MarketSet) -> impl Strategy<Value = Vec<OrderSubmission>> {
    prop::collection::vec(
        (
            0..N_ACCOUNTS,
            0..N_MARKETS,
            NANOS_PER_DOLLAR / 100..45 * NANOS_PER_DOLLAR / 100,
            1u64..50,
            0u8..2,
        )
            .prop_map(
                move |(acct, market_idx, price, qty, outcome)| OrderSubmission {
                    account_id: AccountId(acct),
                    orders: vec![outcome_buy(
                        &markets,
                        0,
                        MarketId::new(market_idx),
                        outcome,
                        price,
                        qty,
                    )],
                    mm_constraint: None,
                },
            ),
        1..=6,
    )
}

/// Parameters for a full complete-set lifecycle. Buy limits sum above $1,
/// while sell limits sum below $1, guaranteeing positive welfare in both
/// directions.
fn arb_complete_set_cycle() -> impl Strategy<Value = (u64, u64, u64, u64, u64)> {
    (
        NANOS_PER_DOLLAR / 10..9 * NANOS_PER_DOLLAR / 10,
        1..NANOS_PER_DOLLAR / 10,
        NANOS_PER_DOLLAR / 10..9 * NANOS_PER_DOLLAR / 10,
        1..NANOS_PER_DOLLAR / 10,
        1u64..=1_000,
    )
}

// ---------------------------------------------------------------------------
// Property 1: value conservation across trades, deposits, withdrawals and
// resolution
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum EconAction {
    /// Produce a block from a random (partially crossing) book.
    Block(Vec<OrderSubmission>),
    /// Ingest a valid L1 deposit for `account`.
    Deposit { account: u64, token_units: u64 },
    /// Request a bridge withdrawal (escrows balance until refund/finalize).
    Withdraw {
        account: u64,
        token_units: u64,
        expiry_delta: u64,
    },
    /// Deliver an L1 `Cancelled` event for one of our withdrawals (refund).
    CancelWithdrawal { pick: prop::sample::Index },
    /// Deliver an L1 `Finalized` event for one of our withdrawals (value
    /// permanently leaves L2; later expiry must NOT re-credit).
    FinalizeWithdrawal { pick: prop::sample::Index },
    /// Advance the observed L1 height (expires + refunds old withdrawals).
    ObserveL1 { bump: u64 },
    /// Resolve a market at a fractional payout.
    Resolve { market: u32, payout: u64 },
}

fn arb_econ_action(markets: MarketSet) -> impl Strategy<Value = EconAction> {
    prop_oneof![
        3 => arb_trading_batch(markets).prop_map(EconAction::Block),
        2 => (0..N_ACCOUNTS, 1u64..5_000_000u64)
            .prop_map(|(account, token_units)| EconAction::Deposit { account, token_units }),
        2 => (0..N_ACCOUNTS, 1u64..2_000_000u64, 0u64..4)
            .prop_map(|(account, token_units, expiry_delta)| EconAction::Withdraw {
                account,
                token_units,
                expiry_delta,
            }),
        1 => any::<prop::sample::Index>()
            .prop_map(|pick| EconAction::CancelWithdrawal { pick }),
        1 => any::<prop::sample::Index>()
            .prop_map(|pick| EconAction::FinalizeWithdrawal { pick }),
        2 => (1u64..6).prop_map(|bump| EconAction::ObserveL1 { bump }),
        1 => (0..N_MARKETS, 0..=NANOS_PER_DOLLAR)
            .prop_map(|(market, payout)| EconAction::Resolve { market, payout }),
    ]
}

/// Driver state for the conservation property.
struct ConservationHarness {
    seq: BlockSequencer,
    markets: MarketSet,
    /// D: initial funding + L1 deposits.
    external_in: i64,
    /// W: requested withdrawal amounts not (yet) refunded. Includes finalized
    /// withdrawals — their value left L2 for good.
    withdrawal_escrow: i64,
    /// Cumulative floor-dust allowance: <1 nano per fill settlement, per MINT
    /// adjustment, and per resolution payout leg.
    dust_budget: i64,
    /// Withdrawals we successfully requested, and which ones were refunded.
    requested: Vec<WithdrawalLeaf>,
    refund_credited: HashSet<u64>,
    resolved: HashSet<u32>,
    now_ms: u64,
}

impl ConservationHarness {
    fn new() -> Self {
        let (seq, markets) = make_sequencer();
        Self {
            seq,
            markets,
            external_in: N_ACCOUNTS as i64 * INITIAL_BALANCE,
            withdrawal_escrow: 0,
            dust_budget: 0,
            requested: Vec::new(),
            refund_credited: HashSet::new(),
            resolved: HashSet::new(),
            now_ms: 1_000,
        }
    }

    fn credit_refund(&mut self, leaf: &WithdrawalLeaf) {
        if self.refund_credited.insert(leaf.withdrawal_id) {
            self.withdrawal_escrow -= leaf.amount_nanos as i64;
        }
    }

    fn apply(&mut self, action: EconAction) {
        self.now_ms += 1_000;
        match action {
            EconAction::Block(submissions) => {
                let bp = self.seq.produce_block(submissions, self.now_ms);
                // One floored notional per fill, plus at most one floored MINT
                // adjustment per market.
                self.dust_budget += nonzero_fill_count(&bp.witness) + N_MARKETS as i64;
            }
            EconAction::Deposit {
                account,
                token_units,
            } => {
                let account_id = AccountId(account);
                let bridge = self.seq.bridge_state();
                let mut frontier = bridge.deposit_frontier;
                let pre_count = bridge.deposit_cursor;
                let mut deposit = L1Deposit {
                    deposit_id: pre_count + 1,
                    account_id: Some(account_id),
                    chain_id: 1,
                    vault_address: [0xAA; 20],
                    token_address: [0xBB; 20],
                    sender: [0xCC; 20],
                    sybil_account_key: account_key(account_id),
                    amount_token_units: token_units,
                    deposit_root: [0; 32],
                };
                deposit.deposit_root = append_deposit_frontier(&mut frontier, pre_count, &deposit)
                    .expect("deposit frontier has capacity in tests");
                self.seq
                    .ingest_l1_deposit(deposit)
                    .expect("sequentially valid deposit must be accepted");
                self.external_in += token_units as i64 * 1_000; // NANOS_PER_TOKEN_UNIT
            }
            EconAction::Withdraw {
                account,
                token_units,
                expiry_delta,
            } => {
                let request = BridgeWithdrawalRequest {
                    account_id: AccountId(account),
                    chain_id: 1,
                    vault_address: [0xAA; 20],
                    recipient: [0xDD; 20],
                    token_address: [0xBB; 20],
                    amount_token_units: token_units,
                    expiry_height: self.seq.bridge_state().observed_l1_height + expiry_delta,
                };
                // Rejections (e.g. insufficient available balance) are valid
                // outcomes and must not move value.
                if let Ok(leaf) = self.seq.request_bridge_withdrawal(request) {
                    self.withdrawal_escrow += leaf.amount_nanos as i64;
                    self.requested.push(leaf);
                }
            }
            EconAction::CancelWithdrawal { pick } | EconAction::FinalizeWithdrawal { pick }
                if self.requested.is_empty() =>
            {
                let _ = pick; // nothing to target yet
            }
            EconAction::CancelWithdrawal { pick } => {
                let leaf = &self.requested[pick.index(self.requested.len())];
                let event = BridgeWithdrawalL1Event {
                    nullifier: leaf.nullifier,
                    status: L1WithdrawalStatus::Cancelled,
                    event_at_unix: self.now_ms / 1_000,
                    executable_at_unix: None,
                    tx_hash: None,
                    // Same height: pure state-machine transition, no expiry sweep.
                    l1_block_height: self.seq.bridge_state().observed_l1_height,
                };
                if let Ok(Some(updated)) = self.seq.apply_bridge_withdrawal_l1_event(event)
                    && updated.l1_status == L1WithdrawalStatus::Refunded
                {
                    self.credit_refund(&updated);
                }
            }
            EconAction::FinalizeWithdrawal { pick } => {
                let leaf = &self.requested[pick.index(self.requested.len())];
                let event = BridgeWithdrawalL1Event {
                    nullifier: leaf.nullifier,
                    status: L1WithdrawalStatus::Finalized,
                    event_at_unix: self.now_ms / 1_000,
                    executable_at_unix: None,
                    tx_hash: None,
                    l1_block_height: self.seq.bridge_state().observed_l1_height,
                };
                // Finalized value stays in `withdrawal_escrow`: it left L2 for
                // good. If a later expiry wrongly re-credited it, the defect
                // would jump by the full amount and the check below fails.
                let _ = self.seq.apply_bridge_withdrawal_l1_event(event);
            }
            EconAction::ObserveL1 { bump } => {
                let height = self.seq.bridge_state().observed_l1_height + bump;
                let refunded = self
                    .seq
                    .observe_bridge_l1_height(height)
                    .expect("height observation must not overflow in tests");
                for leaf in refunded {
                    self.credit_refund(&leaf);
                }
            }
            EconAction::Resolve { market, payout } => {
                if self.resolved.contains(&market) {
                    return;
                }
                if self
                    .seq
                    .resolve_market(MarketId::new(market), Nanos(payout), self.now_ms)
                    .is_ok()
                {
                    self.resolved.insert(market);
                    // Two payout legs per account (YES and NO), each floored.
                    self.dust_budget += 2 * (N_ACCOUNTS as i64 + 1);
                }
            }
        }
    }

    fn check(&self) -> Result<(), String> {
        check_value_conservation(
            self.external_in,
            self.withdrawal_escrow,
            total_balance(&self.seq),
            &market_totals(&self.seq, &self.markets),
            self.dust_budget,
        )
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Value conservation. For any interleaving of blocks, L1 deposits,
    /// withdrawal requests, L1 cancel/finalize events, expiry refunds and
    /// market resolutions:
    ///
    ///   external_in == Σ balances (incl. MINT) + $1·Σ outstanding sets
    ///                  + withdrawal escrow          (± floor-dust budget)
    ///
    /// checked after EVERY action; and after resolving all markets and
    /// expiring all withdrawals, the full cycle closes: all remaining value
    /// is cash.
    #[test]
    fn value_conservation_deposits_trades_withdrawals_resolution(
        actions in prop::collection::vec(arb_econ_action(make_markets()), 1..8),
    ) {
        let mut harness = ConservationHarness::new();
        prop_assert!(harness.check().is_ok(), "genesis state must conserve");

        for action in actions {
            harness.apply(action);
            if let Err(violation) = harness.check() {
                return Err(TestCaseError::fail(violation));
            }
        }

        // Close the cycle: resolve everything, expire every open withdrawal.
        for market in 0..N_MARKETS {
            harness.apply(EconAction::Resolve { market, payout: 6 * NANOS_PER_DOLLAR / 10 });
        }
        harness.apply(EconAction::ObserveL1 { bump: 1_000_000 });
        if let Err(violation) = harness.check() {
            return Err(TestCaseError::fail(format!("post-cycle: {violation}")));
        }
        // With every market resolved, no redemption liability may remain.
        for (market, yes, no) in market_totals(&harness.seq, &harness.markets) {
            prop_assert_eq!((yes, no), (0, 0), "resolved market {:?} still has positions", market);
        }
    }
}

// ---------------------------------------------------------------------------
// Property 2: no-arbitrage price coherence
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// No-arbitrage coherence. For any block produced from a random book:
    /// every market that minted complete sets publishes a two-entry price
    /// vector with p_YES + p_NO = $1 (± 2 nanos), all published prices lie in
    /// [0, $1], and every fill respects its order's limit price. (EG duals of
    /// the position-balance constraints; stationarity of the free mint
    /// variable — design/eg-conic.typ §Price Extraction, ADR-0001.)
    #[test]
    fn no_arbitrage_complementary_clearing_prices(
        submissions in arb_trading_batch(make_markets()),
    ) {
        let (mut seq, _) = make_sequencer();
        let bp = seq.produce_block(submissions, 1_000);

        let traded = traded_markets(&bp.witness);
        prop_assert!(!traded.is_empty(), "crossing generator must trade in ≥1 market");

        if let Err(violation) =
            check_no_arbitrage_prices(&bp.block.clearing_prices, &traded)
        {
            return Err(TestCaseError::fail(violation));
        }
        if let Err(violation) =
            check_fill_feasibility(
                &witness_orders(&bp.witness),
                &bp.witness.fills,
                &bp.block.clearing_prices,
            )
        {
            return Err(TestCaseError::fail(violation));
        }
    }
}

// ---------------------------------------------------------------------------
// Property 3: crossing-book non-triviality
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Non-triviality. A random book containing at least one strictly
    /// crossing complementary pair (p_YES + p_NO ≥ $1 + 1%) produces
    /// strictly positive matched volume and strictly positive welfare.
    /// (Anti-regression for the SYB-242 "zero fills" incident: the engine
    /// must never silently stop matching a fundable cross.)
    #[test]
    fn strictly_crossing_book_produces_nonzero_volume(
        submissions in arb_trading_batch(make_markets()),
    ) {
        let (mut seq, _) = make_sequencer();
        let bp = seq.produce_block(submissions, 1_000);

        let volume: u64 = bp
            .block
            .fills
            .iter()
            .filter(|f| f.fill_qty > Qty::ZERO)
            .map(|f| f.fill_qty.0)
            .sum();
        prop_assert!(volume > 0, "strictly crossing book matched zero volume");
        prop_assert!(
            bp.analytics.total_welfare > 0,
            "strictly crossing book realized no welfare (volume={})",
            volume
        );
    }
}

// ---------------------------------------------------------------------------
// Property 4: arrival-order independence
// ---------------------------------------------------------------------------

fn raw_welfare_numerator(witness: &BlockWitness) -> i128 {
    let order_map: HashMap<u64, &Order> = witness
        .orders
        .iter()
        .map(|wo| (wo.order.id, &wo.order))
        .collect();
    witness
        .fills
        .iter()
        .filter_map(|fill| {
            order_map.get(&fill.order_id).map(|order| {
                let surplus_per_unit = if order.is_seller() {
                    fill.fill_price.0 as i128 - order.limit_price.0 as i128
                } else {
                    order.limit_price.0 as i128 - fill.fill_price.0 as i128
                };
                surplus_per_unit * fill.fill_qty.0 as i128
            })
        })
        .sum()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Order-independence (frequent-batch-auction fairness). Shuffling the
    /// arrival order of submissions within one batch leaves the economic
    /// outcome unchanged: the unscaled welfare objective is identical, and
    /// conservation + no-arbitrage hold in both orderings. Byte-level blocks
    /// may differ when the LP has degenerate duals (multiple optimal fill
    /// sets with equal welfare) — that multiplicity is documented in
    /// `invariants.rs::batch_commutativity` and is not an economic
    /// difference.
    #[test]
    fn arrival_order_shuffle_preserves_economic_outcome(
        submissions in arb_trading_batch(make_markets()),
        seed in any::<u64>(),
    ) {
        use rand::seq::SliceRandom;
        use rand::SeedableRng;

        let (mut seq1, markets1) = make_sequencer();
        let bp1 = seq1.produce_block(submissions.clone(), 1_000);

        let mut shuffled = submissions;
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(seed);
        shuffled.shuffle(&mut rng);
        let (mut seq2, markets2) = make_sequencer();
        let bp2 = seq2.produce_block(shuffled, 1_000);

        prop_assert_eq!(
            raw_welfare_numerator(&bp1.witness),
            raw_welfare_numerator(&bp2.witness),
            "arrival order changed the realized welfare objective"
        );

        for (seq, markets, bp, label) in [
            (&seq1, &markets1, &bp1, "original"),
            (&seq2, &markets2, &bp2, "shuffled"),
        ] {
            let dust = nonzero_fill_count(&bp.witness) + N_MARKETS as i64;
            if let Err(violation) = check_value_conservation(
                N_ACCOUNTS as i64 * INITIAL_BALANCE,
                0,
                total_balance(seq),
                &market_totals(seq, markets),
                dust,
            ) {
                return Err(TestCaseError::fail(format!("{label}: {violation}")));
            }
            if let Err(violation) =
                check_no_arbitrage_prices(&bp.block.clearing_prices, &traded_markets(&bp.witness))
            {
                return Err(TestCaseError::fail(format!("{label}: {violation}")));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Property 5: zero-fill idempotence
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Idempotence. A block whose book cannot cross (buys only, every
    /// complementary limit pair sums below $1) fills nothing and is an exact
    /// economic no-op: total cash, every position, the escrow and the
    /// conservation defect are unchanged — with a ZERO dust allowance, since
    /// no fill means no floored conversion. The following (empty) block, in
    /// which the same orders rest, is a no-op again.
    #[test]
    fn zero_fill_block_is_economic_noop(
        submissions in arb_noncrossing_book(make_markets()),
    ) {
        let (mut seq, markets) = make_sequencer();
        let cash_before = total_balance(&seq);

        let bp1 = seq.produce_block(submissions, 1_000);
        let volume: u64 = bp1
            .block
            .fills
            .iter()
            .filter(|f| f.fill_qty > Qty::ZERO)
            .map(|f| f.fill_qty.0)
            .sum();
        prop_assert_eq!(volume, 0, "non-crossing book must not fill");
        prop_assert_eq!(bp1.analytics.total_welfare, 0, "no fill, no welfare");

        for pass in 1..=2u32 {
            prop_assert_eq!(
                total_balance(&seq), cash_before,
                "zero-fill block {} moved cash", pass
            );
            for (market, yes, no) in market_totals(&seq, &markets) {
                prop_assert_eq!(
                    (yes, no), (0, 0),
                    "zero-fill block {} created positions in {:?}", pass, market
                );
            }
            let defect = value_conservation_defect(
                N_ACCOUNTS as i64 * INITIAL_BALANCE,
                0,
                total_balance(&seq),
                &market_totals(&seq, &markets),
            ).map_err(TestCaseError::fail)?;
            prop_assert_eq!(defect, 0, "zero-fill block {} left a conservation defect", pass);

            // Second pass: the unfilled orders rest into the next block.
            if pass == 1 {
                let bp2 = seq.produce_block(vec![], 2_000);
                prop_assert_eq!(
                    nonzero_fill_count(&bp2.witness), 0,
                    "resting non-crossing orders filled in the follow-up block"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Property 6: complete-set mint/burn round trip
// ---------------------------------------------------------------------------

#[test]
fn one_mm_account_can_redeem_its_complete_set_with_paired_sells() {
    let markets = make_markets();
    let market = MarketId::new(0);
    let qty = shares_to_qty(100);
    let face_value = oracle_notional(NANOS_PER_DOLLAR, qty.0).unwrap();
    let initial_cash = 10 * NANOS_PER_DOLLAR as i64;
    let mut accounts = AccountStore::new();
    let mm = accounts.create_account(initial_cash);
    let account = accounts.get_mut(mm).unwrap();
    account.positions.insert((market, 0), qty.0 as i64);
    account.positions.insert((market, 1), qty.0 as i64);
    let mut seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        SequencerConfig {
            min_resting_order_notional_nanos: 0,
            ..SequencerConfig::default()
        },
    );

    let yes_limit = 630_000_000;
    let no_limit = NANOS_PER_DOLLAR - 100_000_000 - yes_limit;
    let production = seq.produce_block(
        vec![OrderSubmission {
            account_id: mm,
            orders: vec![
                outcome_sell(&markets, 0, market, 0, yes_limit, qty.0),
                outcome_sell(&markets, 1, market, 1, no_limit, qty.0),
            ],
            mm_constraint: None,
        }],
        1_000,
    );

    assert!(production.block.rejections.is_empty());
    assert_eq!(production.witness.fills.len(), 2);
    assert!(
        production
            .witness
            .fills
            .iter()
            .all(|fill| fill.fill_qty == qty)
    );
    assert_eq!(production.witness.minting_cost, -face_value);
    let account = seq.accounts.get(mm).unwrap();
    assert_eq!(account.position(market, 0), 0);
    assert_eq!(account.position(market, 1), 0);
    assert_eq!(account.balance, initial_cash + face_value);
    assert!(sybil_verifier::verify_full(&production.witness, false).valid);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Complete-set lifecycle. Two complementary buys first mint a generated
    /// quantity of live positions; the same accounts then sell those holdings
    /// as a complete-set burn. Both price vectors sum to exactly $1, all four
    /// orders fill fully at UCP, signed minting cost changes from +face value
    /// to -face value, reported welfare equals an independent surplus oracle,
    /// and the round trip restores aggregate cash with zero positions.
    #[test]
    fn complete_set_mint_then_burn_round_trip_conserves_value(
        (mint_yes_limit, mint_surplus, burn_yes_limit, burn_surplus, shares)
            in arb_complete_set_cycle(),
    ) {
        let markets = make_markets();
        let market = MarketId::new(0);
        let qty = shares_to_qty(shares);
        let face_value = oracle_notional(NANOS_PER_DOLLAR, qty.0)
            .map_err(TestCaseError::fail)?;
        let initial_balance = face_value
            .checked_add(NANOS_PER_DOLLAR as i64)
            .expect("generated lifecycle balance fits i64");
        let initial_cash = initial_balance
            .checked_mul(2)
            .expect("generated aggregate balance fits i64");

        let mut accounts = AccountStore::new();
        let yes_account = accounts.create_account(initial_balance);
        let no_account = accounts.create_account(initial_balance);
        let mut seq = BlockSequencer::with_default_solver(
            accounts,
            markets.clone(),
            vec![],
            SequencerConfig {
                min_resting_order_notional_nanos: 0,
                ..SequencerConfig::default()
            },
        );

        let mint_no_limit = NANOS_PER_DOLLAR - mint_yes_limit + mint_surplus;
        let mint = seq.produce_block(
            vec![
                OrderSubmission {
                    account_id: yes_account,
                    orders: vec![outcome_buy(
                        &markets,
                        0,
                        market,
                        0,
                        mint_yes_limit,
                        qty.0,
                    )],
                    mm_constraint: None,
                },
                OrderSubmission {
                    account_id: no_account,
                    orders: vec![outcome_buy(
                        &markets,
                        0,
                        market,
                        1,
                        mint_no_limit,
                        qty.0,
                    )],
                    mm_constraint: None,
                },
            ],
            1_000,
        );
        let mint_orders = witness_orders(&mint.witness);
        check_fill_feasibility(
            &mint_orders,
            &mint.witness.fills,
            &mint.block.clearing_prices,
        )
        .map_err(TestCaseError::fail)?;
        check_no_arbitrage_prices(
            &mint.block.clearing_prices,
            &traded_markets(&mint.witness),
        )
        .map_err(TestCaseError::fail)?;
        let mint_prices = mint
            .block
            .clearing_prices
            .get(&market)
            .expect("mint block must publish its traded market");
        prop_assert_eq!(
            mint_prices[0].0 + mint_prices[1].0,
            NANOS_PER_DOLLAR,
            "mint UCP vector must sum exactly to face value"
        );
        prop_assert_eq!(mint.witness.fills.len(), 2, "both mint orders must fill");
        for fill in &mint.witness.fills {
            prop_assert_eq!(fill.fill_qty, qty, "mint order did not fill fully");
        }
        prop_assert_eq!(
            mint.witness.minting_cost,
            face_value,
            "complete-set creation must have positive face-value cost"
        );
        let mint_surplus_oracle =
            oracle_fill_surplus(&mint_orders, &mint.witness.fills).map_err(TestCaseError::fail)?;
        prop_assert_eq!(mint.witness.total_welfare, mint_surplus_oracle);
        prop_assert_eq!(mint.analytics.total_welfare, mint_surplus_oracle);
        prop_assert_eq!(
            mint.analytics.welfare_by_market.get(&market),
            Some(&mint_surplus_oracle)
        );
        prop_assert!(sybil_verifier::verify_full(&mint.witness, false).valid);
        prop_assert_eq!(seq.accounts.get(yes_account).unwrap().position(market, 0), qty.0 as i64);
        prop_assert_eq!(seq.accounts.get(no_account).unwrap().position(market, 1), qty.0 as i64);
        check_value_conservation(
            initial_cash,
            0,
            total_balance(&seq),
            &market_totals(&seq, &markets),
            0,
        )
        .map_err(TestCaseError::fail)?;

        let burn_no_limit = NANOS_PER_DOLLAR - burn_yes_limit - burn_surplus;
        let burn = seq.produce_block(
            vec![
                OrderSubmission {
                    account_id: yes_account,
                    orders: vec![outcome_sell(
                        &markets,
                        0,
                        market,
                        0,
                        burn_yes_limit,
                        qty.0,
                    )],
                    mm_constraint: None,
                },
                OrderSubmission {
                    account_id: no_account,
                    orders: vec![outcome_sell(
                        &markets,
                        0,
                        market,
                        1,
                        burn_no_limit,
                        qty.0,
                    )],
                    mm_constraint: None,
                },
            ],
            2_000,
        );
        let burn_orders = witness_orders(&burn.witness);
        check_fill_feasibility(
            &burn_orders,
            &burn.witness.fills,
            &burn.block.clearing_prices,
        )
        .map_err(TestCaseError::fail)?;
        check_no_arbitrage_prices(
            &burn.block.clearing_prices,
            &traded_markets(&burn.witness),
        )
        .map_err(TestCaseError::fail)?;
        let burn_prices = burn
            .block
            .clearing_prices
            .get(&market)
            .expect("burn block must publish its traded market");
        prop_assert_eq!(
            burn_prices[0].0 + burn_prices[1].0,
            NANOS_PER_DOLLAR,
            "burn UCP vector must sum exactly to face value"
        );
        prop_assert_eq!(burn.witness.fills.len(), 2, "both burn orders must fill");
        for fill in &burn.witness.fills {
            prop_assert_eq!(fill.fill_qty, qty, "burn order did not fill fully");
        }
        prop_assert_eq!(
            burn.witness.minting_cost,
            -face_value,
            "complete-set burning must have negative face-value cost"
        );
        let burn_surplus_oracle =
            oracle_fill_surplus(&burn_orders, &burn.witness.fills).map_err(TestCaseError::fail)?;
        prop_assert_eq!(burn.witness.total_welfare, burn_surplus_oracle);
        prop_assert_eq!(burn.analytics.total_welfare, burn_surplus_oracle);
        prop_assert_eq!(
            burn.analytics.welfare_by_market.get(&market),
            Some(&burn_surplus_oracle)
        );
        prop_assert!(sybil_verifier::verify_full(&burn.witness, false).valid);
        prop_assert_eq!(seq.accounts.get(yes_account).unwrap().position(market, 0), 0);
        prop_assert_eq!(seq.accounts.get(no_account).unwrap().position(market, 1), 0);
        for (settled_market, yes, no) in market_totals(&seq, &markets) {
            prop_assert_eq!(
                (yes, no),
                (0, 0),
                "round trip left positions in market {:?}",
                settled_market
            );
        }
        prop_assert_eq!(
            total_balance(&seq),
            initial_cash,
            "mint/burn round trip must restore aggregate cash exactly"
        );
        check_value_conservation(
            initial_cash,
            0,
            total_balance(&seq),
            &market_totals(&seq, &markets),
            0,
        )
        .map_err(TestCaseError::fail)?;
    }
}

// ---------------------------------------------------------------------------
// Falsifiability: the checkers reject a broken solver (SYB-246 acceptance)
// ---------------------------------------------------------------------------

/// One honest block from a deterministic crossing pair, with everything the
/// post-hoc checkers need.
struct HonestBlock {
    pre: LedgerSnapshot,
    orders: Vec<(Order, u64)>,
    fills: Vec<Fill>,
    clearing_prices: HashMap<MarketId, Vec<Nanos>>,
    markets: MarketSet,
    dust: i64,
}

fn honest_block() -> HonestBlock {
    let (mut seq, markets) = make_sequencer();
    let m0 = MarketId::new(0);
    let qty = shares_to_qty(10).0;
    let pre = snapshot_ledger(&seq, &markets);

    let bp = seq.produce_block(
        vec![
            OrderSubmission {
                account_id: AccountId(0),
                orders: vec![outcome_buy(
                    &markets,
                    0,
                    m0,
                    0,
                    55 * NANOS_PER_DOLLAR / 100,
                    qty,
                )],
                mm_constraint: None,
            },
            OrderSubmission {
                account_id: AccountId(1),
                orders: vec![outcome_buy(
                    &markets,
                    0,
                    m0,
                    1,
                    55 * NANOS_PER_DOLLAR / 100,
                    qty,
                )],
                mm_constraint: None,
            },
        ],
        1_000,
    );
    assert!(
        nonzero_fill_count(&bp.witness) > 0,
        "honest crossing pair must fill"
    );
    HonestBlock {
        pre,
        orders: witness_orders(&bp.witness),
        fills: bp.witness.fills.clone(),
        clearing_prices: bp.block.clearing_prices.clone(),
        markets,
        dust: nonzero_fill_count(&bp.witness) + N_MARKETS as i64,
    }
}

fn conservation_verdict(
    pre: &LedgerSnapshot,
    orders: &[(Order, u64)],
    fills: &[Fill],
    clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
    markets: &MarketSet,
    dust: i64,
) -> Result<(), String> {
    let (balance_total, totals) =
        replay_claimed_block(pre, orders, fills, clearing_prices, markets)?;
    check_value_conservation(
        N_ACCOUNTS as i64 * INITIAL_BALANCE,
        0,
        balance_total,
        &totals,
        dust,
    )
}

/// A solver that reports inflated fill prices silently destroys buyer cash.
/// Replaying the perturbed fills through the SAME settlement math and
/// conservation checker used by the live property must reject them, while
/// the honest fills pass. This is the falsifiability demonstration required
/// by SYB-246: no `src/` change, purely post-hoc fill perturbation.
#[test]
fn falsifiability_inflated_fill_price_breaks_conservation() {
    let hb = honest_block();

    conservation_verdict(
        &hb.pre,
        &hb.orders,
        &hb.fills,
        &hb.clearing_prices,
        &hb.markets,
        hb.dust,
    )
    .expect("honest block must satisfy value conservation");

    let mut broken = hb.fills.clone();
    let victim = broken
        .iter_mut()
        .find(|f| f.fill_qty > Qty::ZERO)
        .expect("honest block has a fill");
    victim.fill_price = Nanos(victim.fill_price.0 + 5 * NANOS_PER_DOLLAR / 100);

    let verdict = conservation_verdict(
        &hb.pre,
        &hb.orders,
        &broken,
        &hb.clearing_prices,
        &hb.markets,
        hb.dust,
    );
    assert!(
        verdict.is_err(),
        "conservation checker failed to reject a +5% fill-price perturbation"
    );
}

/// The mirror image: deflated fill prices fabricate buyer cash. Also caught.
#[test]
fn falsifiability_deflated_fill_price_breaks_conservation() {
    let hb = honest_block();

    let mut broken = hb.fills.clone();
    let victim = broken
        .iter_mut()
        .find(|f| f.fill_qty > Qty::ZERO)
        .expect("honest block has a fill");
    victim.fill_price = Nanos(victim.fill_price.0 - 5 * NANOS_PER_DOLLAR / 100);

    let verdict = conservation_verdict(
        &hb.pre,
        &hb.orders,
        &broken,
        &hb.clearing_prices,
        &hb.markets,
        hb.dust,
    );
    assert!(
        verdict.is_err(),
        "conservation checker failed to reject a -5% fill-price perturbation"
    );
}

/// A solver publishing incoherent complementary prices (p_YES + p_NO ≠ $1)
/// creates a mint/redeem arbitrage. The SAME no-arbitrage checker used by
/// the live property must reject the perturbed prices while accepting the
/// honest ones.
#[test]
fn falsifiability_incoherent_clearing_prices_break_no_arbitrage() {
    let hb = honest_block();
    let minted: HashSet<MarketId> = hb.clearing_prices.keys().copied().collect();

    check_no_arbitrage_prices(&hb.clearing_prices, &minted)
        .expect("honest clearing prices must be arbitrage-free");
    check_fill_feasibility(&hb.orders, &hb.fills, &hb.clearing_prices)
        .expect("honest fills must be feasible at UCP");

    let mut broken = hb.clearing_prices.clone();
    let market_prices = broken
        .values_mut()
        .next()
        .expect("honest block published prices");
    market_prices[0] = Nanos(market_prices[0].0 + 5 * NANOS_PER_DOLLAR / 100);

    assert!(
        check_no_arbitrage_prices(&broken, &minted).is_err(),
        "no-arbitrage checker failed to reject p_YES + p_NO = $1.05"
    );
}

/// A solver filling a buyer above their limit extracts non-consensual value.
/// The rationality checker must reject it.
#[test]
fn falsifiability_limit_violating_fill_breaks_rationality() {
    let hb = honest_block();

    let mut broken = hb.fills.clone();
    let victim = broken
        .iter_mut()
        .find(|f| f.fill_qty > Qty::ZERO)
        .expect("honest block has a fill");
    let limit = hb
        .orders
        .iter()
        .find(|(o, _)| o.id == victim.order_id)
        .map(|(o, _)| o.limit_price)
        .expect("fill references a known order");
    victim.fill_price = Nanos(limit.0 + 1);

    assert!(
        check_fill_feasibility(&hb.orders, &broken, &hb.clearing_prices).is_err(),
        "rationality checker failed to reject a fill 1 nano above the buyer's limit"
    );
}

/// A favorable but non-UCP execution is still invalid: uniform clearing is a
/// market-level fairness guarantee, not merely an individual limit check.
#[test]
fn falsifiability_non_ucp_fill_breaks_uniform_clearing() {
    let hb = honest_block();
    check_fill_feasibility(&hb.orders, &hb.fills, &hb.clearing_prices)
        .expect("honest fills must be feasible at UCP");

    let mut broken = hb.fills.clone();
    let victim = broken
        .iter_mut()
        .find(|fill| fill.fill_qty > Qty::ZERO)
        .expect("honest block has a fill");
    victim.fill_price = if victim.fill_price.0 > 0 {
        Nanos(victim.fill_price.0 - 1)
    } else {
        Nanos(1)
    };

    assert!(
        check_fill_feasibility(&hb.orders, &broken, &hb.clearing_prices).is_err(),
        "feasibility checker failed to reject a favorable non-UCP fill"
    );
}

/// Quantity bounds are economic authorization: a solver may not consume more
/// balance or inventory than the participant offered.
#[test]
fn falsifiability_overfill_breaks_fill_feasibility() {
    let hb = honest_block();
    let mut broken = hb.fills.clone();
    let victim = broken
        .iter_mut()
        .find(|fill| fill.fill_qty > Qty::ZERO)
        .expect("honest block has a fill");
    let max_fill = hb
        .orders
        .iter()
        .find(|(order, _)| order.id == victim.order_id)
        .map(|(order, _)| order.max_fill)
        .expect("fill references a known order");
    victim.fill_qty = Qty(max_fill.0 + 1);

    assert!(
        check_fill_feasibility(&hb.orders, &broken, &hb.clearing_prices).is_err(),
        "feasibility checker failed to reject a one-unit overfill"
    );
}
