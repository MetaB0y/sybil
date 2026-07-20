//! Read-only load test for the sequencer/history-service isolation boundary.

use std::env;
use std::error::Error;
use std::io;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use goose::metrics::{
    GooseCoordinatedOmissionMitigation, GooseMetrics, GooseRequestMetricTimingData,
};
use goose::prelude::*;

const CONTROL_NAME: &str = "control: sequencer health";
const HISTORY_PREFIX: &str = "history:";

static TARGET: OnceLock<Target> = OnceLock::new();

#[derive(Clone)]
struct Target {
    host: String,
    account_id: u64,
    market_id: u32,
    bearer_token: String,
}

#[derive(Clone)]
struct Thresholds {
    baseline_samples: usize,
    baseline_interval_ms: u64,
    minimum_health_samples: usize,
    minimum_history_samples: usize,
    maximum_health_p95_ms: usize,
    maximum_health_p95_increase_ms: usize,
}

#[derive(Clone)]
struct LoadConfig {
    target: Target,
    thresholds: Thresholds,
}

impl LoadConfig {
    fn from_env() -> Result<Self, Box<dyn Error>> {
        let host = required_env("SYBIL_LOADTEST_HOST")?
            .trim()
            .trim_end_matches('/')
            .to_owned();
        if !(host.starts_with("http://") || host.starts_with("https://")) {
            return Err(invalid_input(
                "SYBIL_LOADTEST_HOST must start with http:// or https://",
            ));
        }
        let account_id = parse_required_env("SYBIL_LOADTEST_ACCOUNT_ID")?;
        let market_id = parse_env("SYBIL_LOADTEST_MARKET_ID", 0_u32)?;
        let bearer_token = required_env("SYBIL_LOADTEST_BEARER_TOKEN")?;

        let thresholds = Thresholds {
            baseline_samples: parse_env("SYBIL_LOADTEST_BASELINE_SAMPLES", 30_usize)?,
            baseline_interval_ms: parse_env("SYBIL_LOADTEST_BASELINE_INTERVAL_MS", 20_u64)?,
            minimum_health_samples: parse_env("SYBIL_LOADTEST_MIN_HEALTH_SAMPLES", 100_usize)?,
            minimum_history_samples: parse_env("SYBIL_LOADTEST_MIN_HISTORY_SAMPLES", 500_usize)?,
            maximum_health_p95_ms: parse_env("SYBIL_LOADTEST_MAX_HEALTH_P95_MS", 250_usize)?,
            maximum_health_p95_increase_ms: parse_env(
                "SYBIL_LOADTEST_MAX_HEALTH_P95_INCREASE_MS",
                100_usize,
            )?,
        };
        if thresholds.baseline_samples == 0 {
            return Err(invalid_input(
                "SYBIL_LOADTEST_BASELINE_SAMPLES must be greater than zero",
            ));
        }

        Ok(Self {
            target: Target {
                host,
                account_id,
                market_id,
                bearer_token,
            },
            thresholds,
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| invalid_input("failed to install the rustls ring crypto provider"))?;

    let config = LoadConfig::from_env()?;
    let baseline_p95_ms = measure_baseline(&config).await?;
    TARGET
        .set(config.target.clone())
        .map_err(|_| invalid_input("load-test target was initialized twice"))?;

    eprintln!(
        "history isolation load: target={} account={} market={} baseline_health_p95={}ms",
        config.target.host, config.target.account_id, config.target.market_id, baseline_p95_ms,
    );

    let scenario = scenario!("History read isolation")
        .set_host(&config.target.host)
        .register_transaction(transaction!(account_fills).set_weight(3)?)
        .register_transaction(transaction!(account_events).set_weight(3)?)
        .register_transaction(transaction!(account_equity).set_weight(2)?)
        .register_transaction(transaction!(price_history).set_weight(3)?)
        .register_transaction(transaction!(price_candles).set_weight(2)?)
        .register_transaction(transaction!(sequencer_health).set_weight(3)?);

    let metrics = GooseAttack::initialize()?
        .register_scenario(scenario)
        .set_default(GooseDefault::Users, 32_usize)?
        .set_default(GooseDefault::HatchRate, "8")?
        .set_default(GooseDefault::RunTime, 30_usize)?
        .set_default(GooseDefault::ReportFile, "target/history-load-report.html")?
        .set_default(
            GooseDefault::CoordinatedOmissionMitigation,
            GooseCoordinatedOmissionMitigation::Average,
        )?
        .execute()
        .await?;

    evaluate(&metrics, baseline_p95_ms, &config.thresholds)?;
    Ok(())
}

async fn measure_baseline(config: &LoadConfig) -> Result<usize, Box<dyn Error>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let url = format!("{}/v1/health", config.target.host);
    let mut timings = Vec::with_capacity(config.thresholds.baseline_samples);
    for sample in 0..config.thresholds.baseline_samples {
        let started = Instant::now();
        client.get(&url).send().await?.error_for_status()?;
        timings.push(duration_ms_ceil(started.elapsed()));
        if sample + 1 < config.thresholds.baseline_samples
            && config.thresholds.baseline_interval_ms > 0
        {
            tokio::time::sleep(Duration::from_millis(
                config.thresholds.baseline_interval_ms,
            ))
            .await;
        }
    }
    Ok(percentile_samples(&mut timings, 95))
}

async fn account_fills(user: &mut GooseUser) -> TransactionResult {
    let target = target();
    named_get(
        user,
        &format!("/v1/accounts/{}/fills?limit=500", target.account_id),
        "history: account fills",
    )
    .await
}

async fn account_events(user: &mut GooseUser) -> TransactionResult {
    let target = target();
    named_get(
        user,
        &format!("/v1/accounts/{}/events?limit=500", target.account_id),
        "history: account events",
    )
    .await
}

async fn account_equity(user: &mut GooseUser) -> TransactionResult {
    let target = target();
    named_get(
        user,
        &format!("/v1/accounts/{}/equity?range=all", target.account_id),
        "history: account equity",
    )
    .await
}

async fn price_history(user: &mut GooseUser) -> TransactionResult {
    let target = target();
    named_get(
        user,
        &format!("/v1/markets/{}/prices/history?limit=500", target.market_id),
        "history: market prices",
    )
    .await
}

async fn price_candles(user: &mut GooseUser) -> TransactionResult {
    let target = target();
    named_get(
        user,
        &format!(
            "/v1/markets/{}/prices/candles?resolution=1m&limit=500",
            target.market_id
        ),
        "history: market candles",
    )
    .await
}

async fn sequencer_health(user: &mut GooseUser) -> TransactionResult {
    named_get(user, "/v1/health", CONTROL_NAME).await
}

async fn named_get(user: &mut GooseUser, path: &str, name: &str) -> TransactionResult {
    let request_builder = user
        .get_request_builder(&GooseMethod::Get, path)?
        .bearer_auth(&target().bearer_token);
    let request = GooseRequest::builder()
        .path(path)
        .name(name)
        .set_request_builder(request_builder)
        .error_on_fail()
        .build();
    user.request(request).await?;
    Ok(())
}

fn evaluate(
    metrics: &GooseMetrics,
    baseline_p95_ms: usize,
    thresholds: &Thresholds,
) -> Result<(), Box<dyn Error>> {
    let control = metrics
        .requests
        .values()
        .find(|request| request.path == CONTROL_NAME)
        .ok_or_else(|| invalid_input("Goose recorded no sequencer-health control request"))?;
    let control_timing = control
        .coordinated_omission_data
        .as_ref()
        .unwrap_or(&control.raw_data);
    let loaded_p95_ms = percentile_timing(control_timing, 95);
    let actual_health_samples = control.raw_data.counter;
    let history_samples: usize = metrics
        .requests
        .values()
        .filter(|request| request.path.starts_with(HISTORY_PREFIX))
        .map(|request| request.raw_data.counter)
        .sum();
    let failures: usize = metrics
        .requests
        .values()
        .map(|request| request.fail_count)
        .sum();
    let relative_ceiling =
        baseline_p95_ms.saturating_add(thresholds.maximum_health_p95_increase_ms);

    eprintln!(
        "history isolation verdict: baseline_health_p95={}ms loaded_health_p95={}ms \
         health_samples={} history_samples={} failures={} absolute_ceiling={}ms \
         increase_ceiling={}ms",
        baseline_p95_ms,
        loaded_p95_ms,
        actual_health_samples,
        history_samples,
        failures,
        thresholds.maximum_health_p95_ms,
        relative_ceiling,
    );

    let mut violations = Vec::new();
    if actual_health_samples < thresholds.minimum_health_samples {
        violations.push(format!(
            "only {actual_health_samples} health samples (minimum {})",
            thresholds.minimum_health_samples
        ));
    }
    if history_samples < thresholds.minimum_history_samples {
        violations.push(format!(
            "only {history_samples} history samples (minimum {})",
            thresholds.minimum_history_samples
        ));
    }
    if failures > 0 {
        violations.push(format!("{failures} HTTP requests failed"));
    }
    if loaded_p95_ms > thresholds.maximum_health_p95_ms {
        violations.push(format!(
            "loaded health p95 {loaded_p95_ms}ms exceeds absolute ceiling {}ms",
            thresholds.maximum_health_p95_ms
        ));
    }
    if loaded_p95_ms > relative_ceiling {
        violations.push(format!(
            "loaded health p95 {loaded_p95_ms}ms exceeds baseline {baseline_p95_ms}ms + {}ms",
            thresholds.maximum_health_p95_increase_ms
        ));
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(invalid_input(format!(
            "history isolation load test failed: {}",
            violations.join("; ")
        )))
    }
}

fn percentile_timing(timing: &GooseRequestMetricTimingData, percentile: usize) -> usize {
    if timing.counter == 0 {
        return 0;
    }
    let wanted = timing
        .counter
        .saturating_mul(percentile)
        .div_ceil(100)
        .max(1);
    let mut seen = 0_usize;
    for (millis, count) in &timing.times {
        seen = seen.saturating_add(*count);
        if seen >= wanted {
            return *millis;
        }
    }
    timing.maximum_time
}

fn percentile_samples(samples: &mut [usize], percentile: usize) -> usize {
    samples.sort_unstable();
    let index = samples
        .len()
        .saturating_mul(percentile)
        .div_ceil(100)
        .saturating_sub(1)
        .min(samples.len().saturating_sub(1));
    samples[index]
}

fn duration_ms_ceil(duration: Duration) -> usize {
    usize::try_from(duration.as_millis())
        .unwrap_or(usize::MAX)
        .max(1)
}

fn target() -> &'static Target {
    TARGET
        .get()
        .expect("load-test target is initialized before Goose starts")
}

fn required_env(name: &str) -> Result<String, Box<dyn Error>> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid_input(format!("{name} is required")))
}

fn parse_required_env<T>(name: &str) -> Result<T, Box<dyn Error>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    required_env(name)?
        .parse()
        .map_err(|error| invalid_input(format!("{name} must be a valid number: {error}")))
}

fn parse_env<T>(name: &str, default: T) -> Result<T, Box<dyn Error>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => value
            .parse()
            .map_err(|error| invalid_input(format!("{name} must be a valid number: {error}"))),
        _ => Ok(default),
    }
}

fn invalid_input(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn sample_percentile_uses_the_slowest_value_within_the_requested_rank() {
        let mut samples = vec![
            40, 1, 3, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
        ];
        assert_eq!(percentile_samples(&mut samples, 95), 19);
    }

    #[test]
    fn goose_histogram_percentile_counts_buckets() {
        let timing = GooseRequestMetricTimingData {
            times: BTreeMap::from([(2, 90), (25, 5), (80, 5)]),
            minimum_time: 2,
            maximum_time: 80,
            total_time: 6_325,
            counter: 100,
        };
        assert_eq!(percentile_timing(&timing, 95), 25);
        assert_eq!(percentile_timing(&timing, 99), 80);
    }

    #[test]
    fn elapsed_time_rounds_sub_millisecond_samples_up() {
        assert_eq!(duration_ms_ceil(Duration::from_nanos(1)), 1);
        assert_eq!(duration_ms_ceil(Duration::from_millis(12)), 12);
    }
}
