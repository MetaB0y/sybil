//! Scenario creation helpers.
//!
//! Maps scenario names to problem generators with appropriate configurations.

use matching_scenarios::{
    generate_combined_scenario, generate_mega_scenario, generate_milp_killer_scenario,
    generate_random_scenario, MegaScenarioConfig, MilpKillerConfig, Problem, RandomConfig,
};

/// Create a problem from a scenario name and seed.
pub fn create_problem(scenario_name: &str, seed: u64) -> Problem {
    match scenario_name {
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
        _ => {
            eprintln!("Unknown scenario: {}, using random-easy", scenario_name);
            generate_random_scenario(RandomConfig {
                seed,
                ..RandomConfig::easy()
            })
        }
    }
}
