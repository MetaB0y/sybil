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

use sybil_api::app::{
    ApiDoc, DEV_ROUTE_TABLE, OWNER_ROUTE_TABLE, PUBLIC_ROUTE_TABLE, SERVICE_ROUTE_TABLE,
};
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

const EXPECTED_UNIT_FIELD_DESCRIPTIONS: usize = 137;

/// Unique path templates across all three mount tables, minus the non-API
/// exemptions. `MatchedPath`/utoipa both key on the path template (not the
/// method), so GET+PUT on the same path collapse to a single documented path.
fn documented_route_templates() -> BTreeSet<String> {
    PUBLIC_ROUTE_TABLE
        .iter()
        .chain(OWNER_ROUTE_TABLE)
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

fn openapi_json() -> serde_json::Value {
    serde_json::to_value(ApiDoc::openapi()).expect("serialize OpenAPI document")
}

fn expected_unit_phrase(field: &str) -> Option<&'static str> {
    if matches!(
        field,
        "quantity"
            | "max_fill"
            | "fill_qty"
            | "remaining_quantity"
            | "original_quantity"
            | "qty"
            | "delta"
    ) {
        Some("Integer share-units")
    } else if field.ends_with("_nanos")
        || matches!(
            field,
            "prices" | "min_yes_price" | "max_yes_price" | "min_volume"
        )
    {
        Some("Integer nanodollars")
    } else if matches!(
        field,
        "block_hash"
            | "state_root"
            | "genesis_hash"
            | "witness_root"
            | "payload_root"
            | "provider_refs_hash"
            | "da_commitment"
            | "public_input_hash"
    ) {
        Some("Hex-encoded 32-byte")
    } else {
        None
    }
}

fn should_describe_probability_range(field: &str) -> bool {
    field == "prices"
        || field.contains("price")
        || field == "payout_nanos"
        || field == "clearing_prices_nanos"
}

fn normalize_description(description: &str) -> String {
    description.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn check_schema_unit_descriptions(
    value: &serde_json::Value,
    path: &str,
    missing: &mut Vec<String>,
    covered: &mut usize,
) {
    let Some(object) = value.as_object() else {
        if let Some(array) = value.as_array() {
            for (index, child) in array.iter().enumerate() {
                check_schema_unit_descriptions(
                    child,
                    &format!("{path}[{index}]"),
                    missing,
                    covered,
                );
            }
        }
        return;
    };

    if let Some(properties) = object
        .get("properties")
        .and_then(serde_json::Value::as_object)
    {
        for (field, schema) in properties {
            let Some(expected) = expected_unit_phrase(field) else {
                continue;
            };

            let description = schema
                .get("description")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let normalized = normalize_description(description);

            let unit_ok = normalized.contains(expected);
            let probability_ok = !should_describe_probability_range(field)
                || normalized.contains("per-share probabilities in [0, 1e9]")
                || normalized.contains("per-share probabilities in `[0, 1e9]`");

            if unit_ok && probability_ok {
                *covered += 1;
            } else {
                missing.push(format!(
                    "{path}.{field}: expected {expected:?}{} in description, got {description:?}",
                    if should_describe_probability_range(field) {
                        " and per-share probability range"
                    } else {
                        ""
                    }
                ));
            }
        }
    }

    for (key, child) in object {
        check_schema_unit_descriptions(child, &format!("{path}.{key}"), missing, covered);
    }
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
        .chain(OWNER_ROUTE_TABLE)
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

#[test]
fn openapi_info_mentions_units_convention() {
    let spec = openapi_json();
    let description = spec
        .pointer("/info/description")
        .and_then(serde_json::Value::as_str)
        .expect("OpenAPI info.description");

    assert!(
        description.contains("integer share-units")
            && description.contains("integer nanodollars")
            && description.contains("docs/architecture/REST%20API.md#units"),
        "OpenAPI info.description must mention global unit conventions and link REST API units; got {description:?}"
    );
}

#[test]
fn openapi_unit_fields_have_unit_descriptions() {
    let spec = openapi_json();
    let schemas = spec
        .pointer("/components/schemas")
        .expect("OpenAPI components.schemas");

    let mut missing = Vec::new();
    let mut covered = 0;
    check_schema_unit_descriptions(schemas, "components.schemas", &mut missing, &mut covered);

    assert!(
        missing.is_empty(),
        "OpenAPI unit field descriptions are missing or incomplete:\n{}",
        missing.join("\n")
    );
    assert_eq!(
        covered, EXPECTED_UNIT_FIELD_DESCRIPTIONS,
        "OpenAPI unit-bearing field description count changed; update the pin if deliberate"
    );
}
