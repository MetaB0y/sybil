use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use axum::Json;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use matching_engine::mm_constraint::{MmConstraint, MmId};
use matching_engine::{MarketId, NANOS_PER_DOLLAR, Nanos, Qty};
use matching_sequencer::{AccountId, ActorEpochSubmission, OrderSubmission};
use sha2::{Digest as _, Sha256};

use crate::convert::order_spec_to_order;
use crate::convert::public_block_to_response;
use crate::state::{ActorCredential, ActorEpochObservation, ActorMarketObservation, AppState};
use crate::types::error::AppError;
use crate::types::request::{
    ActivateLiquidityUniverseRequest, ActorEpochRequest, ActorRole, CompleteSetActionRequest,
    CompleteSetInventoryRequest, OrderSpec,
};
use crate::types::response::{
    ActorEpochResponse, ActorIdentityResponse, ActorMarketReceipt, LiquidityHealthResponse,
    LiquidityUniverseResponse, MarketLiquidityHealthResponse, MmQuoteMarketResponse,
    MmQuoteSnapshotResponse,
};

#[derive(serde::Deserialize)]
pub struct MmQuoteQuery {
    pub target_height: u64,
}

fn authenticate_actor<'a>(
    state: &'a AppState,
    headers: &HeaderMap,
) -> Result<&'a ActorCredential, AppError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::unauthorized("Missing actor bearer token"))?;
    state
        .actor_credentials
        .iter()
        .find(|credential| {
            crate::app::constant_time_eq(token.as_bytes(), credential.token.as_bytes())
        })
        .ok_or_else(|| AppError::forbidden("Invalid actor bearer token"))
}

fn spec_market_id(spec: &OrderSpec) -> u32 {
    match spec {
        OrderSpec::BuyYes { market_id, .. }
        | OrderSpec::BuyNo { market_id, .. }
        | OrderSpec::SellYes { market_id, .. }
        | OrderSpec::SellNo { market_id, .. } => *market_id,
    }
}

fn sequencer_role(role: ActorRole) -> matching_sequencer::ActorRole {
    match role {
        ActorRole::MarketMaker => matching_sequencer::ActorRole::MarketMaker,
        ActorRole::Noise => matching_sequencer::ActorRole::Noise,
    }
}

fn mm_quote_market(intent: &crate::types::request::ActorMarketIntent) -> MmQuoteMarketResponse {
    let mut yes_bid = None::<(u64, u64)>;
    let mut yes_ask = None::<(u64, u64)>;
    for order in &intent.orders {
        let (bid, ask, quantity) = match order {
            OrderSpec::BuyYes {
                limit_price_nanos,
                quantity,
                ..
            } => (Some(*limit_price_nanos), None, *quantity),
            OrderSpec::SellNo {
                limit_price_nanos,
                quantity,
                ..
            } => (
                Some(NANOS_PER_DOLLAR.saturating_sub(*limit_price_nanos)),
                None,
                *quantity,
            ),
            OrderSpec::SellYes {
                limit_price_nanos,
                quantity,
                ..
            } => (None, Some(*limit_price_nanos), *quantity),
            OrderSpec::BuyNo {
                limit_price_nanos,
                quantity,
                ..
            } => (
                None,
                Some(NANOS_PER_DOLLAR.saturating_sub(*limit_price_nanos)),
                *quantity,
            ),
        };
        if let Some(price) = bid
            && yes_bid.is_none_or(|(current, _)| price > current)
        {
            yes_bid = Some((price, quantity));
        }
        if let Some(price) = ask
            && yes_ask.is_none_or(|(current, _)| price < current)
        {
            yes_ask = Some((price, quantity));
        }
    }
    let quote_state = match (yes_bid, yes_ask) {
        (Some(_), Some(_)) => "two_sided",
        (Some(_), None) | (None, Some(_)) => "one_sided",
        (None, None) => "skipped",
    };
    MmQuoteMarketResponse {
        market_id: intent.market_id,
        yes_bid_nanos: yes_bid.map(|(price, _)| price),
        yes_ask_nanos: yes_ask.map(|(price, _)| price),
        bid_quantity: yes_bid.map(|(_, quantity)| quantity),
        ask_quantity: yes_ask.map(|(_, quantity)| quantity),
        quote_state: quote_state.to_string(),
        skip_reason: intent.skip_reason.clone(),
    }
}

fn order_crosses_mm_quote(spec: &OrderSpec, quote: &MmQuoteMarketResponse) -> bool {
    let (Some(yes_bid), Some(yes_ask)) = (quote.yes_bid_nanos, quote.yes_ask_nanos) else {
        return false;
    };
    match spec {
        OrderSpec::BuyYes {
            limit_price_nanos, ..
        } => *limit_price_nanos >= yes_ask,
        OrderSpec::SellYes {
            limit_price_nanos, ..
        } => *limit_price_nanos <= yes_bid,
        OrderSpec::BuyNo {
            limit_price_nanos, ..
        } => *limit_price_nanos >= NANOS_PER_DOLLAR.saturating_sub(yes_bid),
        OrderSpec::SellNo {
            limit_price_nanos, ..
        } => *limit_price_nanos <= NANOS_PER_DOLLAR.saturating_sub(yes_ask),
    }
}

#[utoipa::path(
    get,
    path = "/v1/actor/mm-quotes",
    params(("target_height" = u64, Query)),
    responses((status = 200, body = MmQuoteSnapshotResponse))
)]
pub async fn get_mm_quotes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MmQuoteQuery>,
) -> Result<Json<MmQuoteSnapshotResponse>, AppError> {
    let _credential = authenticate_actor(&state, &headers)?;
    state
        .mm_quote_snapshots
        .read()
        .await
        .get(&query.target_height)
        .cloned()
        .map(Json)
        .ok_or_else(|| AppError::not_found("MM quote snapshot is not available"))
}

#[utoipa::path(
    get,
    path = "/v1/actor/universe",
    responses((status = 200, body = LiquidityUniverseResponse))
)]
pub async fn get_actor_universe(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<LiquidityUniverseResponse>, AppError> {
    let credential = authenticate_actor(&state, &headers)?;
    let universe = state.sequencer.get_liquidity_universe().await?;
    let committed = state.sequencer.get_committed_liquidity_universe().await?;
    Ok(Json(LiquidityUniverseResponse {
        generation: universe.generation,
        policy_digest_hex: hex::encode(universe.policy_digest),
        activated_at_height: universe.activated_at_height,
        market_ids: universe
            .market_ids
            .into_iter()
            .map(|market| market.0)
            .collect(),
        committed_market_ids: committed
            .market_ids
            .into_iter()
            .map(|market| market.0)
            .collect(),
        actor_ready: universe.generation > 0,
        principal_id: Some(credential.principal_id.clone()),
        actor_role: Some(credential.role),
        account_id: Some(credential.account_id),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/liquidity/universe",
    responses((status = 200, body = LiquidityUniverseResponse))
)]
pub async fn get_liquidity_universe(
    State(state): State<AppState>,
) -> Result<Json<LiquidityUniverseResponse>, AppError> {
    let universe = state.sequencer.get_liquidity_universe().await?;
    let committed = state.sequencer.get_committed_liquidity_universe().await?;
    Ok(Json(LiquidityUniverseResponse {
        generation: universe.generation,
        policy_digest_hex: hex::encode(universe.policy_digest),
        activated_at_height: universe.activated_at_height,
        market_ids: universe
            .market_ids
            .into_iter()
            .map(|market| market.0)
            .collect(),
        committed_market_ids: committed
            .market_ids
            .into_iter()
            .map(|market| market.0)
            .collect(),
        actor_ready: universe.generation > 0,
        principal_id: None,
        actor_role: None,
        account_id: None,
    }))
}

#[utoipa::path(
    get,
    path = "/v1/liquidity/health",
    responses((status = 200, body = LiquidityHealthResponse))
)]
pub async fn get_liquidity_health(
    State(state): State<AppState>,
) -> Result<Json<LiquidityHealthResponse>, AppError> {
    let universe = state.sequencer.get_liquidity_universe().await?;
    let sealed = state
        .sequencer
        .get_latest_block()
        .await?
        .ok_or_else(|| AppError::not_found("No blocks produced yet"))?;
    let block = public_block_to_response(&sealed);
    let recent_blocks = state.sequencer.get_recent_blocks(100).await?;
    let observations = state
        .actor_epoch_observations
        .read()
        .await
        .get(&block.height)
        .cloned()
        .unwrap_or_default();
    let current = observations
        .values()
        .filter(|observation| observation.valid_until_ms >= block.timestamp_ms)
        .collect::<Vec<_>>();
    let observed_noise = current
        .iter()
        .filter(|observation| observation.role == ActorRole::Noise)
        .map(|observation| observation.principal_id.as_str())
        .collect::<HashSet<_>>()
        .len() as u32;

    let mut markets = Vec::with_capacity(universe.market_ids.len());
    for market_id in &universe.market_ids {
        let mut mm_orders = 0u32;
        let mut mm_skip_reason = None;
        let mut noise_actor_count = 0u32;
        let mut noise_orders = 0u32;
        let mut noise_crossing_orders = 0u32;
        for observation in &current {
            let Some(market) = observation.markets.get(&market_id.0) else {
                continue;
            };
            match observation.role {
                ActorRole::MarketMaker => {
                    mm_orders = mm_orders.saturating_add(market.order_count as u32);
                    if market.order_count == 0 {
                        mm_skip_reason = market.skip_reason.clone();
                    }
                }
                ActorRole::Noise => {
                    noise_orders = noise_orders.saturating_add(market.order_count as u32);
                    noise_actor_count += u32::from(market.order_count > 0);
                    noise_crossing_orders += u32::from(market.crosses_mm_quote);
                }
            }
        }
        let stats = block.by_market.get(&market_id.0.to_string());
        let placed = stats.map(|stats| stats.placed).unwrap_or(0);
        markets.push(MarketLiquidityHealthResponse {
            market_id: market_id.0,
            mm_orders,
            mm_skip_reason,
            noise_actor_count,
            noise_orders,
            noise_crossing_orders,
            other_non_mm_orders: placed
                .saturating_sub(noise_orders)
                .saturating_sub(mm_orders),
            clearing_price_present: block
                .clearing_prices_nanos
                .contains_key(&market_id.0.to_string()),
            fill_volume_nanos: stats.map(|stats| stats.volume_nanos).unwrap_or(0),
        });
    }
    let active_markets = markets.len() as u32;
    let mm_markets_quoted = markets.iter().filter(|market| market.mm_orders > 0).count() as u32;
    let mm_markets_two_sided = state
        .mm_quote_snapshots
        .read()
        .await
        .get(&block.height)
        .map(|snapshot| {
            snapshot
                .markets
                .iter()
                .filter(|market| market.quote_state == "two_sided")
                .count() as u32
        })
        .unwrap_or(0);
    let noise_markets_selected = markets
        .iter()
        .filter(|market| market.noise_orders > 0)
        .count() as u32;
    let noise_markets_crossing_mm = markets
        .iter()
        .filter(|market| market.noise_crossing_orders > 0)
        .count() as u32;
    let markets_with_noise_fills = block
        .by_market
        .values()
        .filter(|stats| stats.noise_matched_orders > 0)
        .count() as u32;
    let active_market_ids = universe
        .market_ids
        .iter()
        .map(|market_id| market_id.0)
        .collect::<HashSet<_>>();
    let rolling_blocks = recent_blocks
        .iter()
        .filter(|sealed| sealed.canonical.header.height >= universe.activated_at_height)
        .collect::<Vec<_>>();
    let rolling_window_blocks = rolling_blocks.len() as u32;
    let rolling_denominator = u64::from(rolling_window_blocks) * u64::from(active_markets);
    let (rolling_mm_markets, rolling_noise_markets, rolling_noise_crossing_markets) = {
        let all_observations = state.actor_epoch_observations.read().await;
        let mut mm_total = 0u64;
        let mut noise_total = 0u64;
        let mut noise_crossing_total = 0u64;
        for sealed in &rolling_blocks {
            let height = sealed.canonical.header.height;
            let Some(rows) = all_observations.get(&height) else {
                continue;
            };
            let mut mm = HashSet::new();
            let mut noise = HashSet::new();
            let mut noise_crossing = HashSet::new();
            for observation in rows.values() {
                for (market_id, market) in &observation.markets {
                    if market.order_count == 0 || !active_market_ids.contains(market_id) {
                        continue;
                    }
                    match observation.role {
                        ActorRole::MarketMaker => {
                            mm.insert(*market_id);
                        }
                        ActorRole::Noise => {
                            noise.insert(*market_id);
                            if market.crosses_mm_quote {
                                noise_crossing.insert(*market_id);
                            }
                        }
                    }
                }
            }
            mm_total = mm_total.saturating_add(mm.len() as u64);
            noise_total = noise_total.saturating_add(noise.len() as u64);
            noise_crossing_total = noise_crossing_total.saturating_add(noise_crossing.len() as u64);
        }
        (mm_total, noise_total, noise_crossing_total)
    };
    let rolling_mm_two_sided_markets = {
        let snapshots = state.mm_quote_snapshots.read().await;
        rolling_blocks
            .iter()
            .map(|sealed| {
                snapshots
                    .get(&sealed.canonical.header.height)
                    .map(|snapshot| {
                        snapshot
                            .markets
                            .iter()
                            .filter(|market| {
                                market.quote_state == "two_sided"
                                    && active_market_ids.contains(&market.market_id)
                            })
                            .count() as u64
                    })
                    .unwrap_or(0)
            })
            .sum::<u64>()
    };
    let rolling_noise_fill_markets = rolling_blocks
        .iter()
        .map(|sealed| {
            sealed
                .analytics
                .noise_matched_orders_by_market
                .iter()
                .filter(|(market_id, matched)| {
                    **matched > 0 && active_market_ids.contains(&market_id.0)
                })
                .count() as u64
        })
        .sum::<u64>();
    let rolling_bps = |numerator: u64| -> u32 {
        numerator
            .saturating_mul(10_000)
            .checked_div(rolling_denominator)
            .unwrap_or(0)
            .min(u64::from(u32::MAX)) as u32
    };
    let actors = {
        let all_observations = state.actor_epoch_observations.read().await;
        state
            .actor_credentials
            .iter()
            .map(|credential| {
                let last_observed_height = all_observations
                    .iter()
                    .rev()
                    .find(|(_, rows)| rows.contains_key(&credential.principal_id))
                    .map(|(height, _)| *height);
                ActorIdentityResponse {
                    account_id: credential.account_id,
                    principal_id: credential.principal_id.clone(),
                    role: credential.role,
                    last_observed_height,
                    ready: current
                        .iter()
                        .any(|row| row.principal_id == credential.principal_id),
                }
            })
            .collect::<Vec<_>>()
    };
    let response = LiquidityHealthResponse {
        height: block.height,
        universe_generation: universe.generation,
        active_markets,
        mm_markets_quoted,
        mm_coverage_bps: mm_markets_quoted
            .saturating_mul(10_000)
            .checked_div(active_markets)
            .unwrap_or(0),
        mm_markets_two_sided,
        mm_two_sided_coverage_bps: mm_markets_two_sided
            .saturating_mul(10_000)
            .checked_div(active_markets)
            .unwrap_or(0),
        expected_noise_actors: state
            .actor_credentials
            .iter()
            .filter(|credential| credential.role == ActorRole::Noise)
            .count() as u32,
        observed_noise_actors: observed_noise,
        markets_with_two_noise_actors: markets
            .iter()
            .filter(|market| market.noise_actor_count >= 2)
            .count() as u32,
        markets_with_three_noise_actors: markets
            .iter()
            .filter(|market| market.noise_actor_count >= 3)
            .count() as u32,
        noise_markets_selected,
        noise_coverage_bps: noise_markets_selected
            .saturating_mul(10_000)
            .checked_div(active_markets)
            .unwrap_or(0),
        noise_markets_crossing_mm,
        noise_crossing_coverage_bps: noise_markets_crossing_mm
            .saturating_mul(10_000)
            .checked_div(active_markets)
            .unwrap_or(0),
        markets_with_noise_fills,
        rolling_window_blocks,
        rolling_mm_coverage_bps: rolling_bps(rolling_mm_markets),
        rolling_mm_two_sided_coverage_bps: rolling_bps(rolling_mm_two_sided_markets),
        rolling_noise_coverage_bps: rolling_bps(rolling_noise_markets),
        rolling_noise_crossing_coverage_bps: rolling_bps(rolling_noise_crossing_markets),
        rolling_noise_fill_coverage_bps: rolling_bps(rolling_noise_fill_markets),
        markets_with_clearing_prices: markets
            .iter()
            .filter(|market| market.clearing_price_present)
            .count() as u32,
        total_fills: block.fill_count,
        total_rejections: block.rejection_count,
        total_volume_nanos: block.total_volume_nanos,
        actors,
        markets,
    };
    metrics::gauge!("sybil_actor_mm_market_coverage_ratio")
        .set(response.mm_coverage_bps as f64 / 10_000.0);
    metrics::gauge!("sybil_actor_noise_market_coverage_ratio").set(
        if response.active_markets == 0 {
            0.0
        } else {
            response.noise_markets_selected as f64 / response.active_markets as f64
        },
    );
    metrics::gauge!("sybil_actor_mm_two_sided_coverage_ratio_rolling")
        .set(response.rolling_mm_two_sided_coverage_bps as f64 / 10_000.0);
    metrics::gauge!("sybil_actor_noise_market_coverage_ratio_rolling")
        .set(response.rolling_noise_coverage_bps as f64 / 10_000.0);
    metrics::gauge!("sybil_actor_noise_mm_cross_coverage_ratio_rolling")
        .set(response.rolling_noise_crossing_coverage_bps as f64 / 10_000.0);
    metrics::gauge!("sybil_actor_noise_fill_market_coverage_ratio_rolling")
        .set(response.rolling_noise_fill_coverage_bps as f64 / 10_000.0);
    Ok(Json(response))
}

#[utoipa::path(
    post,
    path = "/v1/liquidity/universe/activate",
    request_body = ActivateLiquidityUniverseRequest,
    responses((status = 200, body = LiquidityUniverseResponse))
)]
pub async fn activate_liquidity_universe(
    State(state): State<AppState>,
    Json(request): Json<ActivateLiquidityUniverseRequest>,
) -> Result<Json<LiquidityUniverseResponse>, AppError> {
    let digest_bytes = hex::decode(request.policy_digest_hex.trim_start_matches("0x"))
        .map_err(|_| AppError::bad_request("policy_digest_hex is not valid hex"))?;
    let policy_digest: [u8; 32] = digest_bytes
        .try_into()
        .map_err(|_| AppError::bad_request("policy_digest_hex must contain 32 bytes"))?;
    let snapshot = state
        .sequencer
        .activate_liquidity_universe(
            request.generation,
            policy_digest,
            request.market_ids.into_iter().map(MarketId).collect(),
        )
        .await?;
    Ok(Json(LiquidityUniverseResponse {
        generation: snapshot.generation,
        policy_digest_hex: hex::encode(snapshot.policy_digest),
        activated_at_height: snapshot.activated_at_height,
        market_ids: snapshot.market_ids.iter().map(|market| market.0).collect(),
        committed_market_ids: snapshot
            .market_ids
            .into_iter()
            .map(|market| market.0)
            .collect(),
        actor_ready: false,
        principal_id: None,
        actor_role: None,
        account_id: None,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/actor/epochs",
    request_body = ActorEpochRequest,
    responses((status = 200, body = ActorEpochResponse))
)]
pub async fn submit_actor_epoch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ActorEpochRequest>,
) -> Result<Json<ActorEpochResponse>, AppError> {
    let credential = authenticate_actor(&state, &headers)?.clone();
    let universe = state.sequencer.get_liquidity_universe().await?;
    if universe.generation == 0 {
        return Err(AppError::service_unavailable(
            "Actor liquidity universe is not activated",
        ));
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if request.observed_at_ms > now_ms.saturating_add(5_000)
        || now_ms.saturating_sub(request.observed_at_ms) > 30_000
        || request.valid_until_ms < now_ms
        || request.valid_until_ms < request.observed_at_ms
        || request.valid_until_ms > request.observed_at_ms.saturating_add(60_000)
    {
        return Err(AppError::bad_request(
            "Actor epoch timestamp window is invalid or stale",
        ));
    }
    if request.epoch_id.is_empty() || request.epoch_id.len() > 128 {
        return Err(AppError::bad_request("epoch_id must contain 1..=128 bytes"));
    }
    if request.universe_generation != universe.generation {
        return Err(AppError::conflict(
            "Actor epoch universe generation is stale",
        ));
    }

    let expected = universe
        .market_ids
        .iter()
        .map(|market| market.0)
        .collect::<BTreeSet<_>>();
    let supplied = request
        .market_intents
        .iter()
        .map(|intent| intent.market_id)
        .collect::<BTreeSet<_>>();
    let supplied_are_valid = supplied.len() == request.market_intents.len()
        && match credential.role {
            ActorRole::MarketMaker => supplied == expected,
            ActorRole::Noise => supplied.is_subset(&expected),
        };
    if !supplied_are_valid {
        return Err(AppError::bad_request(match credential.role {
            ActorRole::MarketMaker => {
                "MM market_intents must contain every active market exactly once"
            }
            ActorRole::Noise => {
                "Noise market_intents must be unique active markets; omitted markets mean random_not_selected"
            }
        }));
    }

    let market_count = expected.len();
    let role_cap = match credential.role {
        ActorRole::MarketMaker => (4 * market_count + 32).min(1_024),
        ActorRole::Noise => 32,
    };
    match credential.role {
        ActorRole::MarketMaker if request.mm_budget_nanos.is_none() => {
            return Err(AppError::bad_request("MM epoch requires mm_budget_nanos"));
        }
        ActorRole::Noise if request.mm_budget_nanos.is_some() => {
            return Err(AppError::bad_request(
                "Noise epoch cannot set mm_budget_nanos",
            ));
        }
        _ => {}
    }

    let markets = state.sequencer.list_markets().await?;
    let mut orders = Vec::new();
    let mut mm_sides = Vec::new();
    let mut receipt_shapes = Vec::<(u32, usize, Option<String>, Option<String>)>::new();
    for intent in &request.market_intents {
        if intent.skip_reason.is_some() && !intent.orders.is_empty() {
            return Err(AppError::bad_request(
                "a market intent cannot contain both orders and skip_reason",
            ));
        }
        if intent.orders.is_empty() {
            let reason = intent.skip_reason.as_deref().unwrap_or("").trim();
            if reason.is_empty() || reason.len() > 160 {
                return Err(AppError::bad_request(
                    "every empty market intent requires a 1..=160 byte skip_reason",
                ));
            }
        }
        let per_market_cap = match credential.role {
            ActorRole::MarketMaker => 4,
            ActorRole::Noise => 1,
        };
        if intent.orders.len() > per_market_cap {
            return Err(AppError::bad_request(format!(
                "market {} exceeds role per-market order cap {}",
                intent.market_id, per_market_cap
            )));
        }
        let start = orders.len();
        let mut rejection = None;
        for spec in &intent.orders {
            if spec_market_id(spec) != intent.market_id {
                rejection = Some("order market_id does not match its intent".to_string());
                continue;
            }
            match order_spec_to_order(spec, &markets) {
                Ok(order) => {
                    orders.push(order);
                    mm_sides.push(super::orders::mm_side_from_spec(spec));
                }
                Err(error) => rejection = Some(error),
            }
        }
        if let Some(error) = rejection {
            return Err(AppError::bad_request(format!(
                "market {} has an invalid order: {}",
                intent.market_id, error
            )));
        }
        receipt_shapes.push((
            intent.market_id,
            orders.len() - start,
            None,
            intent.skip_reason.clone(),
        ));
    }
    if orders.len() > role_cap {
        return Err(AppError::bad_request(format!(
            "actor epoch contains {} orders, role cap is {}",
            orders.len(),
            role_cap
        )));
    }

    let mm_constraint = request.mm_budget_nanos.map(|budget| {
        let mut constraint = MmConstraint::new(MmId(credential.account_id), Nanos(budget));
        for (index, side) in mm_sides.iter().copied().enumerate() {
            constraint.add_order(index as u64, side);
        }
        constraint
    });
    let payload = serde_json::to_vec(&request)
        .map_err(|error| AppError::bad_request(format!("cannot encode actor epoch: {error}")))?;
    let accepted_mm_quotes = if credential.role == ActorRole::Noise {
        state
            .mm_quote_snapshots
            .read()
            .await
            .get(&request.target_height)
            .map(|snapshot| {
                snapshot
                    .markets
                    .iter()
                    .map(|quote| (quote.market_id, quote.clone()))
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default()
    } else {
        HashMap::new()
    };
    let epoch = ActorEpochSubmission {
        principal_id: credential.principal_id.clone(),
        role: sequencer_role(credential.role),
        epoch_id: request.epoch_id,
        payload_digest: Sha256::digest(&payload).into(),
        target_height: request.target_height,
        valid_until_ms: request.valid_until_ms,
        universe_generation: request.universe_generation,
        covered_market_ids: universe.market_ids,
        submission: OrderSubmission {
            account_id: AccountId(credential.account_id),
            orders,
            mm_constraint,
        },
    };
    let observation = ActorEpochObservation {
        principal_id: credential.principal_id.clone(),
        role: credential.role,
        target_height: request.target_height,
        valid_until_ms: request.valid_until_ms,
        markets: request
            .market_intents
            .iter()
            .map(|intent| {
                (
                    intent.market_id,
                    ActorMarketObservation {
                        order_count: intent.orders.len(),
                        skip_reason: intent.skip_reason.clone(),
                        crosses_mm_quote: accepted_mm_quotes.get(&intent.market_id).is_some_and(
                            |quote| {
                                intent
                                    .orders
                                    .iter()
                                    .any(|order| order_crosses_mm_quote(order, quote))
                            },
                        ),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
    };
    let mm_snapshot =
        (credential.role == ActorRole::MarketMaker).then(|| MmQuoteSnapshotResponse {
            target_height: request.target_height,
            universe_generation: request.universe_generation,
            observed_at_ms: request.observed_at_ms,
            markets: request.market_intents.iter().map(mm_quote_market).collect(),
        });
    let selected = request
        .market_intents
        .iter()
        .filter(|intent| !intent.orders.is_empty())
        .count() as u32;
    let order_ids = state.sequencer.submit_actor_epoch(epoch).await?;
    let accepted_orders = order_ids.len() as u32;
    state.record_actor_epoch_observation(observation).await;
    if let Some(snapshot) = mm_snapshot {
        state.record_mm_quote_snapshot(snapshot).await;
    }
    let mut cursor = 0usize;
    let market_receipts = receipt_shapes
        .into_iter()
        .map(|(market_id, count, rejection, skip_reason)| {
            let accepted_order_ids = order_ids[cursor..cursor + count].to_vec();
            cursor += count;
            ActorMarketReceipt {
                market_id,
                accepted_order_ids,
                rejection,
                skip_reason,
            }
        })
        .collect();
    Ok(Json(ActorEpochResponse {
        accepted: true,
        principal_id: credential.principal_id,
        target_height: request.target_height,
        universe_generation: request.universe_generation,
        considered: market_count as u32,
        selected,
        accepted_orders,
        skipped: (market_count as u32).saturating_sub(selected),
        markets: market_receipts,
    }))
}

#[utoipa::path(post, path = "/v1/actor/inventory", request_body = CompleteSetInventoryRequest)]
pub async fn update_complete_set_inventory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CompleteSetInventoryRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let credential = authenticate_actor(&state, &headers)?.clone();
    if credential.role != ActorRole::MarketMaker {
        return Err(AppError::forbidden(
            "Only the market-maker principal can manage complete-set inventory",
        ));
    }
    if request.actions.is_empty() || request.actions.len() > 1_024 {
        return Err(AppError::bad_request(
            "complete-set inventory batch must contain 1..=1024 actions",
        ));
    }
    let mut touched = BTreeSet::new();
    for action in &request.actions {
        let (market_id, quantity) = match action {
            CompleteSetActionRequest::Collateralize {
                market_id,
                quantity,
            }
            | CompleteSetActionRequest::Redeem {
                market_id,
                quantity,
            } => (*market_id, *quantity),
        };
        if quantity == 0 || !touched.insert(market_id) {
            return Err(AppError::bad_request(
                "inventory actions require positive quantity and unique market ids",
            ));
        }
    }
    let actions = request
        .actions
        .into_iter()
        .map(|action| match action {
            CompleteSetActionRequest::Collateralize {
                market_id,
                quantity,
            } => matching_sequencer::CompleteSetInventoryAction {
                market_id: MarketId(market_id),
                quantity: Qty(quantity),
                collateralize: true,
            },
            CompleteSetActionRequest::Redeem {
                market_id,
                quantity,
            } => matching_sequencer::CompleteSetInventoryAction {
                market_id: MarketId(market_id),
                quantity: Qty(quantity),
                collateralize: false,
            },
        })
        .collect();
    state
        .sequencer
        .apply_complete_set_inventory_actions(AccountId(credential.account_id), actions)
        .await?;
    Ok(Json(serde_json::json!({"accepted": true})))
}
