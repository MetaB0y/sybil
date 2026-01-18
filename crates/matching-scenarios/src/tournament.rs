//! Tournament/Sports scenario (World Cup style).

use rand::Rng;
use rand::rngs::StdRng;
use rand::SeedableRng;

use matching_engine::{
    ConstraintBuilder, MarketSet, Order, MarketId, Qty,
    price_to_nanos, outcome_buy, bundle_yes,
};

use matching_engine::Problem;

/// Configuration for tournament scenario
#[derive(Clone, Debug)]
pub struct TournamentConfig {
    pub seed: u64,
    pub num_teams: usize,
    pub orders_per_team: usize,
    pub liquidity_multiplier: f64,
}

impl Default for TournamentConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            num_teams: 8,
            orders_per_team: 5,
            liquidity_multiplier: 0.5,
        }
    }
}

struct TeamInfo {
    name: String,
    win_prob: f64,
}

/// Generate a tournament scenario.
pub fn generate_tournament_scenario(config: TournamentConfig) -> Problem {
    let mut rng = StdRng::seed_from_u64(config.seed);
    let mut problem = Problem::new("Tournament Championship");

    let teams: Vec<TeamInfo> = generate_teams(config.num_teams, &mut rng);

    let num_rounds = (config.num_teams as f64).log2().ceil() as usize;

    let mut round_markets: Vec<MarketId> = Vec::new();
    let round_names = ["Round of 16", "Quarterfinals", "Semifinals", "Final", "Champion"];

    for round in 0..num_rounds {
        let round_name = if round < round_names.len() {
            round_names[round]
        } else {
            "Round"
        };

        let outcomes: Vec<String> = teams.iter().map(|t| format!("{} advances", t.name)).collect();
        let market = problem.markets.add(
            format!("{} Winner", round_name),
            outcomes,
        );
        round_markets.push(market);
    }

    let mut constraint_builder = ConstraintBuilder::new();
    for round in 1..num_rounds {
        for team_idx in 0..config.num_teams {
            constraint_builder = constraint_builder.implies(
                round_markets[round],
                team_idx as u8,
                round_markets[round - 1],
                team_idx as u8,
            );
        }
    }
    problem.constraints = constraint_builder.build();

    let base_depth = (50.0 * config.liquidity_multiplier) as Qty;

    for (round_idx, &market) in round_markets.iter().enumerate() {
        for (team_idx, team) in teams.iter().enumerate() {
            let round_multiplier = 1.0 / (2.0_f64.powi(round_idx as i32));
            let base_price = team.win_prob * round_multiplier;
            let spread = 0.02;

            let round_depth = base_depth / (round_idx as Qty + 1);

            problem.liquidity.add_bid(
                market,
                team_idx as u8,
                price_to_nanos((base_price - spread).max(0.01)),
                round_depth,
            );
            problem.liquidity.add_ask(
                market,
                team_idx as u8,
                price_to_nanos((base_price + spread).min(0.99)),
                round_depth,
            );
        }
    }

    let mut order_id = 1u64;

    for (team_idx, team) in teams.iter().enumerate() {
        for _ in 0..config.orders_per_team {
            let order = generate_team_order(
                &problem.markets,
                &mut rng,
                &mut order_id,
                &round_markets,
                team_idx,
                team,
                config.num_teams,
            );
            problem.orders.push(order);
        }
    }

    for _ in 0..config.num_teams {
        let order = generate_cross_team_order(
            &problem.markets,
            &mut rng,
            &mut order_id,
            &round_markets,
            &teams,
        );
        problem.orders.push(order);
    }

    problem
}

fn generate_teams(num_teams: usize, rng: &mut StdRng) -> Vec<TeamInfo> {
    let team_names = [
        "Germany", "Brazil", "Argentina", "France",
        "Spain", "England", "Italy", "Netherlands",
        "Portugal", "Belgium", "Croatia", "Uruguay",
        "Denmark", "Japan", "Mexico", "USA",
    ];

    let mut teams = Vec::new();
    for i in 0..num_teams {
        let name = if i < team_names.len() {
            team_names[i].to_string()
        } else {
            format!("Team {}", i)
        };

        let strength_tier = i / 4;
        let base_prob = match strength_tier {
            0 => rng.gen_range(0.15..0.20),
            1 => rng.gen_range(0.10..0.15),
            2 => rng.gen_range(0.05..0.10),
            _ => rng.gen_range(0.02..0.05),
        };

        teams.push(TeamInfo {
            name,
            win_prob: base_prob,
        });
    }

    teams
}

fn generate_team_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    round_markets: &[MarketId],
    team_idx: usize,
    team: &TeamInfo,
    _num_teams: usize,
) -> Order {
    let id = *order_id;
    *order_id += 1;

    let round_idx = rng.gen_range(0..round_markets.len());
    let market = round_markets[round_idx];

    let round_multiplier = 1.0 / (2.0_f64.powi(round_idx as i32));
    let base_price = team.win_prob * round_multiplier;

    let premium = rng.gen_range(0.0..0.05);
    let limit = (base_price + premium).min(0.95);

    let qty: Qty = rng.gen_range(20..100);

    outcome_buy(
        markets,
        id,
        market,
        team_idx as u8,
        price_to_nanos(limit),
        qty,
    )
}

fn generate_cross_team_order(
    markets: &MarketSet,
    rng: &mut StdRng,
    order_id: &mut u64,
    round_markets: &[MarketId],
    teams: &[TeamInfo],
) -> Order {
    let id = *order_id;
    *order_id += 1;

    if round_markets.len() >= 2 && teams.len() >= 2 {
        let team1 = rng.gen_range(0..teams.len());
        let mut team2 = rng.gen_range(0..teams.len());
        while team2 == team1 {
            team2 = rng.gen_range(0..teams.len());
        }

        let round = rng.gen_range(0..round_markets.len().saturating_sub(1));

        let round_mult = 1.0 / (2.0_f64.powi(round as i32));
        let combined_prob = teams[team1].win_prob * teams[team2].win_prob * round_mult * round_mult;
        let limit = (combined_prob * rng.gen_range(0.8..1.2)).clamp(0.01, 0.95);

        let qty: Qty = rng.gen_range(10..50);

        bundle_yes(
            markets,
            id,
            &[round_markets[round]],
            price_to_nanos(limit),
            qty,
        )
    } else {
        let team_idx = rng.gen_range(0..teams.len());
        let round_idx = rng.gen_range(0..round_markets.len());
        let base_price = teams[team_idx].win_prob / (2.0_f64.powi(round_idx as i32));

        outcome_buy(
            markets,
            id,
            round_markets[round_idx],
            team_idx as u8,
            price_to_nanos(base_price * 1.1),
            50,
        )
    }
}
