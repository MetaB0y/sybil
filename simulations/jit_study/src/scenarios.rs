use rust_decimal_macros::dec;
use crate::types::*;

// =============================================================================
// SCENARIO CATEGORIES
// =============================================================================
//
// 1. MARKET BALANCE: How does buy/sell pressure affect JIT?
//    - balanced, imbalanced_buyers, imbalanced_sellers
//
// 2. MARKET DEPTH: How does liquidity depth affect JIT?
//    - thin_market, deep_market
//
// 3. SPREAD: How does bid-ask spread affect JIT?
//    - tight_spread, wide_spread
//
// 4. INFORMATION: How does informed trading affect JIT?
//    - uninformed_only, one_informed, all_informed
//
// 5. ORDER HETEROGENEITY: Different order sizes
//    - uniform_sizes, whale_and_retail
//
// =============================================================================

// -----------------------------------------------------------------------------
// 1. MARKET BALANCE
// -----------------------------------------------------------------------------

/// Balanced: equal buy and sell pressure, market clears fully
pub fn balanced() -> Scenario {
    Scenario {
        name: "Balanced market",
        orders: vec![
            Order::buy(1, dec!(100), dec!(0.55)),
            Order::buy(2, dec!(100), dec!(0.52)),
            Order::sell(3, dec!(100), dec!(0.45)),
            Order::sell(4, dec!(100), dec!(0.48)),
        ],
        true_value: dec!(0.50),
    }
}

/// More buyers than sellers - excess demand
pub fn imbalanced_buyers() -> Scenario {
    Scenario {
        name: "More buyers",
        orders: vec![
            Order::buy(1, dec!(200), dec!(0.58)),
            Order::buy(2, dec!(150), dec!(0.55)),
            Order::buy(3, dec!(100), dec!(0.52)),
            Order::sell(4, dec!(100), dec!(0.45)),
            Order::sell(5, dec!(80), dec!(0.48)),
        ],
        true_value: dec!(0.50),
    }
}

/// More sellers than buyers - excess supply
pub fn imbalanced_sellers() -> Scenario {
    Scenario {
        name: "More sellers",
        orders: vec![
            Order::buy(1, dec!(100), dec!(0.55)),
            Order::buy(2, dec!(80), dec!(0.52)),
            Order::sell(3, dec!(200), dec!(0.42)),
            Order::sell(4, dec!(150), dec!(0.45)),
            Order::sell(5, dec!(100), dec!(0.48)),
        ],
        true_value: dec!(0.50),
    }
}

// -----------------------------------------------------------------------------
// 2. MARKET DEPTH
// -----------------------------------------------------------------------------

/// Thin market: few orders, single trade possible
pub fn thin_market() -> Scenario {
    Scenario {
        name: "Thin market",
        orders: vec![
            Order::buy(1, dec!(50), dec!(0.55)),
            Order::sell(2, dec!(30), dec!(0.45)),
        ],
        true_value: dec!(0.50),
    }
}

/// Deep market: many orders at various price levels
pub fn deep_market() -> Scenario {
    Scenario {
        name: "Deep market",
        orders: vec![
            // Buyers at various levels
            Order::buy(1, dec!(50), dec!(0.58)),
            Order::buy(2, dec!(80), dec!(0.56)),
            Order::buy(3, dec!(100), dec!(0.54)),
            Order::buy(4, dec!(120), dec!(0.52)),
            Order::buy(5, dec!(80), dec!(0.50)),
            // Sellers at various levels
            Order::sell(6, dec!(50), dec!(0.42)),
            Order::sell(7, dec!(80), dec!(0.44)),
            Order::sell(8, dec!(100), dec!(0.46)),
            Order::sell(9, dec!(120), dec!(0.48)),
            Order::sell(10, dec!(80), dec!(0.50)),
        ],
        true_value: dec!(0.50),
    }
}

// -----------------------------------------------------------------------------
// 3. SPREAD
// -----------------------------------------------------------------------------

/// Tight spread: orders clustered near true value
pub fn tight_spread() -> Scenario {
    Scenario {
        name: "Tight spread",
        orders: vec![
            Order::buy(1, dec!(100), dec!(0.51)),
            Order::buy(2, dec!(100), dec!(0.505)),
            Order::sell(3, dec!(100), dec!(0.49)),
            Order::sell(4, dec!(100), dec!(0.495)),
        ],
        true_value: dec!(0.50),
    }
}

/// Wide spread: big gap, no natural crossing
pub fn wide_spread() -> Scenario {
    Scenario {
        name: "Wide spread (no crossing)",
        orders: vec![
            Order::buy(1, dec!(100), dec!(0.40)),
            Order::buy(2, dec!(50), dec!(0.38)),
            Order::sell(3, dec!(100), dec!(0.60)),
            Order::sell(4, dec!(50), dec!(0.62)),
        ],
        true_value: dec!(0.50),
    }
}

// -----------------------------------------------------------------------------
// 4. INFORMATION ASYMMETRY
// -----------------------------------------------------------------------------

/// All uninformed: orders randomly distributed around true value
pub fn uninformed_only() -> Scenario {
    Scenario {
        name: "Uninformed traders only",
        orders: vec![
            // Random noise around 0.50
            Order::buy(1, dec!(60), dec!(0.53)),
            Order::buy(2, dec!(80), dec!(0.51)),
            Order::buy(3, dec!(40), dec!(0.48)), // "wrong" - below true value
            Order::sell(4, dec!(70), dec!(0.47)),
            Order::sell(5, dec!(50), dec!(0.52)), // "wrong" - above true value
            Order::sell(6, dec!(60), dec!(0.49)),
        ],
        true_value: dec!(0.50),
    }
}

/// One informed trader with large order
/// True value is 0.60 but market thinks it's 0.50
/// Informed buyer knows it's worth 0.60, buys aggressively
pub fn one_informed_buyer() -> Scenario {
    Scenario {
        name: "One informed buyer (TV=0.60)",
        orders: vec![
            // Uninformed (pricing around 0.50)
            Order::buy(1, dec!(50), dec!(0.52)),
            Order::buy(2, dec!(50), dec!(0.48)),
            Order::sell(3, dec!(100), dec!(0.48)),
            Order::sell(4, dec!(80), dec!(0.52)),
            // Informed buyer (knows TV=0.60, willing to pay up to 0.58)
            Order::buy(5, dec!(300), dec!(0.58)),
        ],
        true_value: dec!(0.60),
    }
}

/// One informed seller
/// True value is 0.40 but market thinks it's 0.50
/// Informed seller knows it's worth 0.40, sells aggressively
pub fn one_informed_seller() -> Scenario {
    Scenario {
        name: "One informed seller (TV=0.40)",
        orders: vec![
            // Uninformed (pricing around 0.50)
            Order::buy(1, dec!(100), dec!(0.52)),
            Order::buy(2, dec!(80), dec!(0.48)),
            Order::sell(3, dec!(50), dec!(0.48)),
            Order::sell(4, dec!(50), dec!(0.52)),
            // Informed seller (knows TV=0.40, willing to sell down to 0.42)
            Order::sell(5, dec!(300), dec!(0.42)),
        ],
        true_value: dec!(0.40),
    }
}

// -----------------------------------------------------------------------------
// 5. ORDER HETEROGENEITY
// -----------------------------------------------------------------------------

/// Uniform: all orders same size
pub fn uniform_sizes() -> Scenario {
    Scenario {
        name: "Uniform order sizes",
        orders: vec![
            Order::buy(1, dec!(100), dec!(0.55)),
            Order::buy(2, dec!(100), dec!(0.52)),
            Order::buy(3, dec!(100), dec!(0.50)),
            Order::sell(4, dec!(100), dec!(0.45)),
            Order::sell(5, dec!(100), dec!(0.48)),
            Order::sell(6, dec!(100), dec!(0.50)),
        ],
        true_value: dec!(0.50),
    }
}

/// Whale + retail: one large order, many small orders
pub fn whale_and_retail() -> Scenario {
    Scenario {
        name: "Whale + retail",
        orders: vec![
            // Small retail orders
            Order::buy(1, dec!(10), dec!(0.55)),
            Order::buy(2, dec!(15), dec!(0.53)),
            Order::buy(3, dec!(20), dec!(0.51)),
            Order::sell(4, dec!(10), dec!(0.45)),
            Order::sell(5, dec!(15), dec!(0.47)),
            Order::sell(6, dec!(20), dec!(0.49)),
            // Whale
            Order::buy(7, dec!(500), dec!(0.54)),
        ],
        true_value: dec!(0.50),
    }
}

// -----------------------------------------------------------------------------
// ALL SCENARIOS
// -----------------------------------------------------------------------------

pub fn all_scenarios() -> Vec<Scenario> {
    vec![
        balanced(),
        imbalanced_buyers(),
        imbalanced_sellers(),
        thin_market(),
        deep_market(),
        tight_spread(),
        wide_spread(),
        uninformed_only(),
        one_informed_buyer(),
        one_informed_seller(),
        uniform_sizes(),
        whale_and_retail(),
    ]
}
