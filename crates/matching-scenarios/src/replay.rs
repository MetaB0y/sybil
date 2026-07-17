//! Compact, solver-only replay inputs.
//!
//! A replay case is the solver-relevant projection of a [`Problem`]. It omits
//! account identities, signatures, balances, market text, and solver output,
//! so a sequencer-boundary capture can be retained without making benchmark
//! inputs sensitive or needlessly large. Map-backed fields are represented as
//! sorted vectors to make the encoded corpus deterministic.

use std::collections::{HashMap, HashSet};

use matching_engine::{
    Market, MarketGroup, MarketId, MarketSet, MmConstraint, MmId, MmSide, Nanos, Order, Problem,
};
use serde::{Deserialize, Serialize};

/// The only replay schema currently understood by the benchmark runner.
pub const SOLVER_REPLAY_SCHEMA_V1: u32 = 1;

/// A versioned collection of solver-boundary cases.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolverReplayCorpusV1 {
    pub schema_version: u32,
    pub corpus_id: String,
    /// Coarse provenance such as `sequencer-sim` or `production-redacted`.
    pub source: String,
    pub cases: Vec<SolverReplayCaseV1>,
}

impl SolverReplayCorpusV1 {
    pub fn new(
        corpus_id: impl Into<String>,
        source: impl Into<String>,
        cases: Vec<SolverReplayCaseV1>,
    ) -> Self {
        Self {
            schema_version: SOLVER_REPLAY_SCHEMA_V1,
            corpus_id: corpus_id.into(),
            source: source.into(),
            cases,
        }
    }

    /// Validate the envelope and every reconstructed [`Problem`].
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != SOLVER_REPLAY_SCHEMA_V1 {
            return Err(format!(
                "unsupported solver replay schema {}",
                self.schema_version
            ));
        }
        if self.corpus_id.trim().is_empty() {
            return Err("solver replay corpus has an empty id".to_string());
        }
        if self.source.trim().is_empty() {
            return Err("solver replay corpus has an empty source".to_string());
        }
        if self.cases.is_empty() {
            return Err("solver replay corpus has no cases".to_string());
        }

        let mut case_ids = HashSet::new();
        for case in &self.cases {
            if !case_ids.insert(case.id.as_str()) {
                return Err(format!("duplicate solver replay case id {}", case.id));
            }
            case.to_problem()?;
        }
        Ok(())
    }
}

/// One canonical MM constraint without its map-backed serialization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayMmConstraintV1 {
    pub mm_id: MmId,
    pub max_capital: Nanos,
    pub order_ids: Vec<u64>,
    /// Sorted strictly by order id.
    pub order_sides: Vec<(u64, MmSide)>,
}

impl From<&MmConstraint> for ReplayMmConstraintV1 {
    fn from(constraint: &MmConstraint) -> Self {
        let mut order_sides: Vec<_> = constraint
            .order_sides
            .iter()
            .map(|(&order_id, &side)| (order_id, side))
            .collect();
        order_sides.sort_unstable_by_key(|(order_id, _)| *order_id);
        Self {
            mm_id: constraint.mm_id,
            max_capital: constraint.max_capital,
            order_ids: constraint.order_ids.clone(),
            order_sides,
        }
    }
}

impl ReplayMmConstraintV1 {
    fn to_constraint(&self) -> Result<MmConstraint, String> {
        if !strictly_increasing(self.order_sides.iter().map(|(order_id, _)| *order_id)) {
            return Err(format!(
                "MM {} replay sides are not in canonical order",
                self.mm_id.0
            ));
        }
        let expected: HashSet<_> = self.order_ids.iter().copied().collect();
        if expected.len() != self.order_ids.len() {
            return Err(format!("MM {} repeats an order id", self.mm_id.0));
        }
        let actual: HashSet<_> = self
            .order_sides
            .iter()
            .map(|(order_id, _)| *order_id)
            .collect();
        if expected != actual {
            return Err(format!(
                "MM {} replay order ids and sides disagree",
                self.mm_id.0
            ));
        }
        Ok(MmConstraint {
            mm_id: self.mm_id,
            max_capital: self.max_capital,
            order_ids: self.order_ids.clone(),
            order_sides: self.order_sides.iter().copied().collect(),
        })
    }
}

/// The solver-relevant projection of one batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolverReplayCaseV1 {
    pub id: String,
    /// Small, queryable shape labels; never free-form order-flow data.
    pub traits: Vec<String>,
    pub next_market_id: u32,
    /// Sorted strictly by market id. Names are deliberately omitted.
    pub market_ids: Vec<MarketId>,
    /// Preserves the exact order sequence presented to the solver.
    pub orders: Vec<Order>,
    /// Preserves MM sequence while canonicalizing each constraint's side map.
    pub mm_constraints: Vec<ReplayMmConstraintV1>,
    /// Preserves group and within-group market sequence; names are omitted.
    pub market_groups: Vec<Vec<MarketId>>,
}

impl SolverReplayCaseV1 {
    pub fn from_problem(id: impl Into<String>, traits: Vec<String>, problem: &Problem) -> Self {
        let mut market_ids: Vec<_> = problem.markets.iter().map(|market| market.id).collect();
        market_ids.sort_unstable();
        Self {
            id: id.into(),
            traits,
            next_market_id: problem.markets.next_id(),
            market_ids,
            orders: problem.orders.clone(),
            mm_constraints: problem
                .mm_constraints
                .iter()
                .map(ReplayMmConstraintV1::from)
                .collect(),
            market_groups: problem
                .market_groups
                .iter()
                .map(|group| group.markets.clone())
                .collect(),
        }
    }

    /// Reconstruct a benchmark problem after checking canonical form.
    pub fn to_problem(&self) -> Result<Problem, String> {
        if self.id.trim().is_empty() {
            return Err("solver replay case has an empty id".to_string());
        }
        if !strictly_increasing(self.market_ids.iter().map(|market_id| market_id.0)) {
            return Err(format!(
                "case {} market ids are not in canonical order",
                self.id
            ));
        }
        if self
            .market_ids
            .last()
            .is_some_and(|market_id| market_id.0 >= self.next_market_id)
        {
            return Err(format!(
                "case {} next market id does not follow its markets",
                self.id
            ));
        }

        let markets: HashMap<_, _> = self
            .market_ids
            .iter()
            .copied()
            .map(|market_id| {
                (
                    market_id,
                    Market::new(market_id, format!("replay-market-{}", market_id.0)),
                )
            })
            .collect();
        let mut mm_ids = HashSet::new();
        let mm_constraints = self
            .mm_constraints
            .iter()
            .map(|constraint| {
                if !mm_ids.insert(constraint.mm_id) {
                    return Err(format!(
                        "case {} repeats MM id {}",
                        self.id, constraint.mm_id.0
                    ));
                }
                constraint.to_constraint()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let market_groups = self
            .market_groups
            .iter()
            .enumerate()
            .map(|(index, market_ids)| MarketGroup {
                name: format!("replay-group-{index}"),
                markets: market_ids.clone(),
            })
            .collect();
        let problem = Problem {
            name: self.id.clone(),
            markets: MarketSet::restore(markets, self.next_market_id),
            orders: self.orders.clone(),
            mm_constraints,
            market_groups,
        };
        problem
            .validate()
            .map_err(|errors| format!("invalid replay case {}: {}", self.id, errors.join("; ")))?;
        Ok(problem)
    }
}

fn strictly_increasing<T: Ord>(values: impl IntoIterator<Item = T>) -> bool {
    let mut previous = None;
    for value in values {
        if previous.as_ref().is_some_and(|previous| previous >= &value) {
            return false;
        }
        previous = Some(value);
    }
    true
}

#[cfg(test)]
mod tests {
    use matching_engine::{MmId, outcome_sell, shares_to_qty, simple_yes_buy};

    use super::*;

    fn replay_problem() -> Problem {
        let mut problem = Problem::new("source");
        let market = problem.markets.add_binary("private market name");
        let retail = simple_yes_buy(
            &problem.markets,
            10,
            market,
            600_000_000,
            shares_to_qty(2).0,
        );
        let mm_order = outcome_sell(
            &problem.markets,
            20,
            market,
            0,
            550_000_000,
            shares_to_qty(2).0,
        );
        problem.orders = vec![retail, mm_order];
        problem
            .mm_constraints
            .push(MmConstraint::new(MmId(7), Nanos(900_000_000)).with_order(20, MmSide::SellYes));
        problem
    }

    #[test]
    fn projection_round_trips_solver_inputs_without_names() {
        let source = replay_problem();
        let case = SolverReplayCaseV1::from_problem(
            "case-1",
            vec!["sequencer-boundary".to_string()],
            &source,
        );
        let restored = case.to_problem().expect("valid replay");

        assert_eq!(restored.orders, source.orders);
        assert_eq!(restored.markets.next_id(), source.markets.next_id());
        assert_eq!(restored.mm_constraints[0].order_ids, vec![20]);
        assert_eq!(restored.mm_constraints[0].order_sides[&20], MmSide::SellYes);
        assert_eq!(
            restored.markets.get(MarketId::new(0)).unwrap().name,
            "replay-market-0"
        );
    }

    #[test]
    fn corpus_rejects_noncanonical_side_maps() {
        let mut case = SolverReplayCaseV1::from_problem("case-1", Vec::new(), &replay_problem());
        case.mm_constraints[0]
            .order_sides
            .push((21, MmSide::BuyYes));
        case.mm_constraints[0].order_sides.reverse();
        let corpus = SolverReplayCorpusV1::new("corpus", "test", vec![case]);

        assert!(corpus.validate().unwrap_err().contains("canonical order"));
    }
}
