use super::*;

/// Persisted management metadata for a signing key (SYB-60).
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct PubkeyMetaRow {
    #[serde(default)]
    pub(super) label: Option<String>,
    #[serde(default)]
    pub(super) scope: u8,
    #[serde(default)]
    pub(super) created_at_ms: u64,
}

pub(super) fn key_scope_to_store(scope: crate::crypto::KeyScope) -> u8 {
    match scope {
        crate::crypto::KeyScope::Primary => 0,
        crate::crypto::KeyScope::Agent => 1,
        crate::crypto::KeyScope::Custom => 2,
    }
}

pub(super) fn key_scope_from_store(value: u8) -> crate::crypto::KeyScope {
    match value {
        1 => crate::crypto::KeyScope::Agent,
        2 => crate::crypto::KeyScope::Custom,
        _ => crate::crypto::KeyScope::Primary,
    }
}

pub(super) fn account_auth_scheme_to_store(scheme: crate::crypto::AccountAuthScheme) -> u8 {
    match scheme {
        crate::crypto::AccountAuthScheme::RawP256 => 0,
        crate::crypto::AccountAuthScheme::WebAuthn => 1,
    }
}

pub(super) fn account_auth_scheme_from_store(value: u8) -> crate::crypto::AccountAuthScheme {
    match value {
        1 => crate::crypto::AccountAuthScheme::WebAuthn,
        _ => crate::crypto::AccountAuthScheme::RawP256,
    }
}

pub(super) fn parse_hash32(bytes: &[u8], context: &str) -> Result<[u8; 32], StoreError> {
    bytes.try_into().map_err(|_| {
        StoreError::CorruptLayout(format!("{context} must be 32 bytes, got {}", bytes.len()))
    })
}
