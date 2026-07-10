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
//!
//! Falsifiability (SYB-246 acceptance): the deterministic
//! `falsifiability_*` tests feed post-hoc perturbed fills/prices from a real
//! block through the *same* checker functions used by the properties above
//! and assert the checkers reject them. This demonstrates the properties
//! would fail on a broken solver without modifying `src/`.
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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use matching_engine::{
    compute_fill_settlement, derive_minting, notional_nanos, outcome_buy, shares_to_qty, Fill,
    MarketId, MarketSet, Nanos, Order, Qty, NANOS_PER_DOLLAR,
};
use matching_sequencer::bridge::{account_key, append_deposit_frontier};
use matching_sequencer::{
    AccountId, AccountStore, AdminOracle, BlockSequencer, BridgeWithdrawalL1Event,
    BridgeWithdrawalRequest, L1Deposit, L1WithdrawalStatus, OrderSubmission, SequencerConfig,
    WithdrawalLeaf,
};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
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
    let oracle = Arc::new(AdminOracle::new());
    let seq = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        oracle,
        SequencerConfig::default(),
    );
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
        escrow_value += notional_nanos(Nanos(NANOS_PER_DOLLAR), Qty(yes as u64)).0 as i64;
    }
    Ok(external_in - withdrawal_escrow - balance_total - escrow_value)
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

/// No-arbitrage price coherence (EG/Fisher, `design/eg-conic.typ` §Price
/// Extraction): every published binary price vector has exactly two entries
/// in [0, $1], and for every market in `minted_markets` (markets where the
/// block created complete sets, i.e. minting was active) the complementary
/// prices satisfy p_YES + p_NO = $1 up to `PRICE_COHERENCE_DUST` nanos —
/// stationarity of the free mint variable. If this failed, minting a set for
/// p_YES + p_NO ≠ $1 and redeeming it for exactly $1 would be an arbitrage.
fn check_no_arbitrage_prices(
    clearing_prices: &HashMap<MarketId, Vec<Nanos>>,
    minted_markets: &HashSet<MarketId>,
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
        if minted_markets.contains(market) {
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
    for market in minted_markets {
        if !clearing_prices.contains_key(market) {
            return Err(format!(
                "market {market:?} minted sets but published no clearing prices"
            ));
        }
    }
    Ok(())
}

/// Individual rationality: every non-zero fill executes within its order's
/// limit (buyers never pay more, sellers never receive less) and at a price
/// of at most $1. A fill outside the limit extracts value from a participant
/// who never consented to it — an arbitrage against the book.
fn check_fill_rationality(orders: &[(Order, u64)], fills: &[Fill]) -> Result<(), String> {
    let order_map: HashMap<u64, &Order> = orders.iter().map(|(o, _)| (o.id, o)).collect();
    for fill in fills {
        if fill.fill_qty == Qty::ZERO {
            continue;
        }
        let Some(order) = order_map.get(&fill.order_id) else {
            return Err(format!("fill references unknown order {}", fill.order_id));
        };
        if fill.fill_price.0 > NANOS_PER_DOLLAR {
            return Err(format!(
                "order {} filled at {} > $1",
                fill.order_id, fill.fill_price
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
    }
    Ok(())
}

/// Witness orders as `(Order, account_id)` pairs for the checkers above.
fn witness_orders(witness: &BlockWitness) -> Vec<(Order, u64)> {
    witness
        .orders
        .iter()
        .map(|wo| (wo.order.clone(), wo.account_id))
        .collect()
}

/// Markets in which a block's non-zero fills created complete sets. With the
/// buy-only generators below every non-zero fill mints, so this is simply
/// "markets with a non-zero fill".
fn minted_markets(witness: &BlockWitness) -> HashSet<MarketId> {
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

/// Re-derive the post-block ledger a *claimed* set of fills implies, using
/// the shared settlement math (`compute_fill_settlement` + `derive_minting`)
/// exactly like the verifier does. Returns `(Σ balances, market totals)`
/// ready for [`value_conservation_defect`]. This lets the falsifiability
/// tests run perturbed fills/prices through the same checkers the live
/// properties use, without touching sequencer state or `src/`.
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
        let Some(delta) = compute_fill_settlement(order, fill) else {
            continue;
        };
        let entry = ledger.entry(account_id).or_default();
        entry.0 += delta.balance_delta;
        for (market, outcome, qty_delta) in delta.position_deltas {
            *entry.1.entry((market, outcome)).or_insert(0) += qty_delta;
        }
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

    // MINT absorbs any imbalance, exactly as the sequencer settles it.
    let mint_adjustments = derive_minting(&totals, clearing_prices);
    let mint = ledger.entry(AccountId::MINT.0).or_default();
    for adjustment in &mint_adjustments {
        mint.0 += adjustment.balance_delta;
        *mint
            .1
            .entry((adjustment.market_id, adjustment.outcome))
            .or_insert(0) += adjustment.position_delta;
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
                if let Ok(Some(updated)) = self.seq.apply_bridge_withdrawal_l1_event(event) {
                    if updated.l1_status == L1WithdrawalStatus::Refunded {
                        self.credit_refund(&updated);
                    }
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

        let minted = minted_markets(&bp.witness);
        prop_assert!(!minted.is_empty(), "crossing generator must mint in ≥1 market");

        if let Err(violation) =
            check_no_arbitrage_prices(&bp.block.clearing_prices, &minted)
        {
            return Err(TestCaseError::fail(violation));
        }
        if let Err(violation) =
            check_fill_rationality(&witness_orders(&bp.witness), &bp.witness.fills)
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
                check_no_arbitrage_prices(&bp.block.clearing_prices, &minted_markets(&bp.witness))
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
    check_fill_rationality(&hb.orders, &hb.fills).expect("honest fills must respect limits");

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
        check_fill_rationality(&hb.orders, &broken).is_err(),
        "rationality checker failed to reject a fill 1 nano above the buyer's limit"
    );
}
