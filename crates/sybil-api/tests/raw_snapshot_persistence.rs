//! Process-level restart coverage for raw Polymarket event JSON snapshots
//! (SYB-153).
//!
//! Spawns the real `sybil-api` binary with a persistent `SYBIL_EVENT_SNAPSHOT_DIR`
//! (under the test root, which survives a restart), PUTs a raw event snapshot,
//! restarts the process, and asserts `GET /v1/events/{id}/raw` still serves the
//! exact JSON. This proves the boot path no longer wipes the snapshot dir: the
//! mirror does NOT have to re-fetch from Polymarket for the frontend's
//! multi-card labels / descriptions / resolution source to survive a restart.
//!
//! Kept in its own file (not `process_restart.rs`) per SYB-153 so it doesn't
//! collide with concurrent work extending that suite.

use reqwest::StatusCode;
use serde_json::{json, Value};

mod common;

use common::process::{
    get_status_and_body, restart_api_with_env, spawn_api_with_env, wait_for_health, ProcessTestRoot,
};

const BLOCK_INTERVAL_MS: u64 = 200;

/// PUT a raw event snapshot; the endpoint is open in dev mode (the harness
/// spawns with `--dev-mode`), so no service token is needed.
async fn put_raw(client: &reqwest::Client, base_url: &str, event_id: &str, body: &Value) {
    let resp = client
        .put(format!("{base_url}/v1/events/{event_id}/raw"))
        .json(body)
        .send()
        .await
        .expect("PUT request succeeds");
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    assert!(
        status.is_success(),
        "PUT /v1/events/{event_id}/raw failed with {status}: {text}"
    );
}

#[tokio::test]
async fn raw_event_snapshot_survives_restart() {
    let root = ProcessTestRoot::new("raw-snapshot");
    // Snapshot dir lives under the test root, which persists across a restart.
    // This mirrors prod, where the dir sits on the durable `sybil-data` volume.
    let snapshot_dir = root.data_dir().join("event_snapshots");
    let snapshot_dir_arg = snapshot_dir.to_string_lossy().into_owned();
    let env = [("SYBIL_EVENT_SNAPSHOT_DIR", snapshot_dir_arg.as_str())];

    let client = reqwest::Client::new();

    let mut process = spawn_api_with_env(
        root.data_dir(),
        root.admin_key_path(),
        BLOCK_INTERVAL_MS,
        &env,
    )
    .await;
    wait_for_health(&client, &process.base_url).await;

    // Store a raw event snapshot shaped like the Gamma JSON the FE reads (multi
    // outcome labels, description, resolution source, neg-risk hint).
    let event_id = "event-abc_123";
    let payload = json!({
        "id": event_id,
        "title": "Will it happen?",
        "description": "Full market description text.",
        "resolutionSource": "https://example.com/resolution",
        "negRisk": true,
        "markets": [
            { "conditionId": "0xcond1", "groupItemTitle": "Up 200,000" },
            { "conditionId": "0xcond2", "groupItemTitle": "Down 200,000" }
        ]
    });
    put_raw(&client, &process.base_url, event_id, &payload).await;

    // Refresh semantics (SYB-153): a later mirror cycle overwrites by event id.
    // Re-PUT an updated snapshot and confirm the newest version wins in place.
    let updated = json!({
        "id": event_id,
        "title": "Will it happen? (updated)",
        "description": "Revised market description text.",
        "resolutionSource": "https://example.com/resolution-v2",
        "negRisk": false,
        "markets": [
            { "conditionId": "0xcond1", "groupItemTitle": "Up 250,000" },
            { "conditionId": "0xcond2", "groupItemTitle": "Down 250,000" }
        ]
    });
    put_raw(&client, &process.base_url, event_id, &updated).await;

    // Present before restart — reflects the latest overwrite.
    let (status, body) = get_status_and_body(
        &client,
        &process.base_url,
        &format!("/v1/events/{event_id}/raw"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "snapshot readable before restart");
    assert_eq!(
        body, updated,
        "snapshot reflects the latest overwrite-by-id"
    );

    // Restart the process with the SAME persistent snapshot dir.
    process = restart_api_with_env(process, &root, BLOCK_INTERVAL_MS, &env).await;
    wait_for_health(&client, &process.base_url).await;

    // The core assertion: raw JSON survives restart WITHOUT any re-sync.
    let (status, body) = get_status_and_body(
        &client,
        &process.base_url,
        &format!("/v1/events/{event_id}/raw"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "raw event snapshot must still be served after restart (SYB-153)"
    );
    assert_eq!(
        body, updated,
        "the latest raw event JSON must survive restart byte-identical"
    );

    // Sanity: an event that was never stored still 404s (dir wasn't clobbered
    // into serving stale/other data).
    let (status, _) =
        get_status_and_body(&client, &process.base_url, "/v1/events/never-stored/raw").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown event still 404s");

    process.kill().await;
}
