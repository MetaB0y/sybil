//! Scenario creation helpers.
//!
//! Maps scenario names to problem generators with appropriate configurations.

use matching_scenarios::{
    generate_adversarial_scenario, generate_combined_scenario, generate_conditional_chain_scenario,
    generate_deep_implication_scenario, generate_large_interconnected_scenario,
    generate_liquidity_cliff_scenario, generate_mega_scenario, generate_milp_killer_scenario,
    generate_nested_bundle_scenario, generate_planted_chain_scenario,
    generate_planted_complement_scenario, generate_planted_exclusion_scenario,
    generate_presidential_scenario, generate_random_scenario, generate_realistic_scenario,
    generate_tournament_scenario, AdversarialConfig, ConditionalChainConfig, DeepImplicationConfig,
    LargeInterconnectedConfig, LiquidityCliffConfig, MegaScenarioConfig, MilpKillerConfig,
    NestedBundleConfig, PlantedChainConfig, PlantedComplementConfig, PlantedExclusionConfig,
    PresidentialConfig, Problem, RandomConfig, RealisticConfig, TournamentConfig,
};

/// Create a problem from a scenario name and seed.
pub fn create_problem(scenario_name: &str, seed: u64) -> Problem {
    match scenario_name {
        "presidential" => generate_presidential_scenario(PresidentialConfig {
            seed,
            ..Default::default()
        }),
        "presidential-hard" => generate_presidential_scenario(PresidentialConfig {
            seed,
            num_simple_orders: 50,
            num_bundle_orders: 20,
            num_conditional_orders: 10,
            liquidity_multiplier: 0.3,
            ..Default::default()
        }),
        "tournament" => generate_tournament_scenario(TournamentConfig {
            seed,
            ..Default::default()
        }),
        "tournament-large" => generate_tournament_scenario(TournamentConfig {
            seed,
            num_teams: 16,
            orders_per_team: 8,
            liquidity_multiplier: 0.3,
        }),
        "random-easy" => generate_random_scenario(RandomConfig {
            seed,
            ..RandomConfig::easy()
        }),
        "random-medium" => generate_random_scenario(RandomConfig {
            seed,
            ..RandomConfig::medium()
        }),
        "random-hard" => generate_random_scenario(RandomConfig {
            seed,
            ..RandomConfig::hard()
        }),
        // Complex scenarios
        "nested-bundles" => generate_nested_bundle_scenario(NestedBundleConfig {
            seed,
            ..Default::default()
        }),
        "conditional-chains" => generate_conditional_chain_scenario(ConditionalChainConfig {
            seed,
            ..Default::default()
        }),
        "deep-implications" => generate_deep_implication_scenario(DeepImplicationConfig {
            seed,
            ..Default::default()
        }),
        "liquidity-cliffs" => generate_liquidity_cliff_scenario(LiquidityCliffConfig {
            seed,
            ..Default::default()
        }),
        "adversarial" => generate_adversarial_scenario(AdversarialConfig {
            seed,
            ..Default::default()
        }),
        "large-interconnected" => {
            generate_large_interconnected_scenario(LargeInterconnectedConfig {
                seed,
                ..Default::default()
            })
        }
        // Stress scenarios
        "mega" | "mega-medium" => generate_mega_scenario(MegaScenarioConfig {
            seed,
            ..MegaScenarioConfig::medium()
        }),
        "mega-small" => generate_mega_scenario(MegaScenarioConfig {
            seed,
            ..MegaScenarioConfig::small()
        }),
        "mega-large" => generate_mega_scenario(MegaScenarioConfig {
            seed,
            ..MegaScenarioConfig::large()
        }),
        "mega-extreme" => generate_mega_scenario(MegaScenarioConfig {
            seed,
            ..MegaScenarioConfig::extreme()
        }),
        "combined" => generate_combined_scenario(seed),
        // MILP killer scenarios
        "milp-killer" | "milp-killer-test" => generate_milp_killer_scenario(MilpKillerConfig {
            seed,
            ..MilpKillerConfig::test()
        }),
        "milp-killer-full" => generate_milp_killer_scenario(MilpKillerConfig {
            seed,
            ..MilpKillerConfig::timeout_guaranteed()
        }),
        "milp-killer-extreme" => generate_milp_killer_scenario(MilpKillerConfig {
            seed,
            ..MilpKillerConfig::extreme()
        }),
        // Planted pattern scenarios
        "planted-chain" => generate_planted_chain_scenario(PlantedChainConfig {
            seed,
            ..Default::default()
        }),
        "planted-complement" => generate_planted_complement_scenario(PlantedComplementConfig {
            seed,
            ..Default::default()
        }),
        "planted-exclusion" => generate_planted_exclusion_scenario(PlantedExclusionConfig {
            seed,
            ..Default::default()
        }),
        // Realistic scenarios (cross-market value demonstration)
        "realistic" | "realistic-standard" => generate_realistic_scenario(RealisticConfig {
            seed,
            ..RealisticConfig::standard()
        }),
        "realistic-test" => generate_realistic_scenario(RealisticConfig {
            seed,
            ..RealisticConfig::test()
        }),
        "realistic-small" => generate_realistic_scenario(RealisticConfig {
            seed,
            ..RealisticConfig::small()
        }),
        "realistic-extreme" => generate_realistic_scenario(RealisticConfig {
            seed,
            ..RealisticConfig::extreme()
        }),
        "realistic-cross-market" => generate_realistic_scenario(RealisticConfig {
            seed,
            ..RealisticConfig::cross_market_demo()
        }),
        _ => {
            eprintln!("Unknown scenario: {}, using random-easy", scenario_name);
            generate_random_scenario(RandomConfig {
                seed,
                ..RandomConfig::easy()
            })
        }
    }
}

/// List all available scenario names.
pub fn available_scenarios() -> Vec<&'static str> {
    vec![
        "presidential",
        "presidential-hard",
        "tournament",
        "tournament-large",
        "random-easy",
        "random-medium",
        "random-hard",
        "nested-bundles",
        "conditional-chains",
        "deep-implications",
        "liquidity-cliffs",
        "adversarial",
        "large-interconnected",
        "mega",
        "mega-small",
        "mega-medium",
        "mega-large",
        "mega-extreme",
        "combined",
        "milp-killer",
        "milp-killer-test",
        "milp-killer-full",
        "milp-killer-extreme",
        "planted-chain",
        "planted-complement",
        "planted-exclusion",
        "realistic",
        "realistic-standard",
        "realistic-test",
        "realistic-small",
        "realistic-extreme",
        "realistic-cross-market",
    ]
}
