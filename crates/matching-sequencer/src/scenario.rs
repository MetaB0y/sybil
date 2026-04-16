use std::collections::HashMap;

use matching_engine::{MarketId, Nanos, NANOS_PER_DOLLAR};

/// Definition of one outcome in an event (e.g., "Trump", "Harris").
#[derive(Clone, Debug)]
pub struct OutcomeDef {
    pub name: String,
}

/// Definition of an event with multiple outcomes.
#[derive(Clone, Debug)]
pub struct EventDef {
    /// Event name (e.g., "2024 US Election")
    pub name: String,
    /// Event description
    pub description: String,
    /// Possible outcomes
    pub outcomes: Vec<OutcomeDef>,
    /// True probabilities for each outcome (sum to 1.0)
    pub true_probs: Vec<f64>,
    /// Index of the predetermined winner
    pub winner: usize,
    /// Batch at which the event resolves (None = resolve at end)
    pub resolve_at_batch: Option<usize>,
}

/// Visibility of a news item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NewsVisibility {
    /// All agents can see this news (updates public beliefs)
    Public,
    /// Only informed traders see this news (no public belief update)
    InformedOnly,
}

/// A news item that arrives at a specific batch and updates probability estimates.
#[derive(Clone, Debug)]
pub struct NewsItem {
    /// Index into Scenario::events
    pub event_index: usize,
    /// Batch at which this news arrives
    pub batch: usize,
    /// Updated probability estimates for the event's outcomes
    pub updated_probs: Vec<f64>,
    /// Who can see this news
    pub visibility: NewsVisibility,
}

/// A complete scenario definition for event-based simulations.
#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: String,
    pub events: Vec<EventDef>,
    pub news: Vec<NewsItem>,
    // Agent configuration
    pub num_informed: usize,
    pub num_noise: usize,
    pub num_mm: usize,
    pub num_batches: usize,
    pub seed: u64,
    pub initial_balance: i64,
    // Agent tuning params
    pub noise_activity_rate: f64,
    pub noise_max_qty: u64,
    pub noise_price_noise: Nanos,
    pub informed_min_edge: f64,
    pub informed_max_qty: u64,
    pub informed_max_position: i64,
    pub mm_half_spread: Nanos,
    pub mm_qty_per_side: u64,
    pub mm_budget: Nanos,
    pub mm_skew_factor: f64,
}

impl Scenario {
    /// Default agent parameters.
    pub fn defaults() -> Self {
        Self {
            name: String::new(),
            events: Vec::new(),
            news: Vec::new(),
            num_informed: 5,
            num_noise: 20,
            num_mm: 2,
            num_batches: 20,
            seed: 42,
            initial_balance: 5000 * NANOS_PER_DOLLAR as i64,
            noise_activity_rate: 0.6,
            noise_max_qty: 30,
            noise_price_noise: 50_000_000,
            informed_min_edge: 0.05,
            informed_max_qty: 30,
            informed_max_position: 200,
            mm_half_spread: 25_000_000,
            mm_qty_per_side: 100,
            mm_budget: 50_000 * NANOS_PER_DOLLAR,
            mm_skew_factor: 0.1,
        }
    }

    /// Simple coin flip: 1 binary event, no news.
    pub fn coin_flip() -> Self {
        Self {
            name: "coin_flip".to_string(),
            events: vec![EventDef {
                name: "Coin Flip".to_string(),
                description: "Fair coin flip".to_string(),
                outcomes: vec![
                    OutcomeDef {
                        name: "Heads".to_string(),
                    },
                    OutcomeDef {
                        name: "Tails".to_string(),
                    },
                ],
                true_probs: vec![0.5, 0.5],
                winner: 0,
                resolve_at_batch: None,
            }],
            news: Vec::new(),
            num_informed: 3,
            num_noise: 10,
            num_mm: 1,
            num_batches: 10,
            ..Self::defaults()
        }
    }

    /// 3-candidate election with public news and an informed-only leak.
    pub fn election() -> Self {
        Self {
            name: "election".to_string(),
            events: vec![EventDef {
                name: "2024 US Election".to_string(),
                description: "Three-way presidential election".to_string(),
                outcomes: vec![
                    OutcomeDef {
                        name: "Trump".to_string(),
                    },
                    OutcomeDef {
                        name: "Harris".to_string(),
                    },
                    OutcomeDef {
                        name: "Other".to_string(),
                    },
                ],
                true_probs: vec![0.45, 0.48, 0.07],
                winner: 1,
                resolve_at_batch: None,
            }],
            news: vec![
                // Batch 5: public news shifts probs from uniform
                NewsItem {
                    event_index: 0,
                    batch: 5,
                    updated_probs: vec![0.40, 0.45, 0.15],
                    visibility: NewsVisibility::Public,
                },
                // Batch 10: informed-only leak
                NewsItem {
                    event_index: 0,
                    batch: 10,
                    updated_probs: vec![0.42, 0.50, 0.08],
                    visibility: NewsVisibility::InformedOnly,
                },
            ],
            num_informed: 5,
            num_noise: 20,
            num_mm: 2,
            num_batches: 20,
            ..Self::defaults()
        }
    }

    /// Quick preset: 3 independent binary events, small agent counts, 5 batches.
    pub fn quick() -> Self {
        Self {
            name: "quick".to_string(),
            events: make_random_binary_events(3, 42),
            news: Vec::new(),
            num_informed: 2,
            num_noise: 5,
            num_mm: 1,
            num_batches: 5,
            initial_balance: 1000 * NANOS_PER_DOLLAR as i64,
            mm_budget: 10_000 * NANOS_PER_DOLLAR,
            mm_qty_per_side: 50,
            noise_max_qty: 20,
            ..Self::defaults()
        }
    }

    /// Standard preset: 10 independent binary events, moderate agents, 20 batches.
    pub fn standard() -> Self {
        Self {
            name: "standard".to_string(),
            events: make_random_binary_events(10, 42),
            news: Vec::new(),
            ..Self::defaults()
        }
    }

    /// Stress preset: 30 independent binary events, many agents, 100 batches.
    pub fn stress() -> Self {
        Self {
            name: "stress".to_string(),
            events: make_random_binary_events(30, 42),
            news: Vec::new(),
            num_informed: 10,
            num_noise: 50,
            num_mm: 3,
            num_batches: 100,
            initial_balance: 10_000 * NANOS_PER_DOLLAR as i64,
            noise_max_qty: 50,
            mm_budget: 100_000 * NANOS_PER_DOLLAR,
            mm_qty_per_side: 200,
            ..Self::defaults()
        }
    }

    /// Two events: one resolves mid-sim, one at end. Demonstrates mid-sim resolution + insider info.
    pub fn two_events_with_leak() -> Self {
        Self {
            name: "two_events_with_leak".to_string(),
            events: vec![
                EventDef {
                    name: "Fed Rate Decision".to_string(),
                    description: "Binary event resolving mid-simulation".to_string(),
                    outcomes: vec![
                        OutcomeDef {
                            name: "Rate Cut".to_string(),
                        },
                        OutcomeDef {
                            name: "No Cut".to_string(),
                        },
                    ],
                    true_probs: vec![0.65, 0.35],
                    winner: 0,
                    resolve_at_batch: Some(10),
                },
                EventDef {
                    name: "Tech IPO Outcome".to_string(),
                    description: "Three-outcome event resolving at end".to_string(),
                    outcomes: vec![
                        OutcomeDef {
                            name: "IPO Success".to_string(),
                        },
                        OutcomeDef {
                            name: "IPO Delayed".to_string(),
                        },
                        OutcomeDef {
                            name: "IPO Cancelled".to_string(),
                        },
                    ],
                    true_probs: vec![0.50, 0.35, 0.15],
                    winner: 0,
                    resolve_at_batch: None,
                },
            ],
            news: vec![
                // Batch 3: public news on Fed decision
                NewsItem {
                    event_index: 0,
                    batch: 3,
                    updated_probs: vec![0.70, 0.30],
                    visibility: NewsVisibility::Public,
                },
                // Batch 7: informed-only leak about Fed
                NewsItem {
                    event_index: 0,
                    batch: 7,
                    updated_probs: vec![0.85, 0.15],
                    visibility: NewsVisibility::InformedOnly,
                },
                // Batch 12: public news on IPO (after Fed has resolved)
                NewsItem {
                    event_index: 1,
                    batch: 12,
                    updated_probs: vec![0.55, 0.30, 0.15],
                    visibility: NewsVisibility::Public,
                },
            ],
            num_informed: 5,
            num_noise: 20,
            num_mm: 2,
            num_batches: 20,
            ..Self::defaults()
        }
    }
}

/// Generate N independent binary events with deterministic random true probabilities.
fn make_random_binary_events(n: usize, seed: u64) -> Vec<EventDef> {
    use rand::RngExt;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    (0..n)
        .map(|i| {
            let p: f64 = rng.random_range(0.1..0.9);
            let winner = if p >= 0.5 { 0 } else { 1 };
            EventDef {
                name: format!("Event {}", i),
                description: format!("Random binary event {}", i),
                outcomes: vec![
                    OutcomeDef {
                        name: "Yes".to_string(),
                    },
                    OutcomeDef {
                        name: "No".to_string(),
                    },
                ],
                true_probs: vec![p, 1.0 - p],
                winner,
                resolve_at_batch: None,
            }
        })
        .collect()
}

/// Bidirectional mapping between events and markets.
#[derive(Clone, Debug, Default)]
pub struct EventMarketMap {
    /// event_index -> list of MarketIds (one per outcome for N>2, one for binary)
    pub event_markets: Vec<Vec<MarketId>>,
    /// market -> (event_index, outcome_index)
    pub market_to_event: HashMap<MarketId, (usize, usize)>,
}

impl EventMarketMap {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Thin wrapper around a belief map: MarketId -> probability of YES.
pub struct PublicBeliefs {
    beliefs: HashMap<MarketId, f64>,
}

impl PublicBeliefs {
    /// Initialize with uniform priors based on event definitions and market mapping.
    pub fn from_events(events: &[EventDef], event_map: &EventMarketMap) -> Self {
        let mut beliefs = HashMap::new();
        for (event_idx, event) in events.iter().enumerate() {
            let n = event.outcomes.len();
            let market_ids = &event_map.event_markets[event_idx];

            if n == 2 {
                // Single binary market: 50/50
                beliefs.insert(market_ids[0], 0.5);
            } else {
                // N-outcome: 1/N each
                let uniform = 1.0 / n as f64;
                for &mid in market_ids {
                    beliefs.insert(mid, uniform);
                }
            }
        }
        Self { beliefs }
    }

    /// Update beliefs for an event based on new probability estimates.
    pub fn update(&mut self, event_index: usize, probs: &[f64], event_map: &EventMarketMap) {
        let market_ids = &event_map.event_markets[event_index];
        // For 2-outcome events, only one market — prob of YES is probs[0]
        if probs.len() == 2 && market_ids.len() == 1 {
            self.beliefs.insert(market_ids[0], probs[0]);
        } else {
            // N-outcome: each market's YES prob = that outcome's probability
            for (i, &mid) in market_ids.iter().enumerate() {
                if let Some(&p) = probs.get(i) {
                    self.beliefs.insert(mid, p);
                }
            }
        }
    }

    /// Remove beliefs for resolved markets.
    pub fn remove_markets(&mut self, market_ids: &[MarketId]) {
        for mid in market_ids {
            self.beliefs.remove(mid);
        }
    }

    /// Get the current belief map.
    pub fn as_map(&self) -> &HashMap<MarketId, f64> {
        &self.beliefs
    }
}
