//! Terminal reporting: statistics, formatting, and result/gap-analysis tables.

use std::collections::{HashMap, HashSet};

use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, Color, Table};

use matching_engine::{Fill, MarketId, Order, Problem};
use sybil_verifier::VerificationResult;

#[cfg(any(feature = "lp", feature = "conic"))]
use matching_solver::PipelineResult;

use crate::cli::SolverChoice;

/// Compute statistics about orders in the problem
#[cfg(any(feature = "lp", feature = "conic"))]
pub struct OrderStats {
    pub total_orders: usize,
    single_market_orders: usize,
    mm_order_ids: HashSet<u64>,
    pub user_order_count: usize,
    pub mm_order_count: usize,
}

#[cfg(any(feature = "lp", feature = "conic"))]
impl OrderStats {
    pub fn compute(problem: &Problem) -> Self {
        let mm_order_ids: HashSet<u64> = problem
            .mm_constraints
            .iter()
            .flat_map(|c| c.order_ids.iter().copied())
            .collect();

        let mm_count = problem
            .orders
            .iter()
            .filter(|o| mm_order_ids.contains(&o.id))
            .count();

        Self {
            total_orders: problem.orders.len(),
            single_market_orders: problem.orders.len(),
            mm_order_ids,
            user_order_count: problem.orders.len() - mm_count,
            mm_order_count: mm_count,
        }
    }

    fn is_mm_order(&self, order_id: u64) -> bool {
        self.mm_order_ids.contains(&order_id)
    }
}

/// Compute fill statistics from a result
#[cfg(any(feature = "lp", feature = "conic"))]
pub struct FillStats {
    // By fill status
    fully_filled: usize,
    partially_filled: usize,
    unfilled: usize,

    // By order type
    user_filled: usize,
    user_welfare: i64,
    user_volume: u64,
    mm_filled: usize,
    mm_welfare: i64,
    mm_volume: u64,

    // Markets with activity
    markets_with_volume: usize,
}

#[cfg(any(feature = "lp", feature = "conic"))]
impl FillStats {
    pub fn compute(problem: &Problem, result: &PipelineResult, order_stats: &OrderStats) -> Self {
        let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();

        let fill_map: HashMap<u64, u64> = result
            .result
            .fills
            .iter()
            .map(|f| (f.order_id, f.fill_qty.0))
            .collect();

        let mut fully_filled = 0;
        let mut partially_filled = 0;
        let mut unfilled = 0;

        let mut user_filled = 0;
        let mut user_welfare: i64 = 0;
        let mut user_volume: u64 = 0;
        let mut mm_filled = 0;
        let mut mm_welfare: i64 = 0;
        let mut mm_volume: u64 = 0;

        for order in &problem.orders {
            let fill_qty = fill_map.get(&order.id).copied().unwrap_or(0);
            let is_mm = order_stats.is_mm_order(order.id);

            if fill_qty == 0 {
                unfilled += 1;
            } else if fill_qty >= order.max_fill.0 {
                fully_filled += 1;
            } else {
                partially_filled += 1;
            }

            if fill_qty > 0 {
                // Find the fill to get welfare
                if let Some(fill) = result.result.fills.iter().find(|f| f.order_id == order.id) {
                    let welfare = fill.welfare(order);
                    if is_mm {
                        mm_filled += 1;
                        mm_welfare += welfare;
                        mm_volume += fill_qty;
                    } else {
                        user_filled += 1;
                        user_welfare += welfare;
                        user_volume += fill_qty;
                    }
                }
            }
        }

        // Count markets with volume
        let mut market_volumes: HashMap<_, u64> = HashMap::new();
        for fill in &result.result.fills {
            if let Some(order) = order_map.get(&fill.order_id) {
                for market_id in order.active_markets() {
                    *market_volumes.entry(market_id).or_default() += fill.fill_qty.0;
                }
            }
        }
        let markets_with_volume = market_volumes.values().filter(|&&v| v > 0).count();

        Self {
            fully_filled,
            partially_filled,
            unfilled,
            user_filled,
            user_welfare,
            user_volume,
            mm_filled,
            mm_welfare,
            mm_volume,
            markets_with_volume,
        }
    }
}

/// Select a representative subset of markets for detailed output.
/// Picks markets from different groups + some standalone markets.
#[cfg(any(feature = "lp", feature = "conic"))]
pub fn select_sample_markets(problem: &Problem, max_markets: usize) -> Vec<MarketId> {
    let mut selected = Vec::new();

    // First, pick one market from each group (up to half of max)
    let group_quota = max_markets / 2;
    for (i, group) in problem.market_groups.iter().enumerate() {
        if i >= group_quota {
            break;
        }
        if let Some(&market) = group.markets.first() {
            selected.push(market);
        }
    }

    // Then add standalone markets (first few not in any group)
    let markets_in_groups: HashSet<MarketId> = problem
        .market_groups
        .iter()
        .flat_map(|g| g.markets.iter().copied())
        .collect();

    for market in problem.markets.iter() {
        if selected.len() >= max_markets {
            break;
        }
        if !markets_in_groups.contains(&market.id) && !selected.contains(&market.id) {
            selected.push(market.id);
        }
    }

    // If still need more, add from groups
    for market in problem.markets.iter() {
        if selected.len() >= max_markets {
            break;
        }
        if !selected.contains(&market.id) {
            selected.push(market.id);
        }
    }

    selected
}

/// Print detailed market stats table
#[cfg(any(feature = "lp", feature = "conic"))]
pub fn print_market_details(
    problem: &Problem,
    result: &PipelineResult,
    sample_markets: &[MarketId],
) {
    if sample_markets.is_empty() {
        return;
    }

    println!(
        "\nSample Market Details ({} of {}):",
        sample_markets.len(),
        problem.markets.len()
    );

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec![
            "Market", "Group", "P(YES)", "P(NO)", "Volume", "Welfare", "Liq Rem",
        ]);

    // Build order map for volume/welfare calculation
    let _order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();
    let fill_map: HashMap<u64, u64> = result
        .result
        .fills
        .iter()
        .map(|f| (f.order_id, f.fill_qty.0))
        .collect();

    // Find which group each market belongs to
    let market_to_group: HashMap<MarketId, &str> = problem
        .market_groups
        .iter()
        .flat_map(|g| g.markets.iter().map(|&m| (m, g.name.as_str())))
        .collect();

    for &market_id in sample_markets {
        // Get prices
        let (yes_price, no_price) = if let Some(ref pd) = result.price_discovery {
            if let Some(prices) = pd.prices.get(&market_id) {
                (
                    prices.first().map(|n| n.0).unwrap_or(0),
                    prices.get(1).map(|n| n.0).unwrap_or(0),
                )
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };

        // Calculate volume and welfare for this market
        let mut volume: u64 = 0;
        let mut welfare: i64 = 0;
        for order in &problem.orders {
            if order.num_markets == 1 && order.markets[0] == market_id {
                if let Some(&fill_qty) = fill_map.get(&order.id) {
                    volume += fill_qty;
                    if let Some(fill) = result.result.fills.iter().find(|f| f.order_id == order.id)
                    {
                        welfare += fill.welfare(order);
                    }
                }
            }
        }

        // Liquidity pool has been removed; show N/A
        let liq_yes: u64 = 0;
        let liq_no: u64 = 0;

        let group_name = market_to_group.get(&market_id).copied().unwrap_or("-");

        // Format market name
        let market_name = problem
            .markets
            .iter()
            .find(|m| m.id == market_id)
            .map(|m| m.name.as_str())
            .unwrap_or("?");

        table.add_row(vec![
            Cell::new(market_name),
            Cell::new(group_name),
            Cell::new(format!("{:.1}%", yes_price as f64 / 1e7)),
            Cell::new(format!("{:.1}%", no_price as f64 / 1e7)),
            Cell::new(format_qty(volume)),
            Cell::new(format_welfare(welfare)),
            Cell::new(format!("{}/{}", format_qty(liq_yes), format_qty(liq_no))),
        ]);
    }

    println!("{table}");
}

#[cfg(any(feature = "lp", feature = "conic"))]
pub fn print_verification_result(result: &VerificationResult) {
    println!();
    println!("Result Verification (ZK-ready):");
    println!("─────────────────────────────────────────");

    if result.valid {
        println!("  Status: \u{2713} VALID");
        println!("  Fills verified: {}", result.stats.fills_checked);
        println!(
            "  MM constraints verified: {}",
            result.stats.mm_constraints_checked
        );
        println!(
            "  Welfare: computed={} reported={}",
            format_welfare(result.stats.computed_welfare),
            format_welfare(result.stats.reported_welfare)
        );
        if let Some(delta) = result.stats.market_group_avg_delta {
            let pct = delta as f64 / 1e7; // nanos to percentage points
            println!("  Market group avg |sum-1|: {:.2}pp", pct);
        }
    } else {
        println!(
            "  Status: \u{2717} INVALID ({} violations)",
            result.violations.len()
        );
        println!();
        println!("  Violations:");
        for (i, violation) in result.violations.iter().enumerate().take(10) {
            println!("    {}. {:?}: {}", i + 1, violation.kind, violation.details);
        }
        if result.violations.len() > 10 {
            println!("    ... and {} more", result.violations.len() - 10);
        }
    }
}

#[cfg(any(feature = "lp", feature = "conic"))]
pub fn print_problem_summary(problem: &Problem, stats: &OrderStats) {
    println!("Problem Summary:");
    println!("  Markets: {}", problem.markets.len());
    if !problem.market_groups.is_empty() {
        let markets_in_groups: usize = problem.market_groups.iter().map(|g| g.markets.len()).sum();
        println!(
            "    In {} multi-outcome groups: {} markets",
            problem.market_groups.len(),
            markets_in_groups
        );
    }
    println!("  Total orders: {}", stats.total_orders);
    println!("    User orders: {}", stats.user_order_count);
    println!("    MM orders: {}", stats.mm_order_count);
    println!("    Single-market: {}", stats.single_market_orders);
    println!("  MM constraints: {}", problem.mm_constraints.len());
    println!();
}

#[cfg(any(feature = "lp", feature = "conic"))]
pub fn print_fill_stats(stats: &FillStats, order_stats: &OrderStats, num_markets: usize) {
    println!("Fill Statistics:");
    println!("─────────────────────────────────────────");

    // Fill status breakdown
    let total = order_stats.total_orders;
    println!(
        "  Fill Status:    Full {:>4} ({:>5.1}%)  Partial {:>4} ({:>5.1}%)  Unfilled {:>4} ({:>5.1}%)",
        stats.fully_filled,
        pct(stats.fully_filled, total),
        stats.partially_filled,
        pct(stats.partially_filled, total),
        stats.unfilled,
        pct(stats.unfilled, total)
    );
    println!(
        "  Markets active: {}/{} ({:.1}%)",
        stats.markets_with_volume,
        num_markets,
        pct(stats.markets_with_volume, num_markets)
    );

    // User vs MM breakdown
    println!();
    println!("  By Order Type:");

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec!["Type", "Filled", "Volume", "Welfare"]);

    table.add_row(vec![
        Cell::new("User"),
        Cell::new(format!(
            "{}/{} ({:.1}%)",
            stats.user_filled,
            order_stats.user_order_count,
            pct(stats.user_filled, order_stats.user_order_count)
        )),
        Cell::new(format_qty(stats.user_volume)),
        Cell::new(format_welfare(stats.user_welfare)).fg(Color::Green),
    ]);

    table.add_row(vec![
        Cell::new("MM"),
        Cell::new(format!(
            "{}/{} ({:.1}%)",
            stats.mm_filled,
            order_stats.mm_order_count,
            pct(stats.mm_filled, order_stats.mm_order_count)
        )),
        Cell::new(format_qty(stats.mm_volume)),
        Cell::new(format_welfare(stats.mm_welfare)).fg(Color::Yellow),
    ]);

    println!("{table}");

    // Market activity
    println!();
    let total_welfare = stats.user_welfare + stats.mm_welfare;
    println!("  Total welfare: {}", format_welfare(total_welfare));
    println!(
        "  Total volume: {}",
        format_qty(stats.user_volume + stats.mm_volume)
    );
}

#[cfg(any(feature = "lp", feature = "conic"))]
fn pct(num: usize, denom: usize) -> f64 {
    if denom > 0 {
        num as f64 / denom as f64 * 100.0
    } else {
        0.0
    }
}

pub fn format_welfare(w: i64) -> String {
    // Welfare is in nanos (1e9 = $1)
    let dollars = w as f64 / 1_000_000_000.0;
    if dollars.abs() >= 1_000_000.0 {
        format!("${:.2}M", dollars / 1_000_000.0)
    } else if dollars.abs() >= 1_000.0 {
        format!("${:.2}K", dollars / 1_000.0)
    } else if dollars.abs() >= 1.0 {
        format!("${:.2}", dollars)
    } else {
        format!("{:.0}¢", dollars * 100.0)
    }
}

pub fn format_price(p: u64) -> String {
    // Prices are in nanos (1e9 = $1)
    let dollars = p as f64 / 1_000_000_000.0;
    if dollars >= 1_000_000.0 {
        format!("${:.2}M", dollars / 1_000_000.0)
    } else if dollars >= 1_000.0 {
        format!("${:.2}K", dollars / 1_000.0)
    } else if dollars >= 1.0 {
        format!("${:.2}", dollars)
    } else {
        format!("{:.0}¢", dollars * 100.0)
    }
}

pub fn format_qty(q: u64) -> String {
    if q >= 1_000_000 {
        format!("{:.2}M", q as f64 / 1_000_000.0)
    } else if q >= 1_000 {
        format!("{:.2}K", q as f64 / 1_000.0)
    } else {
        format!("{}", q)
    }
}

#[derive(Default)]
pub struct SolverResults {
    pub name: String,
    pub total_welfare: i64,
    pub total_filled: usize,
    pub total_orders: usize,
    pub total_volume: u64,
    pub total_time_secs: f64,
    pub batches: usize,
    pub verification: Option<VerificationResult>,
}

impl SolverResults {
    fn mean_welfare(&self) -> f64 {
        if self.batches > 0 {
            self.total_welfare as f64 / self.batches as f64
        } else {
            0.0
        }
    }

    fn fill_rate(&self) -> f64 {
        if self.total_orders > 0 {
            self.total_filled as f64 / self.total_orders as f64 * 100.0
        } else {
            0.0
        }
    }

    fn mean_time(&self) -> f64 {
        if self.batches > 0 {
            self.total_time_secs / self.batches as f64
        } else {
            0.0
        }
    }
}

/// Per-solver data for gap analysis.
pub struct SolverDetail {
    pub name: String,
    pub result: matching_solver::MatchingResult,
    pub clearing_prices: HashMap<MarketId, Vec<matching_engine::Nanos>>,
    pub is_valid: bool,
}

/// Data for gap analysis between solvers (collected when --solver all -v).
pub struct GapAnalysisData {
    pub problem: Problem,
    pub solver_details: Vec<SolverDetail>,
}

pub fn print_results(results: &[SolverResults], choice: &SolverChoice) {
    println!("\n========================================");
    println!("              RESULTS                   ");
    println!("========================================\n");

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec![
            "Solver",
            "Welfare",
            "Fill %",
            "Volume",
            "Time (avg)",
            "Verified",
        ]);

    // Best welfare among VALID solvers only (invalid results may have inflated welfare)
    let best_valid_welfare = results
        .iter()
        .filter(|r| r.verification.as_ref().is_some_and(|v| v.valid))
        .map(|r| r.mean_welfare())
        .fold(0.0, f64::max);
    let best_welfare = if best_valid_welfare > 0.0 {
        best_valid_welfare
    } else {
        results.iter().map(|r| r.mean_welfare()).fold(0.0, f64::max)
    };

    for result in results {
        let welfare = result.mean_welfare();
        let gap = if best_welfare > 0.0 {
            (best_welfare - welfare) / best_welfare * 100.0
        } else {
            0.0
        };

        let welfare_str = if gap < 0.1 {
            format_welfare(welfare as i64)
        } else {
            format!("{} (-{:.1}%)", format_welfare(welfare as i64), gap)
        };

        let is_valid = result.verification.as_ref().is_some_and(|v| v.valid);
        let welfare_cell = if !is_valid {
            Cell::new(&welfare_str).fg(Color::DarkRed)
        } else if gap < 0.1 {
            Cell::new(&welfare_str).fg(Color::Green)
        } else if gap < 5.0 {
            Cell::new(&welfare_str).fg(Color::Yellow)
        } else {
            Cell::new(&welfare_str).fg(Color::Red)
        };

        let fill_rate = result.fill_rate();
        let fill_cell = if fill_rate >= 90.0 {
            Cell::new(format!("{:.1}%", fill_rate)).fg(Color::Green)
        } else if fill_rate >= 70.0 {
            Cell::new(format!("{:.1}%", fill_rate)).fg(Color::Yellow)
        } else {
            Cell::new(format!("{:.1}%", fill_rate)).fg(Color::Red)
        };

        let volume = if result.batches > 0 {
            result.total_volume / result.batches as u64
        } else {
            0
        };

        let verified_cell = match &result.verification {
            Some(v) if v.valid => Cell::new("\u{2713} VALID").fg(Color::Green),
            Some(v) => {
                let n = v.violations.len();
                Cell::new(format!("\u{2717} {} violations", n)).fg(Color::Red)
            }
            None => Cell::new("-"),
        };

        table.add_row(vec![
            Cell::new(&result.name),
            welfare_cell,
            fill_cell,
            Cell::new(format_qty(volume)),
            Cell::new(format!("{:.3}s", result.mean_time())),
            verified_cell,
        ]);
    }

    println!("{table}");

    // Print violation details for invalid solvers
    for result in results {
        if let Some(ref v) = result.verification {
            if !v.valid {
                println!();
                println!("{}: {} violations", result.name, v.violations.len());
                for (i, violation) in v.violations.iter().enumerate().take(10) {
                    println!("  {}. {:?}: {}", i + 1, violation.kind, violation.details);
                }
                if v.violations.len() > 10 {
                    println!("  ... and {} more", v.violations.len() - 10);
                }
            }
        }
    }

    if *choice == SolverChoice::All && results.len() >= 2 {
        // Find best VALID solver by welfare
        let valid_results: Vec<_> = results
            .iter()
            .filter(|r| r.verification.as_ref().is_some_and(|v| v.valid))
            .collect();

        if let Some(best) = valid_results.iter().max_by(|a, b| {
            a.mean_welfare()
                .partial_cmp(&b.mean_welfare())
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            println!();
            println!(
                "Best valid solver: {} (${:.2}K welfare)",
                best.name,
                best.mean_welfare() / 1e9 / 1e3
            );
        }
    }
}

/// Print gap analysis comparing the best valid solver against each other valid solver.
pub fn print_gap_analysis(data: &GapAnalysisData) {
    // Find best valid solver by welfare
    let best_idx = data
        .solver_details
        .iter()
        .enumerate()
        .filter(|(_, d)| d.is_valid)
        .max_by_key(|(_, d)| d.result.total_welfare())
        .map(|(i, _)| i);

    let Some(best_idx) = best_idx else {
        println!("\nNo valid solver results for gap analysis.");
        return;
    };

    for (i, other) in data.solver_details.iter().enumerate() {
        if i == best_idx {
            continue;
        }
        if !other.is_valid {
            continue; // skip invalid solvers
        }
        print_solver_diff(&data.problem, &data.solver_details[best_idx], other);
    }
}

/// Print a detailed comparison between two solver results.
fn print_solver_diff(problem: &Problem, best: &SolverDetail, other: &SolverDetail) {
    let best_name = &best.name;
    let other_name = &other.name;
    let best_result = &best.result;
    let other_result = &other.result;

    let order_map: HashMap<u64, &Order> = problem.orders.iter().map(|o| (o.id, o)).collect();
    let mm_order_ids: HashSet<u64> = problem
        .mm_constraints
        .iter()
        .flat_map(|c| c.order_ids.iter().copied())
        .collect();
    let market_names: HashMap<MarketId, &str> = problem
        .markets
        .iter()
        .map(|m| (m.id, m.name.as_str()))
        .collect();

    // Use total_welfare which accounts for minting cost
    let best_welfare = best_result.total_welfare();
    let other_welfare = other_result.total_welfare();
    let gap = best_welfare - other_welfare;
    let gap_pct = if best_welfare > 0 {
        gap as f64 / best_welfare as f64 * 100.0
    } else {
        0.0
    };

    // ── Header ──
    println!();
    println!(
        "══════ Gap Analysis: {} ({}) vs {} ({}) ══════",
        best_name,
        format_welfare(best_welfare),
        other_name,
        format_welfare(other_welfare)
    );
    println!("Total gap: {} ({:.1}%)", format_welfare(gap), gap_pct);
    println!();

    // ── Welfare Breakdown ──
    let breakdown = |fills: &[Fill]| -> (i64, i64) {
        let mut user_w: i64 = 0;
        let mut mm_w: i64 = 0;
        for f in fills {
            if let Some(order) = order_map.get(&f.order_id) {
                let w = f.welfare(order);
                if mm_order_ids.contains(&f.order_id) {
                    mm_w += w;
                } else {
                    user_w += w;
                }
            }
        }
        (user_w, mm_w)
    };

    let (best_user, best_mm) = breakdown(&best_result.fills);
    let (other_user, other_mm) = breakdown(&other_result.fills);

    println!("Welfare Breakdown:");
    println!(
        "  {:<18} {:>10} {:>10} {:>10}",
        "", best_name, other_name, "Gap"
    );
    println!(
        "  {:<18} {:>10} {:>10} {:>10}",
        "User orders",
        format_welfare(best_user),
        format_welfare(other_user),
        format_welfare(best_user - other_user)
    );
    println!(
        "  {:<18} {:>10} {:>10} {:>10}",
        "MM orders",
        format_welfare(best_mm),
        format_welfare(other_mm),
        format_welfare(best_mm - other_mm)
    );
    println!();

    // ── Per-Market Comparison ──
    // Build per-market welfare maps
    let market_welfare = |fills: &[Fill]| -> HashMap<MarketId, i64> {
        let mut map: HashMap<MarketId, i64> = HashMap::new();
        for f in fills {
            if let Some(order) = order_map.get(&f.order_id) {
                let w = f.welfare(order);
                if order.num_markets == 1 {
                    *map.entry(order.markets[0]).or_default() += w;
                } else {
                    let n = order.num_markets as i64;
                    for mid in order.active_markets() {
                        *map.entry(mid).or_default() += w / n;
                    }
                }
            }
        }
        map
    };

    let best_market_w = market_welfare(&best_result.fills);
    let other_market_w = market_welfare(&other_result.fills);

    let all_markets: HashSet<MarketId> = best_market_w
        .keys()
        .chain(other_market_w.keys())
        .copied()
        .collect();

    let mut market_gaps: Vec<_> = all_markets
        .iter()
        .map(|&mid| {
            let bw = best_market_w.get(&mid).copied().unwrap_or(0);
            let ow = other_market_w.get(&mid).copied().unwrap_or(0);
            (mid, bw, ow, bw - ow)
        })
        .collect();
    market_gaps.sort_by_key(|entry| std::cmp::Reverse(entry.3.abs()));

    let best_price_yes = |mid: &MarketId| -> u64 {
        best.clearing_prices
            .get(mid)
            .and_then(|p| p.first().map(|n| n.0))
            .unwrap_or(0)
    };
    let other_price_yes = |mid: &MarketId| -> u64 {
        other
            .clearing_prices
            .get(mid)
            .and_then(|p| p.first().map(|n| n.0))
            .unwrap_or(0)
    };

    println!("Per-Market Comparison (top 10 by gap):");
    println!(
        "  {:<8} │ {:>11} │ {:>12} │ {:>8} │ {:>9} │ {:>10} │ {:>8}",
        "Market", "Best P(YES)", "Other P(YES)", "ΔPrice", "Best W$", "Other W$", "Gap$"
    );
    println!(
        "  {:<8}─┼─{:─>11}─┼─{:─>12}─┼─{:─>8}─┼─{:─>9}─┼─{:─>10}─┼─{:─>8}",
        "────────", "", "", "", "", "", ""
    );

    for (mid, bw, ow, gap_w) in market_gaps.iter().take(10) {
        let name = market_names.get(mid).copied().unwrap_or("?");
        let bp = best_price_yes(mid);
        let op = other_price_yes(mid);
        let dp_pp = (bp as f64 - op as f64) / 1e7;

        println!(
            "  {:<8} │ {:>10.1}% │ {:>11.1}% │ {:>+7.1}pp │ {:>9} │ {:>10} │ {:>8}",
            name,
            bp as f64 / 1e7,
            op as f64 / 1e7,
            dp_pp,
            format_welfare(*bw),
            format_welfare(*ow),
            format_welfare(*gap_w)
        );
    }
    println!();

    // ── MM Budget ──
    if !problem.mm_constraints.is_empty() {
        println!("MM Budget:");

        let mm_fills_map =
            |fills: &[Fill]| -> HashMap<u64, (matching_engine::Nanos, matching_engine::Qty)> {
                fills
                    .iter()
                    .filter(|f| mm_order_ids.contains(&f.order_id))
                    .map(|f| (f.order_id, (f.fill_price, f.fill_qty)))
                    .collect()
            };

        let best_mm_fills = mm_fills_map(&best_result.fills);
        let other_mm_fills = mm_fills_map(&other_result.fills);

        for mm in &problem.mm_constraints {
            let best_cap = mm.capital_used(&best_mm_fills);
            let other_cap = mm.capital_used(&other_mm_fills);
            let budget = mm.max_capital;

            let best_active: usize = mm
                .order_ids
                .iter()
                .filter(|id| best_mm_fills.contains_key(id))
                .count();
            let other_active: usize = mm
                .order_ids
                .iter()
                .filter(|id| other_mm_fills.contains_key(id))
                .count();

            let best_util = if budget.0 > 0 {
                best_cap.0 as f64 / budget.0 as f64 * 100.0
            } else {
                0.0
            };
            let other_util = if budget.0 > 0 {
                other_cap.0 as f64 / budget.0 as f64 * 100.0
            } else {
                0.0
            };

            println!(
                "  {}: {} of {} ({:.1}%), {} orders filled",
                best_name,
                format_price(best_cap.0),
                format_price(budget.0),
                best_util,
                best_active
            );
            println!(
                "  {}: {} of {} ({:.1}%), {} orders filled",
                other_name,
                format_price(other_cap.0),
                format_price(budget.0),
                other_util,
                other_active
            );
        }
        println!();
    }

    // ── Differential Fills ──
    let best_fill_ids: HashSet<u64> = best_result.fills.iter().map(|f| f.order_id).collect();
    let other_fill_ids: HashSet<u64> = other_result.fills.iter().map(|f| f.order_id).collect();

    // Fills in best but not in other, sorted by welfare
    let mut best_only: Vec<(&Fill, &Order, i64, String, &str)> = best_result
        .fills
        .iter()
        .filter(|f| !other_fill_ids.contains(&f.order_id))
        .filter_map(|f| {
            order_map.get(&f.order_id).map(|&order| {
                let w = f.welfare(order);
                let market_name = if order.num_markets == 1 {
                    market_names
                        .get(&order.markets[0])
                        .unwrap_or(&"?")
                        .to_string()
                } else {
                    format!("multi({})", order.num_markets)
                };
                let order_type = if order.is_seller() { "sell" } else { "buy" };
                (f, order, w, market_name, order_type)
            })
        })
        .collect();
    best_only.sort_by_key(|entry| std::cmp::Reverse(entry.2.abs()));

    if !best_only.is_empty() {
        println!(
            "Top Differential Fills (in {} only, by welfare):",
            best_name
        );
        println!(
            "  {:>3} │ {:>7} │ {:>8} │ {:>8} │ {:>7} │ {:>7} │ {:>5} │ {:>8}",
            "#", "Order", "Type", "Market", "Limit", "Price", "Qty", "W$"
        );
        println!(
            "  {:─>3}─┼─{:─>7}─┼─{:─>8}─┼─{:─>8}─┼─{:─>7}─┼─{:─>7}─┼─{:─>5}─┼─{:─>8}",
            "", "", "", "", "", "", "", ""
        );

        for (i, (fill, order, welfare, market, order_type)) in best_only.iter().take(15).enumerate()
        {
            println!(
                "  {:>3} │ {:>7} │ {:>8} │ {:>8} │ {:>6.1}c │ {:>6.1}c │ {:>5} │ {:>8}",
                i + 1,
                fill.order_id,
                order_type,
                market,
                order.limit_price.0 as f64 / 1e7,
                fill.fill_price.0 as f64 / 1e7,
                fill.fill_qty,
                format_welfare(*welfare)
            );
        }
        println!();
    }

    // Summary of fills unique to other
    let other_only_welfare: i64 = other_result
        .fills
        .iter()
        .filter(|f| !best_fill_ids.contains(&f.order_id))
        .filter_map(|f| order_map.get(&f.order_id).map(|o| f.welfare(o)))
        .sum();
    let other_only_count = other_result
        .fills
        .iter()
        .filter(|f| !best_fill_ids.contains(&f.order_id))
        .count();

    if other_only_count > 0 {
        println!(
            "Fills unique to {}: {} orders, {} welfare",
            other_name,
            other_only_count,
            format_welfare(other_only_welfare)
        );
        println!();
    }
}
