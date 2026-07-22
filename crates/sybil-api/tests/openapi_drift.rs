//! OpenAPI drift pin. This test enumerates every mounted method/path pair across
//! the four declarative route registries and asserts each one is documented by the
//! OpenAPI-aware runtime registrations — and, in reverse, that the spec
//! documents nothing that is not mounted.
//!
//! Styled after the route_policy pin tests: the mount tables are the source of
//! truth, so any new route forces a matching OpenAPI annotation (or an explicit
//! allowlist entry) before this test goes green again.

use std::collections::BTreeSet;

use sybil_api::app::{
    DEV_ROUTE_TABLE, OWNER_ROUTE_TABLE, PUBLIC_ROUTE_TABLE, SERVICE_ROUTE_TABLE, openapi_document,
};

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

const EXPECTED_UNIT_FIELD_DESCRIPTIONS: usize = 138;
const EXPECTED_NANOS_WIRE_FIELDS: usize = 110;
const EXPECTED_NANOS_WIRE_PARAMETERS: usize = 3;

/// Registered method/path pairs, minus the non-API exemptions.
fn documented_route_mounts() -> BTreeSet<(String, String)> {
    PUBLIC_ROUTE_TABLE
        .iter()
        .chain(OWNER_ROUTE_TABLE)
        .chain(SERVICE_ROUTE_TABLE)
        .chain(DEV_ROUTE_TABLE)
        .filter(|mount| !OPENAPI_EXEMPT_PATHS.contains(&mount.path))
        .map(|mount| (mount.method.to_string(), mount.path.to_string()))
        .collect()
}

/// Method/path pairs present in the generated OpenAPI document.
fn openapi_route_mounts() -> BTreeSet<(String, String)> {
    let document = openapi_json();
    let paths = document["paths"].as_object().expect("OpenAPI paths object");
    let mut mounts = BTreeSet::new();
    for (path, item) in paths {
        let item = item.as_object().expect("OpenAPI path item");
        for method in ["get", "post", "put", "delete", "patch", "head", "options"] {
            if item.contains_key(method) {
                mounts.insert((method.to_ascii_uppercase(), path.clone()));
            }
        }
    }
    mounts
}

fn openapi_paths() -> BTreeSet<String> {
    openapi_route_mounts()
        .into_iter()
        .map(|(_, path)| path)
        .collect()
}

fn openapi_json() -> serde_json::Value {
    serde_json::to_value(openapi_document(true)).expect("serialize OpenAPI document")
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
    } else if field.ends_with("_nanos") {
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

fn nanos_schema_is_exact_decimal_string(schema: &serde_json::Value) -> bool {
    match schema.get("type") {
        Some(serde_json::Value::String(kind)) if kind == "string" => true,
        Some(serde_json::Value::String(kind)) if kind == "array" => schema
            .get("items")
            .is_some_and(nanos_schema_is_exact_decimal_string),
        Some(serde_json::Value::String(kind)) if kind == "object" => schema
            .get("additionalProperties")
            .is_some_and(nanos_schema_is_exact_decimal_string),
        Some(serde_json::Value::Array(kinds)) => {
            kinds.iter().any(|kind| kind == "string")
                && kinds.iter().all(|kind| kind == "string" || kind == "null")
        }
        _ => false,
    }
}

fn check_nanos_wire_schemas(
    value: &serde_json::Value,
    path: &str,
    invalid: &mut Vec<String>,
    covered: &mut usize,
) {
    let Some(object) = value.as_object() else {
        if let Some(array) = value.as_array() {
            for (index, child) in array.iter().enumerate() {
                check_nanos_wire_schemas(child, &format!("{path}[{index}]"), invalid, covered);
            }
        }
        return;
    };

    if let Some(properties) = object
        .get("properties")
        .and_then(serde_json::Value::as_object)
    {
        for (field, schema) in properties {
            if !field.ends_with("_nanos") {
                continue;
            }
            *covered += 1;
            if !nanos_schema_is_exact_decimal_string(schema) {
                invalid.push(format!("{path}.{field}: got {schema}"));
            }
        }
    }

    for (key, child) in object {
        check_nanos_wire_schemas(child, &format!("{path}.{key}"), invalid, covered);
    }
}

fn check_nanos_wire_parameters(
    value: &serde_json::Value,
    path: &str,
    invalid: &mut Vec<String>,
    covered: &mut usize,
) {
    let Some(object) = value.as_object() else {
        if let Some(array) = value.as_array() {
            for (index, child) in array.iter().enumerate() {
                check_nanos_wire_parameters(child, &format!("{path}[{index}]"), invalid, covered);
            }
        }
        return;
    };

    if object
        .get("in")
        .and_then(serde_json::Value::as_str)
        .is_some()
        && let Some(name) = object.get("name").and_then(serde_json::Value::as_str)
        && name.ends_with("_nanos")
    {
        *covered += 1;
        let exact = object
            .get("schema")
            .is_some_and(nanos_schema_is_exact_decimal_string);
        if !exact {
            invalid.push(format!("{path}.{name}: got {}", object["schema"]));
        }
    }

    for (key, child) in object {
        check_nanos_wire_parameters(child, &format!("{path}.{key}"), invalid, covered);
    }
}

#[test]
fn openapi_documents_every_mounted_route() {
    let mounted = documented_route_mounts();
    let documented = openapi_route_mounts();

    let missing_from_spec: Vec<&(String, String)> = mounted.difference(&documented).collect();
    let extra_in_spec: Vec<&(String, String)> = documented.difference(&mounted).collect();

    assert!(
        missing_from_spec.is_empty() && extra_in_spec.is_empty(),
        "OpenAPI spec drifted from the mounted route tables.\n  \
         mounted but undocumented (add a #[utoipa::path] + OpenApiRouter registration, \
         or allowlist as non-API): {missing_from_spec:?}\n  \
         documented but not mounted (remove the stale registration): {extra_in_spec:?}"
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
fn openapi_operations_keep_stable_sdk_tags() {
    let spec = openapi_json();
    let paths = spec
        .pointer("/paths")
        .and_then(serde_json::Value::as_object)
        .expect("OpenAPI paths object");
    let mut missing = Vec::new();

    for (path, item) in paths {
        let Some(operations) = item.as_object() else {
            continue;
        };
        for (method, operation) in operations {
            if !matches!(
                method.as_str(),
                "get" | "post" | "put" | "patch" | "delete" | "options" | "head" | "trace"
            ) {
                continue;
            }
            let tags = operation.get("tags").and_then(serde_json::Value::as_array);
            let stable = tags.is_some_and(|tags| {
                tags.len() == 1
                    && tags[0]
                        .as_str()
                        .is_some_and(|tag| tag.starts_with("routes"))
            });
            if !stable {
                missing.push(format!("{method} {path}: {:?}", operation.get("tags")));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "OpenAPI operations need one stable routes* tag so generated SDK module paths do not collapse into api/default:\n{}",
        missing.join("\n")
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

#[test]
fn openapi_nanos_fields_are_decimal_strings() {
    let spec = openapi_json();
    let schemas = spec
        .pointer("/components/schemas")
        .expect("OpenAPI components.schemas");
    let mut invalid = Vec::new();
    let mut covered_fields = 0;
    let mut covered_parameters = 0;

    check_nanos_wire_schemas(
        schemas,
        "components.schemas",
        &mut invalid,
        &mut covered_fields,
    );
    check_nanos_wire_parameters(
        &spec["paths"],
        "paths",
        &mut invalid,
        &mut covered_parameters,
    );

    assert!(
        invalid.is_empty(),
        "OpenAPI *_nanos fields and parameters must be exact decimal strings (including maps and nested arrays):\n{}",
        invalid.join("\n")
    );
    assert_eq!(
        covered_fields, EXPECTED_NANOS_WIRE_FIELDS,
        "OpenAPI *_nanos field count changed; keep the decimal-string contract and update the pin"
    );
    assert_eq!(
        covered_parameters, EXPECTED_NANOS_WIRE_PARAMETERS,
        "OpenAPI *_nanos parameter count changed; keep the decimal-string contract and update the pin"
    );
}

#[test]
fn openapi_operation_ids_are_complete_and_unique() {
    let spec = openapi_json();
    let mut operation_ids = BTreeSet::new();
    let mut operations = 0;

    for item in spec["paths"].as_object().expect("OpenAPI paths").values() {
        for (method, operation) in item.as_object().expect("OpenAPI path item") {
            if !matches!(
                method.as_str(),
                "get" | "post" | "put" | "patch" | "delete" | "options" | "head" | "trace"
            ) {
                continue;
            }
            operations += 1;
            let operation_id = operation["operationId"]
                .as_str()
                .expect("every operation needs an SDK-stable operationId");
            assert!(
                operation_ids.insert(operation_id.to_string()),
                "duplicate OpenAPI operationId {operation_id:?}"
            );
        }
    }

    assert_eq!(operation_ids.len(), operations);
}

#[test]
fn retained_block_response_documents_gone_status() {
    let spec = openapi_json();
    let response = spec
        .pointer("/paths/~1v1~1blocks~1{height}/get/responses/410")
        .expect("GET /v1/blocks/{height} must document retention expiry");
    assert_eq!(
        response.pointer("/content/application~1json/schema/$ref"),
        Some(&serde_json::Value::String(
            "#/components/schemas/ApiErrorResponse".to_string()
        ))
    );
}

#[test]
fn public_key_examples_are_valid_compressed_p256_points() {
    let spec = openapi_json();
    for schema in ["RegisterKeyRequest", "SignedRegisterKeyRequest"] {
        let pointer = format!("/components/schemas/{schema}/properties/public_key_hex/example");
        let example = spec
            .pointer(&pointer)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("{schema} needs a public-key example"));
        let bytes = hex::decode(example).expect("public-key example must be hex");
        p256::PublicKey::from_sec1_bytes(&bytes)
            .unwrap_or_else(|error| panic!("{schema} example is not a P256 point: {error}"));
    }
}
