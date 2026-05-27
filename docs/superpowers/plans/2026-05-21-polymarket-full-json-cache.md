# Polymarket Full-JSON Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the full Polymarket event/market JSON available to the frontend — stored in a dedicated, restart-wiped folder on `sybil-api` (the only origin the FE talks to), populated by the mirror.

**Architecture:** The mirror already has the full `GammaEvent` JSON in hand every sync cycle (Gamma's `/events` list returns it; we currently discard it — **no extra Polymarket fetch needed**). The mirror has no HTTP port and a separate volume, and the FE only talks to `sybil-api`, so the mirror **pushes** raw JSON to a new dev-mode `sybil-api` endpoint (mirroring the existing `set_market_metadata` pattern). `sybil-api` writes one `{event_id}.json` per event into a configured folder, serves it at `GET /v1/events/{id}/raw`, and **wipes+recreates the folder on startup** so it never grows across restarts. The mirror pushes idempotently every cycle (cheap; the JSON is already in hand), which keeps the folder correct within one cycle after either process restarts. To stay future-proof ("the field list could change"), the Gamma structs gain `Serialize` + a `#[serde(flatten)]` catch-all so re-serialization reproduces all fields (known + any new ones) in camelCase.

**Tech Stack:** Rust, serde (flatten), axum, reqwest, Docker Compose.

**Conventions:** jj VCS; `just fmt`/`just lint`; Rust tests via `cargo test -p <crate>`.

**Delivery decision (from brainstorming):** mirror → POST/PUT to sybil-api (not a shared volume or a mirror-served port).

**Reviewable tradeoff:** the mirror re-pushes each active event every cycle (idempotent upsert) rather than strictly once, because that's the only simple way to repopulate after a `sybil-api` restart wipes the folder. Cost is ~`max_events` small PUTs per `sync-interval` (negligible). Alternative if undesired: push only on new-event and persist the folder instead of wiping — but that contradicts the "wipe on restart" requirement.

---

## File Structure

**Mirror (`sybil-polymarket`)**
- `src/polymarket/types.rs` — `GammaTag`/`GammaEvent`/`GammaMarket` gain `Serialize`; `GammaEvent`/`GammaMarket` gain a flatten catch-all. Round-trip test.
- `src/sybil/client.rs` — new `put_event_raw` method.
- `src/sync.rs` — push each active event's JSON each cycle.

**API (`sybil-api`)**
- `src/config.rs` — `event_snapshot_dir` arg + Default.
- `src/state.rs` — `AppState.event_snapshot_dir: Option<PathBuf>`.
- `src/main.rs` — wipe+recreate the folder on startup.
- `src/routes/events.rs` (new) + `src/routes/mod.rs` — `put_event_raw` / `get_event_raw`.
- `src/app.rs` — register both routes.
- `tests/common/mod.rs` — `put_json` helper; `tests/api_integration.rs` — endpoint test.

**Deploy**
- `docker-compose.yml` — `SYBIL_EVENT_SNAPSHOT_DIR` on `sybil-api`.

---

## Task 1: Make the Gamma types serializable and future-proof

**Files:**
- Modify/Test: `crates/sybil-polymarket/src/polymarket/types.rs`

- [ ] **Step 1: Write the failing round-trip test**

Add to `types.rs` (create a `#[cfg(test)] mod tests` at the end if none exists):

```rust
#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn gamma_event_roundtrips_including_unknown_fields() {
        let raw = serde_json::json!({
            "id": "1",
            "title": "T",
            "negRisk": true,
            "markets": [],
            "someBrandNewField": { "a": 1 }
        });
        let ev: GammaEvent = serde_json::from_value(raw).unwrap();
        let back = serde_json::to_value(&ev).unwrap();
        assert_eq!(back["id"], "1");
        assert_eq!(back["negRisk"], true);
        // A field the struct doesn't model survives via the flatten catch-all.
        assert_eq!(back["someBrandNewField"], serde_json::json!({ "a": 1 }));
    }
}
```

- [ ] **Step 2: Run, verify failure**

Run: `cargo test -p sybil-polymarket gamma_event_roundtrips_including_unknown_fields`
Expected: FAIL — `GammaEvent` is not `Serialize` (won't compile).

- [ ] **Step 3: Derive `Serialize` and add flatten catch-alls**

In `types.rs`:
- Line 90 (`GammaTag`): `#[derive(Debug, Clone, Default, Deserialize, Serialize)]`
- Line 100 (`GammaEvent`): `#[derive(Debug, Clone, Deserialize, Serialize)]`
- Line 200 (`GammaMarket`): `#[derive(Debug, Clone, Deserialize, Serialize)]`

Add a flatten catch-all as the **last field** of `GammaEvent` (after `icon`, line 137):

```rust
    /// Any Gamma fields not modelled above, preserved verbatim so stored
    /// snapshots stay complete as Polymarket's schema evolves.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
```

And as the last field of `GammaMarket` (after `resolved_by`, line 253):

```rust
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
```

Ensure `Serialize` is imported (top of file: `use serde::{Deserialize, Serialize};`).

- [ ] **Step 4: Run, verify pass**

Run: `cargo test -p sybil-polymarket gamma_event_roundtrips_including_unknown_fields`
Expected: PASS.

- [ ] **Step 5: Build the crate (catch any struct-literal construction sites that now need `extra`)**

Run: `cargo build -p sybil-polymarket`
Expected: PASS. If any code constructs `GammaEvent`/`GammaMarket` with struct literals (e.g. tests), add `extra: Default::default(),`.

- [ ] **Step 6: Commit**

```bash
just fmt && just lint
jj describe -m "feat(polymarket): make Gamma types Serialize + flatten catch-all for full snapshots"
```

---

## Task 2: sybil-api config, state, and startup wipe

**Files:**
- Modify: `crates/sybil-api/src/config.rs` (after line 142; Default ~line 188)
- Modify: `crates/sybil-api/src/state.rs` (struct ~line 190; `new` ~line 204-223)
- Modify: `crates/sybil-api/src/main.rs` (after config parse, ~line 125)

- [ ] **Step 1: Add the config field**

In `config.rs`, after the `market_ref_data_path` arg (line 142):

```rust
    /// Directory holding full Polymarket event JSON snapshots, served at
    /// `GET /v1/events/{id}/raw` and wiped+recreated on startup. Empty =
    /// disabled (the raw endpoints return 404).
    #[arg(long, default_value = "", env = "SYBIL_EVENT_SNAPSHOT_DIR")]
    pub event_snapshot_dir: String,
```

In the `impl Default for ApiConfig` block, next to `market_ref_data_path: String::new(),` (line 188):

```rust
            event_snapshot_dir: String::new(),
```

- [ ] **Step 2: Add the `AppState` field + resolve it in `new`**

In `state.rs`, in the `AppState` struct after `market_ref_data_path` (line 190):

```rust
    /// Directory for full Polymarket event JSON snapshots (`{event_id}.json`).
    /// `None` disables the raw-event endpoints. Wiped on startup in `main`.
    pub event_snapshot_dir: Option<PathBuf>,
```

In `AppState::new`, alongside the `market_ref_data_path` resolution (after line 208):

```rust
        let event_snapshot_dir = if config.event_snapshot_dir.is_empty() {
            None
        } else {
            Some(PathBuf::from(&config.event_snapshot_dir))
        };
```

and add to the returned `Self { ... }` (after `market_ref_data_path,`, line 220):

```rust
            event_snapshot_dir,
```

- [ ] **Step 3: Wipe+recreate the folder on startup**

In `main.rs`, after `let config = ApiConfig::parse();` (line 125):

```rust
    if !config.event_snapshot_dir.is_empty() {
        let dir = std::path::Path::new(&config.event_snapshot_dir);
        if dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(dir) {
                tracing::warn!(dir = %dir.display(), error = %e, "failed to wipe event snapshot dir");
            }
        }
        match std::fs::create_dir_all(dir) {
            Ok(()) => tracing::info!(dir = %dir.display(), "event snapshot dir ready (wiped on startup)"),
            Err(e) => tracing::warn!(dir = %dir.display(), error = %e, "failed to create event snapshot dir"),
        }
    }
```

- [ ] **Step 4: Build**

Run: `cargo build -p sybil-api`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
just fmt && just lint
jj describe -m "feat(api): event_snapshot_dir config, AppState field, startup wipe"
```

---

## Task 3: sybil-api raw-event endpoints

**Files:**
- Create: `crates/sybil-api/src/routes/events.rs`
- Modify: `crates/sybil-api/src/routes/mod.rs`
- Modify: `crates/sybil-api/src/app.rs` (near the `/v1/events/{event_id}/traders` route, line 615)
- Modify: `crates/sybil-api/tests/common/mod.rs` (add `put_json`)
- Test: `crates/sybil-api/tests/api_integration.rs`

- [ ] **Step 1: Add a `put_json` test helper**

In `crates/sybil-api/tests/common/mod.rs`, after `post_json` (line 176):

```rust
/// Send a PUT request with a JSON body and return (status, body bytes).
#[allow(dead_code)]
pub async fn put_json(app: Router, uri: &str, body: serde_json::Value) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .method(Method::PUT)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, body)
}
```

- [ ] **Step 2: Write the failing integration test**

In `api_integration.rs`, add `put_json` to the `use common::{...}` import, and add:

```rust
#[tokio::test]
async fn event_raw_snapshot_put_then_get() {
    let dir = std::env::temp_dir().join(format!("sybil-snap-{}-{}", std::process::id(), 1));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let (app, _) = test_app_with_config(ApiConfig {
        dev_mode: true,
        event_snapshot_dir: dir.to_string_lossy().into_owned(),
        ..ApiConfig::default()
    })
    .await;

    let payload = json!({ "id": "evt123", "description": "hi", "negRisk": true });
    let (status, _) = put_json(app.clone(), "/v1/events/evt123/raw", payload.clone()).await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get(app.clone(), "/v1/events/evt123/raw").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(parse_json(&body), payload);

    // Unknown event → 404.
    let (status, _) = get(app, "/v1/events/nope/raw").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
```

- [ ] **Step 3: Run, verify failure**

Run: `cargo test -p sybil-api --test api_integration event_raw_snapshot_put_then_get`
Expected: FAIL — routes 404 (not registered).

- [ ] **Step 4: Create the route handlers**

Create `crates/sybil-api/src/routes/events.rs`:

```rust
use std::path::{Path as FsPath, PathBuf};

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::state::AppState;
use crate::types::error::AppError;

/// Resolve `{dir}/{event_id}.json`, rejecting ids that could escape the dir.
fn snapshot_path(dir: &FsPath, event_id: &str) -> Option<PathBuf> {
    let safe = !event_id.is_empty()
        && event_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    safe.then(|| dir.join(format!("{event_id}.json")))
}

/// PUT /v1/events/{event_id}/raw — store the full Polymarket event JSON.
/// Dev-mode only (mirrors the metadata push). Body must be valid JSON.
pub async fn put_event_raw(
    State(state): State<AppState>,
    Path(event_id): Path<String>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, AppError> {
    if !state.dev_mode {
        return Err(AppError::dev_mode_required());
    }
    let dir = state
        .event_snapshot_dir
        .as_ref()
        .ok_or_else(|| AppError::not_found("event snapshots disabled"))?;
    let path = snapshot_path(dir, &event_id).ok_or_else(|| AppError::bad_request("invalid event_id"))?;
    serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|e| AppError::bad_request(format!("body is not JSON: {e}")))?;
    tokio::fs::write(&path, &body)
        .await
        .map_err(|e| AppError::internal(format!("snapshot write failed: {e}")))?;
    Ok(Json(serde_json::json!({ "stored": true })))
}

/// GET /v1/events/{event_id}/raw — return the stored event JSON, or 404.
pub async fn get_event_raw(
    State(state): State<AppState>,
    Path(event_id): Path<String>,
) -> Result<Response, AppError> {
    let dir = state
        .event_snapshot_dir
        .as_ref()
        .ok_or_else(|| AppError::not_found("event snapshots disabled"))?;
    let path = snapshot_path(dir, &event_id).ok_or_else(|| AppError::bad_request("invalid event_id"))?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::not_found("event snapshot not found"))?;
    Ok(([(axum::http::header::CONTENT_TYPE, "application/json")], bytes).into_response())
}
```

- [ ] **Step 5: Register the module**

In `crates/sybil-api/src/routes/mod.rs`, add (alphabetical, after `bridge`):

```rust
pub mod events;
```

- [ ] **Step 6: Register the routes**

In `crates/sybil-api/src/app.rs`, after the `/v1/events/{event_id}/traders` route (line 615-618):

```rust
        .route(
            "/v1/events/{event_id}/raw",
            axum::routing::get(routes::events::get_event_raw)
                .put(routes::events::put_event_raw),
        )
```

- [ ] **Step 7: Run, verify pass**

Run: `cargo test -p sybil-api --test api_integration event_raw_snapshot_put_then_get`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
just fmt && just lint
jj describe -m "feat(api): GET/PUT /v1/events/{id}/raw event JSON snapshots"
```

---

## Task 4: Mirror pushes raw JSON each cycle

**Files:**
- Modify: `crates/sybil-polymarket/src/sybil/client.rs` (after `set_market_metadata`, line 214)
- Modify: `crates/sybil-polymarket/src/sync.rs` (in `sync_once`, after the fetch, ~line 90)

- [ ] **Step 1: Add the client method**

In `client.rs`, after `set_market_metadata` (line 214):

```rust
    /// Push the full Polymarket event JSON to sybil-api's snapshot store.
    /// Idempotent upsert; dev-mode-only on the server (same as metadata).
    pub async fn put_event_raw(
        &self,
        event_id: &str,
        value: &serde_json::Value,
    ) -> Result<(), Error> {
        let resp = self
            .http
            .put(self.url(&format!("/v1/events/{}/raw", event_id)))
            .json(value)
            .send()
            .await?;
        let _ = self.check_response(resp).await?;
        Ok(())
    }
```

- [ ] **Step 2: Push each active event's JSON in `sync_once`**

In `sync.rs`, in `sync_once`, immediately after the `info!("fetched events…")` log (line 88) and before `let mut new_token_ids` (line 90):

```rust
        // Push the full event JSON to sybil-api so the FE can read it. We
        // already hold the parsed event (no extra Polymarket fetch); this is
        // an idempotent upsert each cycle, so the folder self-heals after a
        // restart of either process. Only events with a tradeable market.
        for event in &events {
            if !event.markets.iter().any(|m| m.active && !m.closed) {
                continue;
            }
            match serde_json::to_value(event) {
                Ok(value) => {
                    if let Err(e) = self.sybil_client.put_event_raw(&event.id, &value).await {
                        warn!(event_id = &event.id, error = %e, "failed to push event snapshot");
                    }
                }
                Err(e) => warn!(event_id = &event.id, error = %e, "failed to serialize event snapshot"),
            }
        }
```

> Implementer note: confirm the field name of the `SybilClient` on `self` (the mirror's sync struct). In the market-creation code it's used as `self.sybil_client` — match whatever the existing `create_market` call uses.

- [ ] **Step 3: Build the mirror**

Run: `cargo build -p sybil-polymarket`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
just fmt && just lint
jj describe -m "feat(polymarket): push full event JSON to sybil-api each sync cycle"
```

---

## Task 5: Wire the snapshot dir in Docker Compose

**Files:**
- Modify: `docker-compose.yml` (the `sybil-api` `environment:` block, ~line 8-15)

- [ ] **Step 1: Set the snapshot dir on `sybil-api`**

In `docker-compose.yml`, add to `sybil-api.environment`:

```yaml
      SYBIL_EVENT_SNAPSHOT_DIR: "/data/event_snapshots"
```

> Notes: the dir is wiped on startup, so persistence is irrelevant — it works on both the dev `sybil-api` (no `/data` mount → container-local) and prod (`sybil-data:/data`). `SYBIL_DEV_MODE=true` is already set on `sybil-api`, so the mirror's PUT is authorized (same as the metadata push).

- [ ] **Step 2: Commit**

```bash
jj describe -m "chore(compose): SYBIL_EVENT_SNAPSHOT_DIR for event JSON snapshots"
```

---

## Task 6: Manual end-to-end verification

- [ ] **Step 1: Run sybil-api with snapshots enabled + the mirror**

```bash
cargo run --release -p sybil-api -- --dev-mode --port 3001 --event-snapshot-dir /tmp/sybil-snaps &
cargo run --release -p sybil-polymarket -- --sybil-url=http://localhost:3001 --max-events=5
```

- [ ] **Step 2: Confirm snapshots land and serve, with the fields you need**

After one sync cycle:

```bash
EID=$(curl -s localhost:3001/v1/markets | jq -r '[.[] | select(.event_id != null)][0].event_id')
echo "event: $EID"
curl -s "localhost:3001/v1/events/$EID/raw" \
  | jq '{id, description, resolutionSource: .markets[0].resolutionSource, endDate, negRisk, tags: (.tags|length),
         m_question: .markets[0].question, m_outcomes: .markets[0].outcomes, m_groupItemTitle: .markets[0].groupItemTitle}'
ls -1 /tmp/sybil-snaps | head
```

Expected: a populated JSON object with the event-level fields (`id`, `description`, `resolutionSource`, `endDate`, `image`, `icon`, `negRisk`, `tags`) and market-level fields (`question`, `outcomes`, `groupItemTitle`, `negRisk`, …); one `{event_id}.json` file per mirrored event.

- [ ] **Step 3: Confirm the restart wipe**

Restart sybil-api (Ctrl-C, rerun), confirm `/tmp/sybil-snaps` is empty immediately after start, then repopulates within one mirror sync cycle.

---

## Self-Review Notes

- **Spec coverage:** "fetch its json when we first mirror" → mirror holds the full JSON each cycle, no extra fetch (Task 1 makes it serializable; Task 4 pushes it). "store in a dedicated folder" → Task 2/3 (`event_snapshot_dir`, `{event_id}.json`). "cleanup on restart" → Task 2 Step 3 (wipe+recreate). FE delivery → Task 3 (`GET /v1/events/{id}/raw` on the FE's origin).
- **Field coverage:** your minimal list (event: id/description/resolutionSource/endDate/image/icon/negRisk/tags; market: id/question/resolutionSource/endDate/image/icon/description/outcomes/groupItemTitle/negRisk) are all modelled fields → faithfully re-serialized in camelCase; the flatten catch-all covers anything added later.
- **Type consistency:** mirror `put_event_raw(event_id, &Value)` ↔ API `PUT /v1/events/{id}/raw` (Bytes body) ↔ `GET …/raw` returns the stored bytes.
- **Security:** `snapshot_path` rejects non-alphanumeric ids (no path traversal); PUT is dev-mode-gated like the existing metadata push.
- **No placeholders:** all code exact; one implementer note (the `self.sybil_client` field name in `sync.rs`, matched from the existing `create_market` call).
