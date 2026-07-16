use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tokio::sync::{RwLock, mpsc, watch};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{error, info, warn};

use sybil_client::SybilClient;
use sybil_market_maker::{MmActor, MmMessage, MmProgress, PriceSnapshot, dollars_to_nanos};
use sybil_polymarket::config::Config;
use sybil_polymarket::feed::FeedActor;
use sybil_polymarket::mapping::MappingStore;
use sybil_polymarket::polymarket::gamma::GammaClient;
use sybil_polymarket::resolution::ResolutionActor;
use sybil_polymarket::signer::ResolutionSigner;
use sybil_polymarket::sync::SyncActor;

/// Reattach to the persisted MM account, or mint and persist a fresh one (PM-7).
///
/// A fresh account minted on every process start orphans prior inventory while
/// the real exposure persists on-chain. We look up the account id stored in the
/// mapping store and reuse it when the server still recognises it; otherwise we
/// create a new account and persist the id so the next restart reattaches. This
/// mirrors the arena's bot-account reattach (AR-3).
async fn resolve_mm_account(
    client: &SybilClient,
    mapping: &Arc<RwLock<MappingStore>>,
    balance_nanos: u64,
) -> Result<u64, Box<dyn std::error::Error>> {
    if let Some(account_id) = mapping.read().await.mm_account_id() {
        match client.get_account(account_id).await {
            Ok(_) => {
                info!(account_id, "reattached to persisted MM account");
                return Ok(account_id);
            }
            Err(e) => {
                warn!(
                    account_id,
                    error = %e,
                    "persisted MM account unusable; minting a new one"
                );
            }
        }
    }

    // The mirror has service authority and submits unsigned MM orders, so it
    // intentionally uses the deprecated operator-only bare-account variant.
    let account = client.create_bare_account(balance_nanos).await?;
    {
        let mut map = mapping.write().await;
        map.set_mm_account_id(account.account_id);
        if let Err(e) = map.save() {
            warn!(error = %e, "failed to persist MM account id (will re-mint next restart)");
        }
    }
    Ok(account.account_id)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Install rustls crypto provider (needed for WebSocket TLS)
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sybil_polymarket=info".into()),
        )
        .init();

    let config = Config::parse();
    let config_mm = config.market_maker_config().validate()?;
    let mm_initial_balance_nanos = dollars_to_nanos(
        "mm_initial_balance_dollars",
        config.mm_initial_balance_dollars,
    )?;
    info!(?config, "starting sybil-polymarket");

    // Curated seed set (SYB-150). Parent event ids are fetch keys; exact child
    // condition ids, when present, are the authoritative mirror allow-list.
    // Parse failure is fatal so a typo cannot fall back to the broad scan.
    let (curated_event_ids, curated_condition_ids): (Vec<String>, Vec<String>) =
        if config.curated_markets_path.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            let curated = sybil_polymarket::curated::CuratedMarkets::load(std::path::Path::new(
                &config.curated_markets_path,
            ))?;
            let events = curated.event_ids();
            let conditions = curated.condition_ids();
            info!(
                path = %config.curated_markets_path,
                events = events.len(),
                conditions = conditions.len(),
                "loaded curated mirror allow-list"
            );
            (events, conditions)
        };
    let sybil_service_token = std::env::var("SYBIL_SERVICE_TOKEN")
        .ok()
        .and_then(|value| (!value.trim().is_empty()).then_some(value));

    // Shared HTTP client
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    // Clients
    let gamma_client = GammaClient::new(
        http.clone(),
        config.gamma_url.clone(),
        config.clob_url.clone(),
    );
    let sybil_client_sync = SybilClient::new(
        http.clone(),
        config.sybil_url.clone(),
        sybil_service_token.clone(),
    );
    let sybil_client_mm = SybilClient::new(
        http.clone(),
        config.sybil_url.clone(),
        sybil_service_token.clone(),
    );
    let gamma_client_feed = GammaClient::new(
        http.clone(),
        config.gamma_url.clone(),
        config.clob_url.clone(),
    );
    let gamma_client_resolution = GammaClient::new(
        http.clone(),
        config.gamma_url.clone(),
        config.clob_url.clone(),
    );
    let sybil_client_resolution =
        SybilClient::new(http.clone(), config.sybil_url.clone(), sybil_service_token);

    // Wait for Sybil to be healthy
    info!(url = &config.sybil_url, "waiting for Sybil API...");
    let health = loop {
        match sybil_client_sync.health().await {
            Ok(h) if h.status == "ok" => break h,
            Ok(h) => {
                info!(status = h.status, "Sybil not ready, retrying...");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            Err(e) => {
                info!(error = %e, "Sybil not reachable, retrying...");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    };
    info!("Sybil API is healthy");
    let genesis_hash = health
        .genesis_hash
        .ok_or_else(|| std::io::Error::other("healthy Sybil API returned no genesis hash"))?;

    // Mapping identity is explicit: a file is valid for exactly one canonical
    // Sybil chain and never inferred from whichever market ids happen to exist.
    let mapping_store = if config.mapping_store_path.is_empty() {
        MappingStore::new(None, &genesis_hash)
    } else {
        let path = PathBuf::from(&config.mapping_store_path);
        MappingStore::load(&path, &genesis_hash)?
    };
    info!(
        events = mapping_store.event_count(),
        markets = mapping_store.market_count(),
        %genesis_hash,
        "loaded genesis-bound mapping store"
    );
    let mapping = Arc::new(RwLock::new(mapping_store));

    // Resolve the MM account: reattach to the persisted one when the server
    // still knows it, otherwise mint and persist a fresh account (PM-7).
    let mm_account_id =
        resolve_mm_account(&sybil_client_sync, &mapping, mm_initial_balance_nanos).await?;
    info!(
        account_id = mm_account_id,
        balance_dollars = config.mm_initial_balance_dollars,
        "MM account ready"
    );

    // Channels — size MM channel to fit all existing markets for bootstrap.
    // When category filters are configured, apply the same filtered universe to
    // persisted mappings so an old broad mapping does not silently re-expand MM.
    let allowed_conditions = if !curated_condition_ids.is_empty() {
        Some(
            curated_condition_ids
                .iter()
                .cloned()
                .collect::<HashSet<_>>(),
        )
    } else if !curated_event_ids.is_empty() {
        // Curated mode: scope the MM bootstrap to the curated events' active
        // conditions so a broad persisted mapping cannot re-expand the MM.
        match gamma_client.fetch_curated_events(&curated_event_ids).await {
            Ok(events) => {
                let conditions: HashSet<String> = events
                    .iter()
                    .flat_map(|event| event.markets.iter())
                    .filter(|market| market.active && !market.closed)
                    .map(|market| market.condition_id.clone())
                    .collect();
                info!(
                    allowed_conditions = conditions.len(),
                    "scoped MM bootstrap to curated events"
                );
                Some(conditions)
            }
            Err(error) => {
                warn!(
                    error = %error,
                    "failed to fetch curated events; skipping persisted mirrored MM bootstrap"
                );
                // Curated mode is an allowlist. A transient Gamma failure must
                // not turn it into "quote every market ever persisted".
                Some(HashSet::new())
            }
        }
    } else if config.mirror_categories.is_empty() && config.mirror_excluded_categories.is_empty() {
        None
    } else {
        match gamma_client
            .fetch_active_events(
                config.max_events,
                &config.mirror_categories,
                &config.mirror_excluded_categories,
                config.min_volume_usd,
            )
            .await
        {
            Ok(events) => {
                let conditions: HashSet<String> = events
                    .iter()
                    .flat_map(|event| event.markets.iter())
                    .filter(|market| market.active && !market.closed)
                    .map(|market| market.condition_id.clone())
                    .collect();
                info!(
                    allowed_conditions = conditions.len(),
                    "filtered existing mapping for MM bootstrap"
                );
                Some(conditions)
            }
            Err(error) => {
                warn!(
                    error = %error,
                    "failed to fetch category-filtered events; bootstrapping all persisted mapped markets"
                );
                None
            }
        }
    };

    let mut existing_mirror = {
        let mapping = mapping.read().await;
        match &allowed_conditions {
            Some(conditions) => mapping.all_markets_for_conditions(conditions),
            None => mapping.all_markets(),
        }
    };
    existing_mirror.sort_by_key(|(sybil_market_id, _, _, _)| std::cmp::Reverse(*sybil_market_id));

    let mut existing_mm: Vec<_> = existing_mirror
        .iter()
        .map(
            |(sybil_market_id, yes_token_id, group_key, group_size)| MmMessage::MarketMirrored {
                sybil_market_id: *sybil_market_id,
                yes_token_id: yes_token_id.clone(),
                initial_mid: 0.5,
                group_key: group_key.clone(),
                group_size: *group_size,
            },
        )
        .collect();

    if config.mm_max_markets > 0 && existing_mm.len() > config.mm_max_markets {
        info!(
            total = existing_mm.len(),
            active = config.mm_max_markets,
            "limiting MM bootstrap to configured market cap"
        );
        existing_mm.truncate(config.mm_max_markets);
    } else if config.mm_max_markets == 0 {
        info!(
            total = existing_mm.len(),
            "MM market cap disabled; bootstrapping all filtered mapped markets"
        );
    }
    let mm_channel_size = (existing_mm.len() + 256).max(256);
    let (feed_tx, feed_rx) = mpsc::channel(64);
    let (mm_tx, mm_rx) = mpsc::channel(mm_channel_size);
    let (price_tx, price_rx) = watch::channel(PriceSnapshot::default());
    // Live-set channel: MM publishes how many markets it is actively quoting so
    // Sync recycles `mm_max_markets` slots as markets resolve/untrack (PM-8).
    let (mm_live_tx, mm_live_rx) = watch::channel(MmProgress::default());

    // Bootstrap MM with existing markets from mapping
    if !existing_mm.is_empty() {
        info!(
            count = existing_mm.len(),
            "bootstrapping MM with existing markets"
        );
        for msg in &existing_mm {
            let _ = mm_tx.try_send(msg.clone());
        }
    }

    // Bootstrap Feed with existing token subscriptions
    let all_tokens: Vec<String> = existing_mm
        .iter()
        .filter_map(|msg| match msg {
            MmMessage::MarketMirrored { yes_token_id, .. } => Some(yes_token_id.clone()),
            MmMessage::MarketNative { .. } => None,
        })
        .collect();
    if !all_tokens.is_empty() {
        info!(
            count = all_tokens.len(),
            "bootstrapping Feed with existing tokens"
        );
        let _ = feed_tx.try_send(sybil_polymarket::feed::FeedMessage::SubscribeTokens(
            all_tokens,
        ));
    }

    // Cancellation
    let cancel = CancellationToken::new();
    let tasks = TaskTracker::new();
    let cancel_sync = cancel.clone();
    let cancel_feed = cancel.clone();
    let cancel_mm = cancel.clone();

    // Spawn actors
    let config_sync = config.clone();
    let config_feed = config.clone();
    let mapping_for_sync = mapping.clone();
    let sync_handle = tasks.spawn(async move {
        let actor = SyncActor::new(
            config_sync,
            gamma_client,
            sybil_client_sync,
            mapping_for_sync,
            feed_tx,
            mm_tx,
            mm_live_rx,
            curated_event_ids,
            curated_condition_ids,
        );
        actor.run(cancel_sync).await;
    });

    let feed_handle = tasks.spawn(async move {
        let actor = FeedActor::new(config_feed, gamma_client_feed, price_tx, feed_rx);
        actor.run(cancel_feed).await;
    });

    let mm_handle = tasks.spawn(async move {
        let actor = MmActor::new(
            config_mm,
            sybil_client_mm,
            mm_account_id,
            price_rx,
            mm_rx,
            mm_live_tx,
        );
        actor.run(cancel_mm).await;
    });

    // Resolution actor — or a no-op placeholder when no signer key is
    // configured, so the select! below treats it uniformly. A panic in the
    // real resolution actor needs to trip shutdown the same way sync/feed/mm
    // do; otherwise Polymarket auto-resolution could stop silently while the
    // process keeps looking healthy.
    let resolution_handle: tokio::task::JoinHandle<()> = if config.signer_key_path.is_empty() {
        info!("SIGNER_KEY_PATH not set; resolution actor disabled");
        let cancel_idle = cancel.clone();
        tasks.spawn(async move { cancel_idle.cancelled().await })
    } else {
        let signer =
            ResolutionSigner::load_or_create(std::path::Path::new(&config.signer_key_path))?;
        info!(
            pubkey = signer.pubkey_hex(),
            "loaded resolution signer; register this pubkey as the polymarket_mirror feed on sybil-api"
        );
        let config_res = config.clone();
        let cancel_res = cancel.clone();
        let mapping_for_res = mapping.clone();
        tasks.spawn(async move {
            let actor = ResolutionActor::new(
                config_res,
                gamma_client_resolution,
                sybil_client_resolution,
                mapping_for_res,
                signer,
            );
            actor.run(cancel_res).await;
        })
    };

    // Wait for shutdown signal
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("received Ctrl+C, shutting down...");
        }
        r = sync_handle => {
            if let Err(e) = r {
                error!(error = %e, "SyncActor panicked");
            }
        }
        r = feed_handle => {
            if let Err(e) = r {
                error!(error = %e, "FeedActor panicked");
            }
        }
        r = mm_handle => {
            if let Err(e) = r {
                error!(error = %e, "MmActor panicked");
            }
        }
        r = resolution_handle => {
            if let Err(e) = r {
                error!(error = %e, "ResolutionActor panicked");
            } else {
                error!("ResolutionActor exited unexpectedly");
            }
        }
    }

    cancel.cancel();
    tasks.close();
    tasks.wait().await;
    info!("shutdown complete");
    Ok(())
}
