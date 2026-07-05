//! OpenAPI drift pin. The `ApiDoc` derive in `app.rs` is hand-maintained, so it
//! silently rots as routes come and go. This test enumerates every mounted route
//! template across the three declarative mount tables (the same tables pinned by
//! `route_policy.rs`) and asserts each one is documented in the generated OpenAPI
//! spec — and, in reverse, that the spec documents nothing that is not mounted.
//!
//! Styled after the route_policy pin tests: the mount tables are the source of
//! truth, so any new route forces a matching OpenAPI annotation (or an explicit
//! allowlist entry) before this test goes green again.

use std::collections::BTreeSet;

use sybil_api::app::{ApiDoc, DEV_ROUTE_TABLE, PUBLIC_ROUTE_TABLE, SERVICE_ROUTE_TABLE};
use utoipa::OpenApi;

/// Mounted route templates that are deliberately absent from the OpenAPI spec.
/// Each entry is a non-API surface: it does not speak the JSON request/response
/// contract that OpenAPI describes, so documenting it would be misleading.
const OPENAPI_EXEMPT_PATHS: &[&str] = &[
    // The OpenAPI document itself; describing it inside itself is circular.
    "/openapi.json",
    // Prometheus text-exposition scrape target, not JSON — served outside the
    // API contract for the metrics stack.
    "/metrics",
];

/// Unique path templates across all three mount tables, minus the non-API
/// exemptions. `MatchedPath`/utoipa both key on the path template (not the
/// method), so GET+PUT on the same path collapse to a single documented path.
fn documented_route_templates() -> BTreeSet<String> {
    PUBLIC_ROUTE_TABLE
        .iter()
        .chain(SERVICE_ROUTE_TABLE)
        .chain(DEV_ROUTE_TABLE)
        .map(|mount| mount.path)
        .filter(|path| !OPENAPI_EXEMPT_PATHS.contains(path))
        .map(str::to_string)
        .collect()
}

/// Path templates present in the generated OpenAPI document.
fn openapi_paths() -> BTreeSet<String> {
    ApiDoc::openapi().paths.paths.keys().cloned().collect()
}

#[test]
fn openapi_documents_every_mounted_route() {
    let mounted = documented_route_templates();
    let documented = openapi_paths();

    let missing_from_spec: Vec<&String> = mounted.difference(&documented).collect();
    let extra_in_spec: Vec<&String> = documented.difference(&mounted).collect();

    assert!(
        missing_from_spec.is_empty() && extra_in_spec.is_empty(),
        "OpenAPI spec drifted from the mounted route tables.\n  \
         mounted but undocumented (add a #[utoipa::path] + ApiDoc paths entry, \
         or allowlist as non-API): {missing_from_spec:?}\n  \
         documented but not mounted (remove the stale ApiDoc paths entry): {extra_in_spec:?}"
    );
}

/// The exemption allowlist must stay honest: every exempt path must actually be
/// mounted (else it is dead) and must never leak into the OpenAPI document.
#[test]
fn openapi_exemptions_are_mounted_and_undocumented() {
    let mounted_all: BTreeSet<&str> = PUBLIC_ROUTE_TABLE
        .iter()
        .chain(SERVICE_ROUTE_TABLE)
        .chain(DEV_ROUTE_TABLE)
        .map(|mount| mount.path)
        .collect();
    let documented = openapi_paths();

    for exempt in OPENAPI_EXEMPT_PATHS {
        assert!(
            mounted_all.contains(exempt),
            "exempt path {exempt} is not mounted; remove the stale allowlist entry"
        );
        assert!(
            !documented.contains(*exempt),
            "exempt path {exempt} is documented in OpenAPI; drop it from the allowlist"
        );
    }
}
