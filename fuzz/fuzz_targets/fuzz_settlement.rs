#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use matching_engine::{Fill, MarketId, Nanos, Order, Qty, NANOS_PER_DOLLAR};
use matching_sequencer::Account;
use matching_sequencer::account::AccountId;

/// Fuzzable input for settlement.
#[derive(Debug, Arbitrary)]
struct SettlementInput {
    balance: i64,
    num_markets: u8,      // 1..=5
    payoffs_raw: [i8; 32],
    limit_price_frac: u64, // will be clamped to NANOS_PER_DOLLAR
    fill_qty: u64,
    fill_price_frac: u64,  // will be clamped to NANOS_PER_DOLLAR
    // Pre-existing positions for sell testing
    yes_position: i64,
    no_position: i64,
}

fuzz_target!(|input: SettlementInput| {
    // Clamp to valid ranges
    let num_markets = (input.num_markets % 5).max(1) as usize;
    let num_states = 1usize << num_markets;
    let limit_price = input.limit_price_frac % (NANOS_PER_DOLLAR + 1);
    let fill_price = input.fill_price_frac % (NANOS_PER_DOLLAR + 1);
    let fill_qty = input.fill_qty % 1_000_000; // Keep fills reasonable

    if fill_qty == 0 {
        return;
    }

    // Build order
    let mut order = Order::new(1);
    for i in 0..num_markets {
        order.markets[i] = MarketId::new(i as u32);
    }
    order.num_markets = num_markets as u8;
    order.num_states = num_states as u8;
    for i in 0..num_states {
        order.payoffs[i] = input.payoffs_raw[i];
    }
    order.limit_price = Nanos(limit_price);
    order.max_fill = Qty(fill_qty);

    // Build account
    let mut account = Account::new(AccountId(0), input.balance);
    let m0 = MarketId::new(0);
    if input.yes_position != 0 {
        account.positions.insert((m0, 0), input.yes_position);
    }
    if input.no_position != 0 {
        account.positions.insert((m0, 1), input.no_position);
    }

    let fill = Fill::new(1, Qty(fill_qty), Nanos(fill_price));

    // Must not panic
    matching_sequencer::settlement::settle_fill(&mut account, &order, &fill);
});
