//! Read-only capacity and recovery test for the public WebSocket block stream.

use std::env;
use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde_json::json;
use sybil_client::{Error as SybilError, PublicBlockStreamEvent, SybilClient};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;

type AnyError = Box<dyn Error + Send + Sync>;

#[derive(Clone, Debug)]
struct Config {
    host: String,
    subscribers: usize,
    slow_subscribers: usize,
    run_duration: Duration,
    slow_read_stall: Duration,
    sample_interval: Duration,
    baseline_samples: usize,
    baseline_interval: Duration,
    minimum_blocks: u64,
    require_lag: bool,
    maximum_rss_growth_bytes: u64,
    maximum_hwm_growth_bytes: u64,
    maximum_actor_queue_depth: u64,
    maximum_solve_p99_micros: u64,
    maximum_solve_p99_increase_micros: u64,
    maximum_health_p95_ms: u64,
    maximum_health_p95_increase_ms: u64,
    report_file: PathBuf,
}

impl Config {
    fn from_env() -> Result<Self, AnyError> {
        let host = required_env("SYBIL_WS_LOADTEST_HOST")?
            .trim()
            .trim_end_matches('/')
            .to_owned();
        if !(host.starts_with("http://") || host.starts_with("https://")) {
            return Err(invalid_input(
                "SYBIL_WS_LOADTEST_HOST must start with http:// or https://",
            ));
        }
        let config = Self {
            host,
            subscribers: parse_env("SYBIL_WS_LOADTEST_SUBSCRIBERS", 100_usize)?,
            slow_subscribers: parse_env("SYBIL_WS_LOADTEST_SLOW_SUBSCRIBERS", 10_usize)?,
            run_duration: Duration::from_secs(parse_env("SYBIL_WS_LOADTEST_RUN_SECONDS", 60_u64)?),
            slow_read_stall: Duration::from_millis(parse_env(
                "SYBIL_WS_LOADTEST_SLOW_READ_STALL_MS",
                45_000_u64,
            )?),
            sample_interval: Duration::from_millis(parse_env(
                "SYBIL_WS_LOADTEST_SAMPLE_INTERVAL_MS",
                250_u64,
            )?),
            baseline_samples: parse_env("SYBIL_WS_LOADTEST_BASELINE_SAMPLES", 20_usize)?,
            baseline_interval: Duration::from_millis(parse_env(
                "SYBIL_WS_LOADTEST_BASELINE_INTERVAL_MS",
                100_u64,
            )?),
            minimum_blocks: parse_env("SYBIL_WS_LOADTEST_MIN_BLOCKS", 70_u64)?,
            require_lag: parse_bool_env("SYBIL_WS_LOADTEST_REQUIRE_LAG", true)?,
            maximum_rss_growth_bytes: mib(parse_env(
                "SYBIL_WS_LOADTEST_MAX_RSS_GROWTH_MIB",
                128_u64,
            )?),
            maximum_hwm_growth_bytes: mib(parse_env(
                "SYBIL_WS_LOADTEST_MAX_HWM_GROWTH_MIB",
                128_u64,
            )?),
            maximum_actor_queue_depth: parse_env(
                "SYBIL_WS_LOADTEST_MAX_ACTOR_QUEUE_DEPTH",
                128_u64,
            )?,
            maximum_solve_p99_micros: milliseconds_to_micros(parse_env(
                "SYBIL_WS_LOADTEST_MAX_SOLVE_P99_MS",
                100_u64,
            )?),
            maximum_solve_p99_increase_micros: milliseconds_to_micros(parse_env(
                "SYBIL_WS_LOADTEST_MAX_SOLVE_P99_INCREASE_MS",
                50_u64,
            )?),
            maximum_health_p95_ms: parse_env("SYBIL_WS_LOADTEST_MAX_HEALTH_P95_MS", 250_u64)?,
            maximum_health_p95_increase_ms: parse_env(
                "SYBIL_WS_LOADTEST_MAX_HEALTH_P95_INCREASE_MS",
                100_u64,
            )?,
            report_file: PathBuf::from(
                env::var("SYBIL_WS_LOADTEST_REPORT_FILE")
                    .unwrap_or_else(|_| "target/ws-load-report.json".to_string()),
            ),
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), AnyError> {
        if self.subscribers < 100 {
            return Err(invalid_input(
                "SYBIL_WS_LOADTEST_SUBSCRIBERS must be at least 100 for a capacity verdict",
            ));
        }
        if self.slow_subscribers == 0 || self.slow_subscribers >= self.subscribers {
            return Err(invalid_input(
                "SYBIL_WS_LOADTEST_SLOW_SUBSCRIBERS must be between 1 and subscribers - 1",
            ));
        }
        if self.run_duration.is_zero()
            || self.slow_read_stall.is_zero()
            || self.sample_interval.is_zero()
            || self.baseline_samples == 0
            || self.minimum_blocks == 0
        {
            return Err(invalid_input(
                "run duration, slow-reader stall, sample interval, baseline samples, and minimum blocks must be positive",
            ));
        }
        if self.slow_read_stall >= self.run_duration {
            return Err(invalid_input(
                "SYBIL_WS_LOADTEST_SLOW_READ_STALL_MS must leave time for recovery before the run ends",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct MetricSnapshot {
    rss_bytes: Option<u64>,
    high_water_bytes: Option<u64>,
    actor_queue_depth: Option<u64>,
    solve_p99_micros: Option<u64>,
}

#[derive(Debug)]
struct Baseline {
    health_p95_ms: u64,
    height: u64,
    metrics: MetricSnapshot,
}

#[derive(Debug, Default)]
struct LoadedObservations {
    health_timings_ms: Vec<u64>,
    last_height: u64,
    maximum_rss_bytes: u64,
    maximum_high_water_bytes: u64,
    maximum_actor_queue_depth: u64,
    maximum_solve_p99_micros: u64,
    samples: usize,
}

#[derive(Debug, Default)]
struct SubscriberStats {
    id: usize,
    slow: bool,
    blocks: u64,
    lag_events: u64,
    reconnects: u64,
    recovered_reconnects: u64,
    replay_completes: u64,
    last_height: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<(), AnyError> {
    let config = Config::from_env()?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let baseline = measure_baseline(&http, &config).await?;
    if baseline.height == 0 {
        return Err(invalid_input(
            "the target must have a positive committed height before the WebSocket load starts",
        ));
    }

    eprintln!(
        "websocket load: target={} subscribers={} slow={} duration={}s baseline_height={} baseline_health_p95={}ms",
        config.host,
        config.subscribers,
        config.slow_subscribers,
        config.run_duration.as_secs(),
        baseline.height,
        baseline.health_p95_ms,
    );

    let (ready_tx, mut ready_rx) = mpsc::channel(config.subscribers);
    let (start_tx, start_rx) = watch::channel(false);
    let mut tasks = JoinSet::new();
    for id in 0..config.subscribers {
        let host = config.host.clone();
        let ready_tx = ready_tx.clone();
        let start_rx = start_rx.clone();
        let slow = id < config.slow_subscribers;
        let slow_read_stall = config.slow_read_stall;
        let run_duration = config.run_duration;
        let from_block = baseline.height.saturating_add(1);
        tasks.spawn(async move {
            run_subscriber(
                id,
                host,
                slow,
                slow_read_stall,
                run_duration,
                from_block,
                ready_tx,
                start_rx,
            )
            .await
        });
    }
    drop(ready_tx);

    for _ in 0..config.subscribers {
        let ready = tokio::time::timeout(Duration::from_secs(30), ready_rx.recv())
            .await
            .map_err(|_| invalid_input("timed out opening WebSocket subscribers"))?
            .ok_or_else(|| invalid_input("subscriber readiness channel closed early"))?;
        if let Err(error) = ready {
            eprintln!("websocket subscriber failed during startup: {error}");
        }
    }

    let started = Instant::now();
    let _ = start_tx.send(true);
    let loaded = monitor_loaded(&http, &config, started).await?;

    let mut subscriber_stats = Vec::with_capacity(config.subscribers);
    let mut subscriber_failures = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(stats)) => subscriber_stats.push(stats),
            Ok(Err(error)) => subscriber_failures.push(error),
            Err(error) => subscriber_failures.push(format!("subscriber task failed: {error}")),
        }
    }
    subscriber_stats.sort_by_key(|stats| stats.id);

    let verdict = evaluate(
        &config,
        &baseline,
        &loaded,
        &subscriber_stats,
        &subscriber_failures,
    );
    write_report(
        &config,
        &baseline,
        &loaded,
        &subscriber_stats,
        &subscriber_failures,
        &verdict,
    )?;
    verdict.map_err(invalid_input)
}

#[allow(clippy::too_many_arguments)]
async fn run_subscriber(
    id: usize,
    host: String,
    slow: bool,
    slow_read_stall: Duration,
    run_duration: Duration,
    first_from_block: u64,
    ready_tx: mpsc::Sender<Result<usize, String>>,
    mut start_rx: watch::Receiver<bool>,
) -> Result<SubscriberStats, String> {
    let client = SybilClient::with_defaults(host, None);
    let mut from_block = first_from_block;
    let mut stream = match client
        .stream_block_events_from_block(Some(from_block))
        .await
    {
        Ok(stream) => Box::pin(stream),
        Err(error) => {
            let message = format!("subscriber {id} failed to connect: {error}");
            let _ = ready_tx.send(Err(message.clone())).await;
            return Err(message);
        }
    };
    ready_tx
        .send(Ok(id))
        .await
        .map_err(|_| "subscriber readiness receiver closed".to_string())?;
    start_rx
        .wait_for(|started| *started)
        .await
        .map_err(|_| "subscriber start channel closed".to_string())?;

    let deadline = Instant::now() + run_duration;
    let mut stats = SubscriberStats {
        id,
        slow,
        ..SubscriberStats::default()
    };
    let mut connection_number = 0_u64;
    let mut expected_height = from_block;
    let mut connection_replay_complete = false;

    if slow {
        tokio::time::sleep(slow_read_stall).await;
    }

    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Ok(stats);
        };
        let event = match tokio::time::timeout(remaining, stream.next()).await {
            Err(_) => return Ok(stats),
            Ok(None) => {
                return Err(format!(
                    "subscriber {id} stream ended before the load deadline"
                ));
            }
            Ok(Some(event)) => event,
        };

        match event {
            Ok(PublicBlockStreamEvent::Block(block)) => {
                if block.height != expected_height {
                    return Err(format!(
                        "subscriber {id} expected height {expected_height}, received {}",
                        block.height
                    ));
                }
                stats.blocks = stats.blocks.saturating_add(1);
                stats.last_height = Some(block.height);
                expected_height = block.height.saturating_add(1);
            }
            Ok(PublicBlockStreamEvent::ReplayComplete { up_to_height }) => {
                if up_to_height.saturating_add(1) != expected_height {
                    return Err(format!(
                        "subscriber {id} replay completed at {up_to_height}, next expected height is {expected_height}"
                    ));
                }
                if connection_replay_complete {
                    return Err(format!(
                        "subscriber {id} received duplicate replay_complete on one connection"
                    ));
                }
                connection_replay_complete = true;
                stats.replay_completes = stats.replay_completes.saturating_add(1);
                if connection_number > 0 {
                    stats.recovered_reconnects = stats.recovered_reconnects.saturating_add(1);
                }
            }
            Err(SybilError::BlockStreamLagged {
                skipped,
                last_sent_height,
            }) => {
                if skipped == 0 {
                    return Err(format!("subscriber {id} received a zero-sized lag"));
                }
                if last_sent_height != stats.last_height {
                    return Err(format!(
                        "subscriber {id} observed last height {:?}, lag envelope reported {:?}",
                        stats.last_height, last_sent_height
                    ));
                }
                if !connection_replay_complete {
                    return Err(format!(
                        "subscriber {id} lagged before its replay/live handoff completed"
                    ));
                }
                let Some(last_height) = last_sent_height else {
                    return Err(format!(
                        "subscriber {id} lagged before receiving a resumable height"
                    ));
                };
                stats.lag_events = stats.lag_events.saturating_add(1);
                stats.reconnects = stats.reconnects.saturating_add(1);
                from_block = last_height.saturating_add(1);
                expected_height = from_block;
                connection_number = connection_number.saturating_add(1);
                connection_replay_complete = false;
                stream = Box::pin(
                    client
                        .stream_block_events_from_block(Some(from_block))
                        .await
                        .map_err(|error| {
                            format!("subscriber {id} reconnect from {from_block} failed: {error}")
                        })?,
                );
            }
            Err(SybilError::RetentionGap {
                requested_height,
                retention_min_height,
                head_height,
            }) => {
                return Err(format!(
                    "subscriber {id} hit retention gap: requested={requested_height} floor={retention_min_height} head={head_height}"
                ));
            }
            Err(error) => return Err(format!("subscriber {id} stream failed: {error}")),
        }
    }
}

async fn measure_baseline(http: &reqwest::Client, config: &Config) -> Result<Baseline, AnyError> {
    let mut timings = Vec::with_capacity(config.baseline_samples);
    let mut height = 0_u64;
    let mut metrics = MetricSnapshot::default();
    for sample in 0..config.baseline_samples {
        let (observed_height, elapsed_ms) = health_height(http, &config.host).await?;
        height = observed_height;
        timings.push(elapsed_ms);
        metrics = fetch_metrics(http, &config.host).await?;
        if sample + 1 < config.baseline_samples && !config.baseline_interval.is_zero() {
            tokio::time::sleep(config.baseline_interval).await;
        }
    }
    Ok(Baseline {
        health_p95_ms: percentile(&mut timings, 95),
        height,
        metrics,
    })
}

async fn monitor_loaded(
    http: &reqwest::Client,
    config: &Config,
    started: Instant,
) -> Result<LoadedObservations, AnyError> {
    let deadline = started + config.run_duration;
    let mut observations = LoadedObservations::default();
    while Instant::now() < deadline {
        let (height, elapsed_ms) = health_height(http, &config.host).await?;
        let metrics = fetch_metrics(http, &config.host).await?;
        observations.health_timings_ms.push(elapsed_ms);
        observations.last_height = height;
        observations.maximum_rss_bytes = observations
            .maximum_rss_bytes
            .max(metrics.rss_bytes.unwrap_or(0));
        observations.maximum_high_water_bytes = observations
            .maximum_high_water_bytes
            .max(metrics.high_water_bytes.unwrap_or(0));
        observations.maximum_actor_queue_depth = observations
            .maximum_actor_queue_depth
            .max(metrics.actor_queue_depth.unwrap_or(0));
        observations.maximum_solve_p99_micros = observations
            .maximum_solve_p99_micros
            .max(metrics.solve_p99_micros.unwrap_or(0));
        observations.samples = observations.samples.saturating_add(1);
        if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            tokio::time::sleep(config.sample_interval.min(remaining)).await;
        }
    }
    Ok(observations)
}

async fn health_height(http: &reqwest::Client, host: &str) -> Result<(u64, u64), AnyError> {
    let started = Instant::now();
    let response = http
        .get(format!("{host}/v1/health"))
        .send()
        .await?
        .error_for_status()?;
    let value: serde_json::Value = response.json().await?;
    let height = value
        .get("height")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            invalid_input("health response did not contain a positive numeric height")
        })?;
    Ok((height, duration_ms_ceil(started.elapsed())))
}

async fn fetch_metrics(http: &reqwest::Client, host: &str) -> Result<MetricSnapshot, AnyError> {
    let text = http
        .get(format!("{host}/metrics"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let metrics = parse_metrics(&text);
    let mut missing = Vec::new();
    if metrics.rss_bytes.is_none() {
        missing.push("sybil_process_resident_memory_bytes");
    }
    if metrics.high_water_bytes.is_none() {
        missing.push("sybil_process_resident_memory_high_water_bytes");
    }
    if metrics.actor_queue_depth.is_none() {
        missing.push("sybil_actor_queue_depth{actor=\"sequencer\"}");
    }
    if metrics.solve_p99_micros.is_none() {
        missing.push("sybil_solve_time_seconds{quantile=\"0.99\"}");
    }
    if missing.is_empty() {
        Ok(metrics)
    } else {
        Err(invalid_input(format!(
            "target /metrics is missing required series: {}",
            missing.join(", ")
        )))
    }
}

fn parse_metrics(text: &str) -> MetricSnapshot {
    MetricSnapshot {
        rss_bytes: metric_value_scaled(text, "sybil_process_resident_memory_bytes", &[], 1),
        high_water_bytes: metric_value_scaled(
            text,
            "sybil_process_resident_memory_high_water_bytes",
            &[],
            1,
        ),
        actor_queue_depth: metric_value_scaled(
            text,
            "sybil_actor_queue_depth",
            &[("actor", "sequencer")],
            1,
        ),
        solve_p99_micros: metric_value_scaled(
            text,
            "sybil_solve_time_seconds",
            &[("quantile", "0.99")],
            1_000_000,
        ),
    }
}

fn metric_value_scaled(text: &str, name: &str, labels: &[(&str, &str)], scale: u64) -> Option<u64> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| {
            let (series, raw_value) = line.split_once(char::is_whitespace)?;
            let series_name = series.split_once('{').map_or(series, |(name, _)| name);
            if series_name != name
                || labels
                    .iter()
                    .any(|(key, value)| !series.contains(&format!(r#"{key}="{value}""#)))
            {
                return None;
            }
            parse_decimal_scaled(raw_value.split_whitespace().next()?, scale)
        })
}

/// Parses a non-negative Prometheus decimal into a scaled integer, rounding up
/// any precision that the target integer cannot retain.
fn parse_decimal_scaled(raw: &str, scale: u64) -> Option<u64> {
    if scale == 0 {
        return None;
    }
    let precision = usize::try_from(scale.ilog10()).ok()?;
    if 10_u64.checked_pow(u32::try_from(precision).ok()?)? != scale {
        return None;
    }

    let (whole, fraction) = raw.split_once('.').unwrap_or((raw, ""));
    if whole.is_empty() && fraction.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }

    let whole = if whole.is_empty() {
        0_u128
    } else {
        whole.parse::<u128>().ok()?
    };
    let scaled_whole = whole.checked_mul(u128::from(scale))?;
    let retained_len = fraction.len().min(precision);
    let retained = if retained_len == 0 {
        0_u128
    } else {
        fraction[..retained_len].parse::<u128>().ok()?
    };
    let padding = precision.saturating_sub(retained_len);
    let scaled_fraction =
        retained.checked_mul(10_u128.checked_pow(u32::try_from(padding).ok()?)?)?;
    let round_up = u128::from(fraction[retained_len..].bytes().any(|byte| byte != b'0'));

    u64::try_from(
        scaled_whole
            .checked_add(scaled_fraction)?
            .checked_add(round_up)?,
    )
    .ok()
}

fn evaluate(
    config: &Config,
    baseline: &Baseline,
    loaded: &LoadedObservations,
    subscribers: &[SubscriberStats],
    subscriber_failures: &[String],
) -> Result<(), String> {
    let loaded_health_p95_ms = percentile(&mut loaded.health_timings_ms.clone(), 95);
    let block_delta = loaded.last_height.saturating_sub(baseline.height);
    let total_blocks: u64 = subscribers.iter().map(|stats| stats.blocks).sum();
    let total_lag_events: u64 = subscribers.iter().map(|stats| stats.lag_events).sum();
    let recovered_reconnects: u64 = subscribers
        .iter()
        .map(|stats| stats.recovered_reconnects)
        .sum();
    let slow_lag_events: u64 = subscribers
        .iter()
        .filter(|stats| stats.slow)
        .map(|stats| stats.lag_events)
        .sum();
    let rss_growth = growth_from(baseline.metrics.rss_bytes, loaded.maximum_rss_bytes);
    let hwm_growth = growth_from(
        baseline.metrics.high_water_bytes,
        loaded.maximum_high_water_bytes,
    );
    let solve_increase = loaded
        .maximum_solve_p99_micros
        .saturating_sub(baseline.metrics.solve_p99_micros.unwrap_or(0));

    eprintln!(
        "websocket load verdict: height_delta={} subscribers={} blocks_seen={} lag_events={} slow_lag_events={} recovered_reconnects={} rss_growth_mib={} hwm_growth_mib={} max_queue={} baseline_solve_p99_ms={} loaded_solve_p99_ms={} baseline_health_p95_ms={} loaded_health_p95_ms={}",
        block_delta,
        subscribers.len(),
        total_blocks,
        total_lag_events,
        slow_lag_events,
        recovered_reconnects,
        format_mib(rss_growth),
        format_mib(hwm_growth),
        loaded.maximum_actor_queue_depth,
        format_micros_as_ms(baseline.metrics.solve_p99_micros.unwrap_or(0)),
        format_micros_as_ms(loaded.maximum_solve_p99_micros),
        baseline.health_p95_ms,
        loaded_health_p95_ms,
    );

    let mut violations = subscriber_failures
        .iter()
        .map(|failure| format!("subscriber failure: {failure}"))
        .collect::<Vec<_>>();
    if subscribers.len() != config.subscribers {
        violations.push(format!(
            "only {} of {} subscribers completed",
            subscribers.len(),
            config.subscribers
        ));
    }
    if block_delta < config.minimum_blocks {
        violations.push(format!(
            "only {block_delta} blocks committed under load (minimum {})",
            config.minimum_blocks
        ));
    }
    if total_blocks == 0 {
        violations.push("subscribers observed no blocks".to_string());
    }
    if config.require_lag && slow_lag_events == 0 {
        violations.push("no intentionally slow subscriber received a lagged envelope".to_string());
    }
    if recovered_reconnects != total_lag_events {
        violations.push(format!(
            "only {recovered_reconnects} of {total_lag_events} lagged connections completed a reconnect replay"
        ));
    }
    if rss_growth > config.maximum_rss_growth_bytes {
        violations.push(format!(
            "RSS grew {} MiB (maximum {} MiB)",
            format_mib(rss_growth),
            format_mib(config.maximum_rss_growth_bytes),
        ));
    }
    if hwm_growth > config.maximum_hwm_growth_bytes {
        violations.push(format!(
            "high-water RSS grew {} MiB (maximum {} MiB)",
            format_mib(hwm_growth),
            format_mib(config.maximum_hwm_growth_bytes),
        ));
    }
    if loaded.maximum_actor_queue_depth > config.maximum_actor_queue_depth {
        violations.push(format!(
            "actor queue depth {} exceeded {}",
            loaded.maximum_actor_queue_depth, config.maximum_actor_queue_depth
        ));
    }
    if loaded.maximum_solve_p99_micros > config.maximum_solve_p99_micros {
        violations.push(format!(
            "solve p99 {}ms exceeded {}ms",
            format_micros_as_ms(loaded.maximum_solve_p99_micros),
            format_micros_as_ms(config.maximum_solve_p99_micros),
        ));
    }
    if solve_increase > config.maximum_solve_p99_increase_micros {
        violations.push(format!(
            "solve p99 increased {}ms (maximum {}ms)",
            format_micros_as_ms(solve_increase),
            format_micros_as_ms(config.maximum_solve_p99_increase_micros),
        ));
    }
    if loaded_health_p95_ms > config.maximum_health_p95_ms {
        violations.push(format!(
            "loaded health p95 {loaded_health_p95_ms}ms exceeded {}ms",
            config.maximum_health_p95_ms
        ));
    }
    if loaded_health_p95_ms
        > baseline
            .health_p95_ms
            .saturating_add(config.maximum_health_p95_increase_ms)
    {
        violations.push(format!(
            "loaded health p95 {loaded_health_p95_ms}ms exceeded baseline {}ms + {}ms",
            baseline.health_p95_ms, config.maximum_health_p95_increase_ms
        ));
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations.join("; "))
    }
}

fn write_report(
    config: &Config,
    baseline: &Baseline,
    loaded: &LoadedObservations,
    subscribers: &[SubscriberStats],
    subscriber_failures: &[String],
    verdict: &Result<(), String>,
) -> Result<(), AnyError> {
    if let Some(parent) = config.report_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let report = json!({
        "target": config.host,
        "config": {
            "subscribers": config.subscribers,
            "slow_subscribers": config.slow_subscribers,
            "run_seconds": config.run_duration.as_secs(),
            "slow_read_stall_ms": duration_ms(config.slow_read_stall),
            "sample_interval_ms": duration_ms(config.sample_interval),
            "baseline_samples": config.baseline_samples,
            "baseline_interval_ms": duration_ms(config.baseline_interval),
            "minimum_blocks": config.minimum_blocks,
            "require_lag": config.require_lag,
            "maximum_rss_growth_bytes": config.maximum_rss_growth_bytes,
            "maximum_hwm_growth_bytes": config.maximum_hwm_growth_bytes,
            "maximum_actor_queue_depth": config.maximum_actor_queue_depth,
            "maximum_solve_p99_micros": config.maximum_solve_p99_micros,
            "maximum_solve_p99_increase_micros": config.maximum_solve_p99_increase_micros,
            "maximum_health_p95_ms": config.maximum_health_p95_ms,
            "maximum_health_p95_increase_ms": config.maximum_health_p95_increase_ms,
        },
        "baseline": {
            "height": baseline.height,
            "health_p95_ms": baseline.health_p95_ms,
            "rss_bytes": baseline.metrics.rss_bytes,
            "high_water_bytes": baseline.metrics.high_water_bytes,
            "actor_queue_depth": baseline.metrics.actor_queue_depth,
            "solve_p99_micros": baseline.metrics.solve_p99_micros,
        },
        "loaded": {
            "last_height": loaded.last_height,
            "samples": loaded.samples,
            "health_p95_ms": percentile(&mut loaded.health_timings_ms.clone(), 95),
            "maximum_rss_bytes": loaded.maximum_rss_bytes,
            "maximum_high_water_bytes": loaded.maximum_high_water_bytes,
            "maximum_actor_queue_depth": loaded.maximum_actor_queue_depth,
            "maximum_solve_p99_micros": loaded.maximum_solve_p99_micros,
        },
        "subscriber_results": subscribers.iter().map(|stats| json!({
            "id": stats.id,
            "slow": stats.slow,
            "blocks": stats.blocks,
            "lag_events": stats.lag_events,
            "reconnects": stats.reconnects,
            "recovered_reconnects": stats.recovered_reconnects,
            "replay_completes": stats.replay_completes,
            "last_height": stats.last_height,
        })).collect::<Vec<_>>(),
        "subscriber_failures": subscriber_failures,
        "passed": verdict.is_ok(),
        "failure": verdict.as_ref().err(),
    });
    std::fs::write(&config.report_file, serde_json::to_vec_pretty(&report)?)?;
    eprintln!("websocket load report: {}", config.report_file.display());
    Ok(())
}

fn growth_from(baseline: Option<u64>, maximum: u64) -> u64 {
    maximum.saturating_sub(baseline.unwrap_or(maximum))
}

fn percentile(samples: &mut [u64], percentile: usize) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    samples.sort_unstable();
    let index = samples
        .len()
        .saturating_mul(percentile)
        .div_ceil(100)
        .saturating_sub(1)
        .min(samples.len().saturating_sub(1));
    samples[index]
}

fn duration_ms_ceil(duration: Duration) -> u64 {
    duration_ms(duration).max(1)
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn milliseconds_to_micros(milliseconds: u64) -> u64 {
    milliseconds.saturating_mul(1_000)
}

fn format_micros_as_ms(micros: u64) -> String {
    format!("{}.{:03}", micros / 1_000, micros % 1_000)
}

fn format_mib(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    let hundredths = bytes % MIB * 100 / MIB;
    format!("{}.{hundredths:02}", bytes / MIB)
}

fn mib(value: u64) -> u64 {
    value.saturating_mul(1024).saturating_mul(1024)
}

fn required_env(name: &str) -> Result<String, AnyError> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid_input(format!("{name} is required")))
}

fn parse_env<T>(name: &str, default: T) -> Result<T, AnyError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => value
            .trim()
            .parse()
            .map_err(|error| invalid_input(format!("{name} must be a valid value: {error}"))),
        _ => Ok(default),
    }
}

fn parse_bool_env(name: &str, default: bool) -> Result<bool, AnyError> {
    match env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" => Ok(true),
            "0" | "false" | "no" => Ok(false),
            _ => Err(invalid_input(format!(
                "{name} must be one of 1/0, true/false, or yes/no"
            ))),
        },
        Err(_) => Ok(default),
    }
}

fn invalid_input(message: impl Into<String>) -> AnyError {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prometheus_summary_and_labeled_gauge() {
        let text = r#"
# TYPE sybil_process_resident_memory_bytes gauge
sybil_process_resident_memory_bytes 1048576
sybil_process_resident_memory_high_water_bytes 2097152
sybil_actor_queue_depth{actor="sequencer"} 7
sybil_actor_queue_depth{actor="other"} 99
sybil_solve_time_seconds{quantile="0.99"} 0.0125
"#;
        let metrics = parse_metrics(text);
        assert_eq!(metrics.rss_bytes, Some(1_048_576));
        assert_eq!(metrics.high_water_bytes, Some(2_097_152));
        assert_eq!(metrics.actor_queue_depth, Some(7));
        assert_eq!(metrics.solve_p99_micros, Some(12_500));
    }

    #[test]
    fn decimal_metrics_round_up_without_floating_point() {
        assert_eq!(parse_decimal_scaled("0.0000001", 1_000_000), Some(1));
        assert_eq!(parse_decimal_scaled("7.5", 1), Some(8));
        assert_eq!(parse_decimal_scaled("12.500", 1_000), Some(12_500));
        assert_eq!(parse_decimal_scaled("NaN", 1_000), None);
        assert_eq!(parse_decimal_scaled("-1", 1_000), None);
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let mut samples = vec![100, 1, 3, 2, 4];
        assert_eq!(percentile(&mut samples, 95), 100);
        assert_eq!(percentile(&mut samples, 50), 3);
        assert_eq!(percentile(&mut [], 95), 0);
    }

    #[test]
    fn config_rejects_profiles_that_do_not_exercise_capacity_and_slow_readers() {
        let mut config = Config {
            host: "http://127.0.0.1:3000".into(),
            subscribers: 99,
            slow_subscribers: 1,
            run_duration: Duration::from_secs(1),
            slow_read_stall: Duration::from_millis(1),
            sample_interval: Duration::from_millis(1),
            baseline_samples: 1,
            baseline_interval: Duration::ZERO,
            minimum_blocks: 1,
            require_lag: true,
            maximum_rss_growth_bytes: 1,
            maximum_hwm_growth_bytes: 1,
            maximum_actor_queue_depth: 1,
            maximum_solve_p99_micros: 1_000,
            maximum_solve_p99_increase_micros: 1_000,
            maximum_health_p95_ms: 1,
            maximum_health_p95_increase_ms: 1,
            report_file: PathBuf::from("unused"),
        };
        assert!(config.validate().is_err());
        config.subscribers = 100;
        config.slow_subscribers = 0;
        assert!(config.validate().is_err());
        config.slow_subscribers = 10;
        config.slow_read_stall = Duration::ZERO;
        assert!(config.validate().is_err());
        config.slow_read_stall = Duration::from_millis(1);
        assert!(config.validate().is_ok());
    }
}
