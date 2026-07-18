//! Canonical integer clearing-price selection.
//!
//! The retained-cash allocation can expose a face of equally supporting
//! zero-temperature prices. Numerical solver duals are proposals only: this
//! module reconstructs the supported face from landed quantities and selects
//! its maximum-entropy integer point without floating point.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    ConditionDir, Fill, MarketGroup, MarketId, MmConstraint, MmSide, NANOS_PER_DOLLAR, Nanos,
    Order, SHARE_SCALE,
};

/// Inclusive integer support for one market's YES price.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalPriceSupport {
    pub lower_yes: Nanos,
    pub upper_yes: Nanos,
}

/// Canonical prices plus the final support intervals used to select them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalPriceSelection {
    pub prices: BTreeMap<MarketId, Vec<Nanos>>,
    pub support: BTreeMap<MarketId, CanonicalPriceSupport>,
}

/// A landed allocation for which no canonical retained-cash price can be
/// established.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalPriceError {
    InvalidInput(String),
    UnsupportedOrder { order_id: u64, reason: String },
    EmptyPriceFace { market: Option<MarketId> },
}

impl std::fmt::Display for CanonicalPriceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(reason) => write!(f, "invalid canonical-price input: {reason}"),
            Self::UnsupportedOrder { order_id, reason } => {
                write!(
                    f,
                    "order {order_id} is unsupported for canonical pricing: {reason}"
                )
            }
            Self::EmptyPriceFace {
                market: Some(market),
            } => {
                write!(f, "market {market} has an empty canonical price face")
            }
            Self::EmptyPriceFace { market: None } => {
                write!(f, "a market group has an empty canonical price face")
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    lower: u64,
    upper: u64,
}

impl Bounds {
    const UNIT: Self = Self {
        lower: 0,
        upper: NANOS_PER_DOLLAR,
    };

    fn intersect(&mut self, lower: u64, upper: u64) -> bool {
        self.lower = self.lower.max(lower);
        self.upper = self.upper.min(upper);
        self.lower <= self.upper
    }

    fn pin(&mut self, value: u64) -> bool {
        self.intersect(value, value)
    }
}

#[derive(Clone, Copy, Debug)]
struct RationalPrice {
    numerator: u128,
    denominator: u128,
}

impl RationalPrice {
    fn integer(value: u64) -> Self {
        Self {
            numerator: value as u128,
            denominator: 1,
        }
    }

    fn floor(self) -> u64 {
        (self.numerator / self.denominator).min(NANOS_PER_DOLLAR as u128) as u64
    }

    fn ceil(self) -> u64 {
        self.numerator
            .div_ceil(self.denominator)
            .min(NANOS_PER_DOLLAR as u128) as u64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum YesDirection {
    /// The order wants a lower YES price: BuyYes or SellNo.
    WantsLow,
    /// The order wants a higher YES price: SellYes or BuyNo.
    WantsHigh,
}

#[derive(Clone, Copy, Debug)]
struct EffectiveOrder {
    market: MarketId,
    direction: YesDirection,
    literal_threshold: u64,
    effective_threshold: RationalPrice,
}

#[derive(Clone, Debug)]
struct MarketFace {
    base: Bounds,
    residual: Bounds,
    yes_demand: i128,
    no_demand: i128,
}

impl Default for MarketFace {
    fn default() -> Self {
        Self {
            base: Bounds::UNIT,
            residual: Bounds::UNIT,
            yes_demand: 0,
            no_demand: 0,
        }
    }
}

#[derive(Clone, Debug)]
struct PriceComponent {
    markets: Vec<MarketId>,
    /// A positive active group maximum consumes the complete simplex. Other
    /// components retain an implicit complementary outcome.
    exact_sum: bool,
}

struct ComponentSelection {
    prices: BTreeMap<MarketId, u64>,
    support: BTreeMap<MarketId, CanonicalPriceSupport>,
}

/// Recompute the canonical clearing price for a landed allocation.
///
/// Input fill prices are deliberately ignored. Only integer fill quantities
/// participate, so a verifier can independently derive the result.
pub fn canonical_clearing_prices(
    orders: &[Order],
    fills: &[Fill],
    mm_constraints: &[MmConstraint],
    market_groups: &[MarketGroup],
) -> Result<CanonicalPriceSelection, CanonicalPriceError> {
    let order_map = canonical_order_map(orders)?;
    let fill_quantities = canonical_fill_quantities(fills, &order_map)?;
    let mm_by_order = canonical_mm_membership(mm_constraints, &order_map)?;
    let pacing = mm_pacing_factors(mm_constraints, &fill_quantities, &order_map)?;

    let mut faces: BTreeMap<MarketId, MarketFace> = BTreeMap::new();
    for order in orders {
        let effective = effective_order(order, mm_by_order.get(&order.id), &pacing)?;
        let quantity = fill_quantities.get(&order.id).copied().unwrap_or(0);
        // An unfilled conditional order need not be active on the landed
        // allocation's price face. A positive fill fixes the active branch;
        // that branch then contributes both its ordinary KKT bounds and its
        // strict integer activation bound.
        if quantity == 0 && order.condition.is_some() {
            continue;
        }
        let face = faces.entry(effective.market).or_default();

        let literal = effective.literal_threshold;
        let effective_floor = effective.effective_threshold.floor();
        let effective_ceil = effective.effective_threshold.ceil();
        if quantity > 0 {
            let supported = match effective.direction {
                YesDirection::WantsLow => face.base.intersect(0, literal),
                YesDirection::WantsHigh => face.base.intersect(literal, NANOS_PER_DOLLAR),
            };
            if !supported {
                return Err(CanonicalPriceError::EmptyPriceFace {
                    market: Some(effective.market),
                });
            }
            face.yes_demand = face
                .yes_demand
                .checked_add(order.payoffs[0] as i128 * quantity as i128)
                .ok_or_else(|| {
                    CanonicalPriceError::InvalidInput("YES demand overflow".to_string())
                })?;
            face.no_demand = face
                .no_demand
                .checked_add(order.payoffs[1] as i128 * quantity as i128)
                .ok_or_else(|| {
                    CanonicalPriceError::InvalidInput("NO demand overflow".to_string())
                })?;
        }

        if quantity < order.max_fill.0 {
            match effective.direction {
                YesDirection::WantsLow => {
                    face.residual.lower = face.residual.lower.max(effective_ceil);
                }
                YesDirection::WantsHigh => {
                    face.residual.upper = face.residual.upper.min(effective_floor);
                }
            }
        }
        if quantity > 0
            && let Some(condition) = &order.condition
        {
            let condition_face = faces.entry(condition.market).or_default();
            let supported = match condition.direction {
                ConditionDir::Above if condition.threshold.0 < NANOS_PER_DOLLAR => condition_face
                    .base
                    .intersect(condition.threshold.0 + 1, NANOS_PER_DOLLAR),
                ConditionDir::Below if condition.threshold.0 > 0 => {
                    condition_face.base.intersect(0, condition.threshold.0 - 1)
                }
                ConditionDir::Above | ConditionDir::Below => false,
            };
            if !supported {
                return Err(CanonicalPriceError::EmptyPriceFace {
                    market: Some(condition.market),
                });
            }
        }
    }
    // A categorical price simplex contains every listed outcome, including a
    // market with no local order in this batch. Omitting a silent coordinate
    // would change the entropy maximizer for the filled sibling markets.
    for group in market_groups {
        for &market in &group.markets {
            faces.entry(market).or_default();
        }
    }

    let components = apply_minting_complementarity(&mut faces, market_groups)?;
    let mut yes_prices = BTreeMap::new();
    let mut support = BTreeMap::new();
    for component in &components {
        let selected = select_component(component, &faces)?;
        yes_prices.extend(selected.prices);
        support.extend(selected.support);
    }

    let prices: BTreeMap<_, _> = yes_prices
        .iter()
        .map(|(&market, &yes)| (market, vec![Nanos(yes), Nanos(NANOS_PER_DOLLAR - yes)]))
        .collect();
    validate_selected_prices(orders, fills, &order_map, &prices)?;

    Ok(CanonicalPriceSelection { prices, support })
}

fn canonical_order_map(orders: &[Order]) -> Result<BTreeMap<u64, &Order>, CanonicalPriceError> {
    let mut order_map = BTreeMap::new();
    for order in orders {
        order.validate_binary_one_hot_payoff().map_err(|reason| {
            CanonicalPriceError::UnsupportedOrder {
                order_id: order.id,
                reason: reason.to_string(),
            }
        })?;
        if let Some(condition) = &order.condition {
            if condition.market.is_none() {
                return Err(CanonicalPriceError::InvalidInput(format!(
                    "order {} has a condition on the NONE market",
                    order.id
                )));
            }
            if condition.threshold.0 > NANOS_PER_DOLLAR {
                return Err(CanonicalPriceError::InvalidInput(format!(
                    "order {} condition threshold exceeds NANOS_PER_DOLLAR",
                    order.id
                )));
            }
        }
        if order_map.insert(order.id, order).is_some() {
            return Err(CanonicalPriceError::InvalidInput(format!(
                "duplicate order id {}",
                order.id
            )));
        }
    }
    Ok(order_map)
}

fn canonical_fill_quantities(
    fills: &[Fill],
    order_map: &BTreeMap<u64, &Order>,
) -> Result<BTreeMap<u64, u64>, CanonicalPriceError> {
    let mut quantities = BTreeMap::new();
    for fill in fills {
        let Some(order) = order_map.get(&fill.order_id) else {
            return Err(CanonicalPriceError::InvalidInput(format!(
                "fill references unknown order {}",
                fill.order_id
            )));
        };
        if fill.fill_qty.0 > order.max_fill.0 {
            return Err(CanonicalPriceError::InvalidInput(format!(
                "fill {} exceeds max quantity",
                fill.order_id
            )));
        }
        if quantities.insert(fill.order_id, fill.fill_qty.0).is_some() {
            return Err(CanonicalPriceError::InvalidInput(format!(
                "duplicate fill for order {}",
                fill.order_id
            )));
        }
    }
    Ok(quantities)
}

fn canonical_mm_membership(
    constraints: &[MmConstraint],
    order_map: &BTreeMap<u64, &Order>,
) -> Result<BTreeMap<u64, (usize, MmSide)>, CanonicalPriceError> {
    let mut membership = BTreeMap::new();
    for (mm_index, mm) in constraints.iter().enumerate() {
        let mut seen = BTreeSet::new();
        for &order_id in &mm.order_ids {
            if !seen.insert(order_id) {
                return Err(CanonicalPriceError::InvalidInput(format!(
                    "MM {} repeats order {order_id}",
                    mm.mm_id.0
                )));
            }
            if !order_map.contains_key(&order_id) {
                return Err(CanonicalPriceError::InvalidInput(format!(
                    "MM {} references unknown order {order_id}",
                    mm.mm_id.0
                )));
            }
            let Some(&side) = mm.order_sides.get(&order_id) else {
                return Err(CanonicalPriceError::InvalidInput(format!(
                    "MM {} has no side for order {order_id}",
                    mm.mm_id.0
                )));
            };
            if membership.insert(order_id, (mm_index, side)).is_some() {
                return Err(CanonicalPriceError::InvalidInput(format!(
                    "order {order_id} belongs to multiple MM constraints"
                )));
            }
        }
    }
    Ok(membership)
}

fn mm_pacing_factors(
    constraints: &[MmConstraint],
    fills: &BTreeMap<u64, u64>,
    orders: &BTreeMap<u64, &Order>,
) -> Result<Vec<RationalPrice>, CanonicalPriceError> {
    constraints
        .iter()
        .map(|mm| {
            let mut utility_numerator = 0_u128;
            for &order_id in &mm.order_ids {
                let order = orders[&order_id];
                let side = mm.order_sides[&order_id];
                validate_mm_side(order, side)?;
                let value = match side {
                    MmSide::BuyYes | MmSide::BuyNo => order.limit_price.0,
                    MmSide::SellYes | MmSide::SellNo => NANOS_PER_DOLLAR - order.limit_price.0,
                };
                utility_numerator = utility_numerator
                    .checked_add(value as u128 * fills.get(&order_id).copied().unwrap_or(0) as u128)
                    .ok_or_else(|| {
                        CanonicalPriceError::InvalidInput("MM utility overflow".to_string())
                    })?;
            }
            let budget_numerator = mm.max_capital.0 as u128 * SHARE_SCALE as u128;
            if budget_numerator == 0 {
                Ok(RationalPrice {
                    numerator: 0,
                    denominator: 1,
                })
            } else if utility_numerator == 0 || budget_numerator >= utility_numerator {
                Ok(RationalPrice {
                    numerator: 1,
                    denominator: 1,
                })
            } else {
                Ok(RationalPrice {
                    numerator: budget_numerator,
                    denominator: utility_numerator,
                })
            }
        })
        .collect()
}

fn validate_mm_side(order: &Order, side: MmSide) -> Result<(), CanonicalPriceError> {
    let matches = matches!(
        (order.payoffs[0], order.payoffs[1], side),
        (1, 0, MmSide::BuyYes)
            | (-1, 0, MmSide::SellYes)
            | (0, 1, MmSide::BuyNo)
            | (0, -1, MmSide::SellNo)
    );
    if matches {
        Ok(())
    } else {
        Err(CanonicalPriceError::InvalidInput(format!(
            "MM side {side:?} disagrees with order {} payoff",
            order.id
        )))
    }
}

fn effective_order(
    order: &Order,
    mm: Option<&(usize, MmSide)>,
    pacing: &[RationalPrice],
) -> Result<EffectiveOrder, CanonicalPriceError> {
    let (direction, literal_threshold) = match (order.payoffs[0], order.payoffs[1]) {
        (1, 0) => (YesDirection::WantsLow, order.limit_price.0),
        (-1, 0) => (YesDirection::WantsHigh, order.limit_price.0),
        (0, 1) => (
            YesDirection::WantsHigh,
            NANOS_PER_DOLLAR - order.limit_price.0,
        ),
        (0, -1) => (
            YesDirection::WantsLow,
            NANOS_PER_DOLLAR - order.limit_price.0,
        ),
        _ => {
            return Err(CanonicalPriceError::UnsupportedOrder {
                order_id: order.id,
                reason: "order is not binary one-hot".to_string(),
            });
        }
    };
    let effective_threshold = if let Some(&(mm_index, side)) = mm {
        validate_mm_side(order, side)?;
        let alpha = pacing[mm_index];
        let reduced_value = match side {
            MmSide::BuyYes | MmSide::BuyNo => order.limit_price.0,
            MmSide::SellYes | MmSide::SellNo => NANOS_PER_DOLLAR - order.limit_price.0,
        };
        let paced_numerator = alpha
            .numerator
            .checked_mul(reduced_value as u128)
            .ok_or_else(|| {
                CanonicalPriceError::InvalidInput("paced threshold overflow".to_string())
            })?;
        match side {
            MmSide::BuyYes | MmSide::SellNo => RationalPrice {
                numerator: paced_numerator,
                denominator: alpha.denominator,
            },
            MmSide::SellYes | MmSide::BuyNo => RationalPrice {
                numerator: (NANOS_PER_DOLLAR as u128)
                    .checked_mul(alpha.denominator)
                    .and_then(|unit| unit.checked_sub(paced_numerator))
                    .ok_or_else(|| {
                        CanonicalPriceError::InvalidInput(
                            "complementary paced threshold overflow".to_string(),
                        )
                    })?,
                denominator: alpha.denominator,
            },
        }
    } else {
        RationalPrice::integer(literal_threshold)
    };
    Ok(EffectiveOrder {
        market: order.markets[0],
        direction,
        literal_threshold,
        effective_threshold,
    })
}

fn apply_minting_complementarity(
    faces: &mut BTreeMap<MarketId, MarketFace>,
    groups: &[MarketGroup],
) -> Result<Vec<PriceComponent>, CanonicalPriceError> {
    let mut membership = BTreeMap::new();
    for (group_index, group) in groups.iter().enumerate() {
        for &market in &group.markets {
            if membership.insert(market, group_index).is_some() {
                return Err(CanonicalPriceError::InvalidInput(format!(
                    "market {market} belongs to multiple market groups"
                )));
            }
        }
    }

    let mut grouped = vec![Vec::new(); groups.len()];
    let mut components = Vec::new();
    let active_markets: Vec<_> = faces.keys().copied().collect();
    for market in active_markets {
        if let Some(&group) = membership.get(&market) {
            grouped[group].push(market);
        } else {
            let difference = faces[&market].yes_demand - faces[&market].no_demand;
            if difference > 0 {
                pin_face(
                    faces.get_mut(&market).expect("known market"),
                    NANOS_PER_DOLLAR,
                )?;
            } else if difference < 0 {
                pin_face(faces.get_mut(&market).expect("known market"), 0)?;
            }
            components.push(PriceComponent {
                markets: vec![market],
                exact_sum: false,
            });
        }
    }

    for mut markets in grouped.into_iter().filter(|markets| !markets.is_empty()) {
        markets.sort_unstable();
        let max_difference = markets
            .iter()
            .map(|market| faces[market].yes_demand - faces[market].no_demand)
            .max()
            .unwrap_or(0);
        let active_difference = max_difference.max(0);
        for &market in &markets {
            let difference = faces[&market].yes_demand - faces[&market].no_demand;
            if difference != active_difference {
                pin_face(faces.get_mut(&market).expect("known market"), 0)?;
            }
        }
        components.push(PriceComponent {
            markets,
            exact_sum: max_difference > 0,
        });
    }
    components.sort_by_key(|component| component.markets[0]);
    Ok(components)
}

fn pin_face(face: &mut MarketFace, value: u64) -> Result<(), CanonicalPriceError> {
    if value >= face.base.lower && value <= face.base.upper {
        face.base.pin(value);
        Ok(())
    } else {
        Err(CanonicalPriceError::EmptyPriceFace { market: None })
    }
}

fn select_component(
    component: &PriceComponent,
    faces: &BTreeMap<MarketId, MarketFace>,
) -> Result<ComponentSelection, CanonicalPriceError> {
    let entries_at = |relaxation: u64| {
        let mut entries: Vec<(Option<MarketId>, Bounds)> = component
            .markets
            .iter()
            .map(|&market| {
                let face = &faces[&market];
                let residual_lower = face.residual.lower.saturating_sub(relaxation);
                let residual_upper = face
                    .residual
                    .upper
                    .saturating_add(relaxation)
                    .min(NANOS_PER_DOLLAR);
                (
                    Some(market),
                    Bounds {
                        lower: face.base.lower.max(residual_lower),
                        upper: face.base.upper.min(residual_upper),
                    },
                )
            })
            .collect();
        if !component.exact_sum {
            // The residual is the independent binary NO outcome or the
            // categorical group's implicit complementary outcome.
            entries.push((None, Bounds::UNIT));
        }
        entries
    };
    let feasible = |relaxation: u64| {
        let entries = entries_at(relaxation);
        entries
            .iter()
            .all(|(_, bounds)| bounds.lower <= bounds.upper)
            && entries
                .iter()
                .map(|(_, bounds)| bounds.lower as u128)
                .sum::<u128>()
                <= NANOS_PER_DOLLAR as u128
            && entries
                .iter()
                .map(|(_, bounds)| bounds.upper as u128)
                .sum::<u128>()
                >= NANOS_PER_DOLLAR as u128
    };

    // Integer quantity landing can move a recomputed rational pacing factor
    // by a few nanos, making otherwise marginal residual bounds cross. Keep
    // filled-order and minting bounds hard, then find the smallest common
    // relaxation of residual KKT bounds that restores a non-empty face.
    if !feasible(NANOS_PER_DOLLAR) {
        return Err(CanonicalPriceError::EmptyPriceFace { market: None });
    }
    let relaxation = if feasible(0) {
        0
    } else {
        let mut low = 0_u64;
        let mut high = NANOS_PER_DOLLAR;
        while low + 1 < high {
            let middle = low + (high - low) / 2;
            if feasible(middle) {
                high = middle;
            } else {
                low = middle;
            }
        }
        high
    };

    let entries = entries_at(relaxation);
    let values = entropy_waterfill(&entries, NANOS_PER_DOLLAR)?;
    let selected = entries
        .iter()
        .zip(&values)
        .filter_map(|((market, _), &value)| market.map(|market| (market, value)))
        .collect();
    let support = entries
        .iter()
        .filter_map(|(market, bounds)| {
            market.map(|market| {
                (
                    market,
                    CanonicalPriceSupport {
                        lower_yes: Nanos(bounds.lower),
                        upper_yes: Nanos(bounds.upper),
                    },
                )
            })
        })
        .collect();
    Ok(ComponentSelection {
        prices: selected,
        support,
    })
}

/// Maximize a symmetric separable concave entropy on an integer box-simplex.
///
/// All unclamped coordinates are equal up to one nano. That water-fill is also
/// the maximizer of Shannon entropy; no logarithms are needed to identify it.
fn entropy_waterfill(
    entries: &[(Option<MarketId>, Bounds)],
    total: u64,
) -> Result<Vec<u64>, CanonicalPriceError> {
    let lower_sum: u128 = entries.iter().map(|(_, bounds)| bounds.lower as u128).sum();
    let upper_sum: u128 = entries.iter().map(|(_, bounds)| bounds.upper as u128).sum();
    if lower_sum > total as u128 || upper_sum < total as u128 {
        return Err(CanonicalPriceError::EmptyPriceFace { market: None });
    }

    let sum_at = |level: u64| -> u128 {
        entries
            .iter()
            .map(|(_, bounds)| level.clamp(bounds.lower, bounds.upper) as u128)
            .sum()
    };
    let mut low = 0_u64;
    let mut high = total.saturating_add(1);
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        if sum_at(middle) <= total as u128 {
            low = middle;
        } else {
            high = middle;
        }
    }

    let mut values: Vec<_> = entries
        .iter()
        .map(|(_, bounds)| low.clamp(bounds.lower, bounds.upper))
        .collect();
    let used: u128 = values.iter().map(|&value| value as u128).sum();
    let mut remaining = (total as u128 - used) as usize;
    let mut eligible: Vec<_> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, (market, bounds))| {
            (values[index] < bounds.upper && values[index] == low).then_some((
                market.is_none(),
                *market,
                index,
            ))
        })
        .collect();
    // Market ids define the indivisible-nano tie-break. The implicit residual
    // outcome sorts last.
    eligible.sort_by_key(|&(is_residual, market, _)| (is_residual, market));
    for (_, _, index) in eligible {
        if remaining == 0 {
            break;
        }
        values[index] += 1;
        remaining -= 1;
    }
    if remaining != 0 {
        return Err(CanonicalPriceError::EmptyPriceFace { market: None });
    }
    Ok(values)
}

fn validate_selected_prices(
    orders: &[Order],
    fills: &[Fill],
    order_map: &BTreeMap<u64, &Order>,
    prices: &BTreeMap<MarketId, Vec<Nanos>>,
) -> Result<(), CanonicalPriceError> {
    for fill in fills {
        if fill.fill_qty.0 == 0 {
            continue;
        }
        let order = order_map[&fill.order_id];
        let market_prices = &prices[&order.markets[0]];
        let fill_price = if order.payoffs[0] != 0 {
            market_prices[0]
        } else {
            market_prices[1]
        };
        if !order.is_satisfied_at_price(fill_price) {
            return Err(CanonicalPriceError::EmptyPriceFace {
                market: Some(order.markets[0]),
            });
        }
        if let Some(condition) = &order.condition {
            let Some(condition_price) = prices
                .get(&condition.market)
                .and_then(|market_prices| market_prices.first())
                .copied()
            else {
                return Err(CanonicalPriceError::EmptyPriceFace {
                    market: Some(condition.market),
                });
            };
            let active = match condition.direction {
                ConditionDir::Above => condition_price > condition.threshold,
                ConditionDir::Below => condition_price < condition.threshold,
            };
            if !active {
                return Err(CanonicalPriceError::EmptyPriceFace {
                    market: Some(condition.market),
                });
            }
        }
    }
    // Keep this explicit so callers cannot accidentally supply a filtered
    // order map that changes repricing semantics.
    if order_map.len() != orders.len() {
        return Err(CanonicalPriceError::InvalidInput(
            "order map is incomplete".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MarketGroup, MarketSet, MmId, Qty, outcome_sell, simple_no_buy, simple_yes_buy};

    fn filled(order_id: u64, quantity: u64) -> Fill {
        Fill::new(order_id, Qty(quantity), Nanos::ZERO)
    }

    #[test]
    fn balanced_cross_selects_binary_maximum_entropy() {
        let mut markets = MarketSet::new();
        let market = markets.add_binary("m");
        let orders = vec![
            simple_yes_buy(&markets, 1, market, 700_000_000, 100),
            outcome_sell(&markets, 2, market, 0, 200_000_000, 100),
        ];
        let selected =
            canonical_clearing_prices(&orders, &[filled(1, 100), filled(2, 100)], &[], &[])
                .unwrap();
        assert_eq!(
            selected.prices[&market],
            vec![Nanos(500_000_000), Nanos(500_000_000)]
        );
    }

    #[test]
    fn observed_wide_support_selects_the_interior_not_either_endpoint() {
        let mut markets = MarketSet::new();
        let market = markets.add_binary("wide-support");
        let orders = vec![
            simple_yes_buy(&markets, 1, market, 549_000_000, 100),
            outcome_sell(&markets, 2, market, 0, 225_000_000, 100),
        ];
        let selected =
            canonical_clearing_prices(&orders, &[filled(1, 100), filled(2, 100)], &[], &[])
                .unwrap();
        assert_eq!(
            selected.support[&market],
            CanonicalPriceSupport {
                lower_yes: Nanos(225_000_000),
                upper_yes: Nanos(549_000_000),
            }
        );
        assert_eq!(
            selected.prices[&market],
            vec![Nanos(500_000_000), Nanos(500_000_000)]
        );
    }

    #[test]
    fn one_sided_residual_selects_the_executable_boundary() {
        let mut markets = MarketSet::new();
        let market = markets.add_binary("m");
        let orders = vec![
            simple_yes_buy(&markets, 1, market, 700_000_000, 100),
            outcome_sell(&markets, 2, market, 0, 200_000_000, 100),
            outcome_sell(&markets, 3, market, 0, 225_000_000, 100),
        ];
        let sell_selected =
            canonical_clearing_prices(&orders, &[filled(1, 100), filled(2, 100)], &[], &[])
                .unwrap();
        assert_eq!(sell_selected.prices[&market][0], Nanos(225_000_000));

        let orders = vec![
            simple_yes_buy(&markets, 1, market, 700_000_000, 100),
            simple_yes_buy(&markets, 2, market, 675_000_000, 100),
            outcome_sell(&markets, 3, market, 0, 200_000_000, 100),
        ];
        let buy_selected =
            canonical_clearing_prices(&orders, &[filled(1, 100), filled(3, 100)], &[], &[])
                .unwrap();
        assert_eq!(buy_selected.prices[&market][0], Nanos(675_000_000));
    }

    #[test]
    fn incompatible_residual_uses_minimax_relaxation_independent_of_order() {
        let mut markets = MarketSet::new();
        let market = markets.add_binary("m");
        let mut orders = vec![
            simple_yes_buy(&markets, 1, market, 700_000_000, 200),
            outcome_sell(&markets, 2, market, 0, 200_000_000, 200),
        ];
        let fills = [filled(1, 100), filled(2, 100)];
        let first = canonical_clearing_prices(&orders, &fills, &[], &[]).unwrap();
        orders.reverse();
        let reversed = canonical_clearing_prices(&orders, &fills, &[], &[]).unwrap();
        assert_eq!(first, reversed);
        assert_eq!(
            first.support[&market],
            CanonicalPriceSupport {
                lower_yes: Nanos(450_000_000),
                upper_yes: Nanos(450_000_000),
            }
        );
        assert_eq!(first.prices[&market][0], Nanos(450_000_000));
    }

    #[test]
    fn yes_no_coordinate_swap_complements_the_price() {
        let mut markets = MarketSet::new();
        let market = markets.add_binary("m");
        let yes_orders = vec![
            simple_yes_buy(&markets, 1, market, 700_000_000, 100),
            outcome_sell(&markets, 2, market, 0, 200_000_000, 100),
            outcome_sell(&markets, 3, market, 0, 225_000_000, 100),
        ];
        let no_orders = vec![
            simple_no_buy(&markets, 1, market, 700_000_000, 100),
            outcome_sell(&markets, 2, market, 1, 200_000_000, 100),
            outcome_sell(&markets, 3, market, 1, 225_000_000, 100),
        ];
        let fills = [filled(1, 100), filled(2, 100)];
        let yes = canonical_clearing_prices(&yes_orders, &fills, &[], &[]).unwrap();
        let no = canonical_clearing_prices(&no_orders, &fills, &[], &[]).unwrap();
        assert_eq!(
            yes.prices[&market][0].0 + no.prices[&market][0].0,
            NANOS_PER_DOLLAR
        );
    }

    #[test]
    fn categorical_tie_waterfills_the_group_simplex() {
        let mut markets = MarketSet::new();
        let a = markets.add_binary("a");
        let b = markets.add_binary("b");
        let c = markets.add_binary("c");
        let orders = vec![
            simple_yes_buy(&markets, 1, a, 900_000_000, 100),
            simple_yes_buy(&markets, 2, b, 900_000_000, 100),
            simple_yes_buy(&markets, 3, c, 900_000_000, 100),
        ];
        let fills = [filled(1, 100), filled(2, 100), filled(3, 100)];
        let group = MarketGroup::new("g")
            .with_market(a)
            .with_market(b)
            .with_market(c);
        let selected = canonical_clearing_prices(&orders, &fills, &[], &[group]).unwrap();
        let values = [a, b, c].map(|market| selected.prices[&market][0].0);
        assert_eq!(values.iter().sum::<u64>(), NANOS_PER_DOLLAR);
        assert!(values.iter().max().unwrap() - values.iter().min().unwrap() <= 1);
    }

    #[test]
    fn silent_categorical_outcomes_participate_in_entropy_selection() {
        let mut markets = MarketSet::new();
        let a = markets.add_binary("a");
        let b = markets.add_binary("b");
        let c = markets.add_binary("c");
        let orders = vec![
            simple_yes_buy(&markets, 1, a, 900_000_000, 100),
            outcome_sell(&markets, 2, a, 0, 100_000_000, 100),
        ];
        let fills = [filled(1, 100), filled(2, 100)];
        let group = MarketGroup::new("g")
            .with_market(a)
            .with_market(b)
            .with_market(c);
        let selected = canonical_clearing_prices(&orders, &fills, &[], &[group]).unwrap();
        for market in [a, b, c] {
            assert_eq!(selected.prices[&market][0], Nanos(250_000_000));
        }
    }

    #[test]
    fn scarce_shared_mm_budget_shades_residual_quotes_across_markets() {
        let mut markets = MarketSet::new();
        let a = markets.add_binary("a");
        let b = markets.add_binary("b");
        let orders = vec![
            simple_yes_buy(&markets, 1, a, 800_000_000, SHARE_SCALE),
            simple_yes_buy(&markets, 2, b, 800_000_000, SHARE_SCALE),
            outcome_sell(&markets, 3, a, 0, 100_000_000, SHARE_SCALE),
        ];
        let mm = MmConstraint::new(MmId(7), Nanos(600_000_000))
            .with_order(1, MmSide::BuyYes)
            .with_order(2, MmSide::BuyYes);
        let fills = [filled(1, SHARE_SCALE), filled(3, SHARE_SCALE)];
        let selected = canonical_clearing_prices(&orders, &fills, &[mm], &[]).unwrap();
        assert_eq!(selected.prices[&a][0], Nanos(500_000_000));
        // One shared alpha = 0.6 / 0.8 shades the otherwise executable 0.8
        // quote on b to 0.6 even though its local book has no fills.
        assert_eq!(selected.prices[&b][0], Nanos(600_000_000));
    }

    #[test]
    fn general_payoff_vectors_fail_closed() {
        let mut markets = MarketSet::new();
        let market = markets.add_binary("m");
        let mut spread = Order::new(1);
        spread.markets[0] = market;
        spread.num_markets = 1;
        spread.num_states = 2;
        spread.payoffs[0] = 1;
        spread.payoffs[1] = -1;
        spread.limit_price = Nanos(500_000_000);
        spread.max_fill = Qty(100);
        assert!(matches!(
            canonical_clearing_prices(&[spread], &[], &[], &[]),
            Err(CanonicalPriceError::UnsupportedOrder { .. })
        ));
    }

    #[test]
    fn filled_conditions_select_the_strict_integer_activation_boundary() {
        let mut markets = MarketSet::new();
        let traded = markets.add_binary("traded");
        let condition_market = markets.add_binary("condition");
        for (direction, expected) in [
            (ConditionDir::Above, 500_000_001),
            (ConditionDir::Below, 499_999_999),
        ] {
            let mut conditional = simple_yes_buy(&markets, 1, traded, 700_000_000, 100);
            conditional.condition = Some(crate::PriceCondition {
                market: condition_market,
                threshold: Nanos(500_000_000),
                direction,
            });
            let seller = outcome_sell(&markets, 2, traded, 0, 200_000_000, 100);
            let selected = canonical_clearing_prices(
                &[conditional, seller],
                &[filled(1, 100), filled(2, 100)],
                &[],
                &[],
            )
            .unwrap();
            assert_eq!(
                selected.prices[&traded],
                vec![Nanos(500_000_000), Nanos(500_000_000)]
            );
            assert_eq!(selected.prices[&condition_market][0], Nanos(expected));
        }
    }
}
