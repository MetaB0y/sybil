use std::fmt;
use std::io::Cursor;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ciborium::value::{Integer, Value};
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use sybil_api_types::request::{WebAuthnAssertion, WebAuthnRegistration};

use crate::config::ApiConfig;

const FLAG_UP: u8 = 0x01;
const FLAG_UV: u8 = 0x04;
const FLAG_AT: u8 = 0x40;

#[derive(Clone, Debug)]
pub struct WebAuthnVerifierConfig {
    pub rp_id: String,
    pub origin: String,
    pub require_user_verification: bool,
    rp_id_hash: [u8; 32],
}

impl WebAuthnVerifierConfig {
    pub fn from_api_config(config: &ApiConfig) -> Self {
        let rp_id_hash = sha256(config.webauthn_rp_id.as_bytes());
        Self {
            rp_id: config.webauthn_rp_id.clone(),
            origin: config.webauthn_origin.clone(),
            require_user_verification: config.webauthn_require_uv,
            rp_id_hash,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebAuthnError {
    BadBase64(&'static str),
    BadJson(&'static str),
    BadCbor(&'static str),
    UnexpectedClientDataType,
    ChallengeMismatch,
    OriginMismatch,
    CrossOrigin,
    AuthenticatorDataTooShort,
    RpIdHashMismatch,
    UserPresenceRequired,
    UserVerificationRequired,
    AttestedCredentialDataMissing,
    UnsupportedCoseKey,
    PublicKeyMismatch,
    BadSignatureEncoding,
    SignatureInvalid,
}

impl fmt::Display for WebAuthnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WebAuthnError::BadBase64(field) => write!(f, "invalid base64url field {field}"),
            WebAuthnError::BadJson(field) => write!(f, "invalid JSON field {field}"),
            WebAuthnError::BadCbor(field) => write!(f, "invalid CBOR field {field}"),
            WebAuthnError::UnexpectedClientDataType => {
                write!(f, "unexpected WebAuthn clientDataJSON type")
            }
            WebAuthnError::ChallengeMismatch => write!(
                f,
                "WebAuthn challenge does not match canonical payload hash"
            ),
            WebAuthnError::OriginMismatch => write!(f, "WebAuthn origin mismatch"),
            WebAuthnError::CrossOrigin => write!(f, "cross-origin WebAuthn assertion rejected"),
            WebAuthnError::AuthenticatorDataTooShort => write!(f, "authenticatorData is too short"),
            WebAuthnError::RpIdHashMismatch => write!(f, "WebAuthn rpIdHash mismatch"),
            WebAuthnError::UserPresenceRequired => {
                write!(f, "WebAuthn user-presence flag is missing")
            }
            WebAuthnError::UserVerificationRequired => {
                write!(f, "WebAuthn user-verification flag is missing")
            }
            WebAuthnError::AttestedCredentialDataMissing => {
                write!(f, "WebAuthn attested credential data is missing")
            }
            WebAuthnError::UnsupportedCoseKey => write!(f, "unsupported WebAuthn COSE key"),
            WebAuthnError::PublicKeyMismatch => {
                write!(
                    f,
                    "WebAuthn registration public key does not match public_key_hex"
                )
            }
            WebAuthnError::BadSignatureEncoding => write!(f, "invalid WebAuthn signature encoding"),
            WebAuthnError::SignatureInvalid => write!(f, "invalid WebAuthn signature"),
        }
    }
}

#[derive(Deserialize)]
struct ClientData {
    #[serde(rename = "type")]
    type_: String,
    challenge: String,
    origin: String,
    #[serde(default, rename = "crossOrigin")]
    cross_origin: Option<bool>,
}

pub fn canonical_challenge_b64url(canonical_bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(sha256(canonical_bytes))
}

pub fn verify_assertion(
    config: &WebAuthnVerifierConfig,
    verifying_key: &VerifyingKey,
    canonical_bytes: &[u8],
    assertion: &WebAuthnAssertion,
) -> Result<(), WebAuthnError> {
    let authenticator_data = decode_b64url(
        "webauthn_assertion.authenticator_data_b64url",
        &assertion.authenticator_data_b64url,
    )?;
    verify_authenticator_data(config, &authenticator_data, false)?;

    let client_data_json = decode_b64url(
        "webauthn_assertion.client_data_json_b64url",
        &assertion.client_data_json_b64url,
    )?;
    let expected_challenge = canonical_challenge_b64url(canonical_bytes);
    verify_client_data(
        config,
        &client_data_json,
        "webauthn.get",
        Some(&expected_challenge),
    )?;

    let signature_bytes = decode_b64url(
        "webauthn_assertion.signature_b64url",
        &assertion.signature_b64url,
    )?;
    let signature =
        Signature::from_der(&signature_bytes).map_err(|_| WebAuthnError::BadSignatureEncoding)?;

    let client_data_hash = sha256(&client_data_json);
    let mut signed_message = Vec::with_capacity(
        authenticator_data
            .len()
            .saturating_add(client_data_hash.len()),
    );
    signed_message.extend_from_slice(&authenticator_data);
    signed_message.extend_from_slice(&client_data_hash);

    verifying_key
        .verify(&signed_message, &signature)
        .map_err(|_| WebAuthnError::SignatureInvalid)
}

pub fn key_op_authorization(
    verifying_key: &VerifyingKey,
    assertion: &WebAuthnAssertion,
) -> Result<matching_sequencer::KeyOpAuth, WebAuthnError> {
    let authenticator_data = decode_b64url(
        "webauthn_assertion.authenticator_data_b64url",
        &assertion.authenticator_data_b64url,
    )?;
    let client_data_json = decode_b64url(
        "webauthn_assertion.client_data_json_b64url",
        &assertion.client_data_json_b64url,
    )?;
    let signature_bytes = decode_b64url(
        "webauthn_assertion.signature_b64url",
        &assertion.signature_b64url,
    )?;
    let signature =
        Signature::from_der(&signature_bytes).map_err(|_| WebAuthnError::BadSignatureEncoding)?;
    let compressed = verifying_key.to_sec1_point(true);
    let mut signer_pubkey = [0u8; 33];
    signer_pubkey.copy_from_slice(compressed.as_bytes());
    Ok(matching_sequencer::KeyOpAuth::WebAuthn {
        signer_pubkey,
        authenticator_data,
        client_data_json,
        signature: signature.to_bytes().into(),
    })
}

/// Parse and validate a WebAuthn registration payload, returning the compressed
/// SEC1 P256 public key extracted from the attested COSE EC2 key.
pub fn public_key_from_registration(
    config: &WebAuthnVerifierConfig,
    registration: &WebAuthnRegistration,
) -> Result<Vec<u8>, WebAuthnError> {
    let client_data_json = decode_b64url(
        "webauthn_registration.client_data_json_b64url",
        &registration.client_data_json_b64url,
    )?;
    verify_client_data(config, &client_data_json, "webauthn.create", None)?;

    let attestation_object = decode_b64url(
        "webauthn_registration.attestation_object_b64url",
        &registration.attestation_object_b64url,
    )?;
    let auth_data = auth_data_from_attestation_object(&attestation_object)?;
    verify_authenticator_data(config, &auth_data, true)?;
    cose_key_from_attested_credential_data(&auth_data)
}

fn verify_client_data(
    config: &WebAuthnVerifierConfig,
    client_data_json: &[u8],
    expected_type: &str,
    expected_challenge: Option<&str>,
) -> Result<(), WebAuthnError> {
    let client_data: ClientData = serde_json::from_slice(client_data_json)
        .map_err(|_| WebAuthnError::BadJson("clientDataJSON"))?;
    if client_data.type_ != expected_type {
        return Err(WebAuthnError::UnexpectedClientDataType);
    }
    if let Some(expected_challenge) = expected_challenge {
        if client_data.challenge != expected_challenge {
            return Err(WebAuthnError::ChallengeMismatch);
        }
    }
    if client_data.origin != config.origin {
        return Err(WebAuthnError::OriginMismatch);
    }
    if client_data.cross_origin.unwrap_or(false) {
        return Err(WebAuthnError::CrossOrigin);
    }
    Ok(())
}

fn verify_authenticator_data(
    config: &WebAuthnVerifierConfig,
    auth_data: &[u8],
    require_attested_credential_data: bool,
) -> Result<(), WebAuthnError> {
    if auth_data.len() < 37 {
        return Err(WebAuthnError::AuthenticatorDataTooShort);
    }
    if auth_data[..32] != config.rp_id_hash {
        return Err(WebAuthnError::RpIdHashMismatch);
    }
    let flags = auth_data[32];
    if flags & FLAG_UP == 0 {
        return Err(WebAuthnError::UserPresenceRequired);
    }
    if config.require_user_verification && flags & FLAG_UV == 0 {
        return Err(WebAuthnError::UserVerificationRequired);
    }
    if require_attested_credential_data && flags & FLAG_AT == 0 {
        return Err(WebAuthnError::AttestedCredentialDataMissing);
    }
    Ok(())
}

fn auth_data_from_attestation_object(attestation_object: &[u8]) -> Result<Vec<u8>, WebAuthnError> {
    let value: Value = ciborium::de::from_reader(Cursor::new(attestation_object))
        .map_err(|_| WebAuthnError::BadCbor("attestationObject"))?;
    let Value::Map(entries) = value else {
        return Err(WebAuthnError::BadCbor("attestationObject"));
    };
    for (key, value) in entries {
        if matches!(key, Value::Text(text) if text == "authData") {
            if let Value::Bytes(bytes) = value {
                return Ok(bytes);
            }
            return Err(WebAuthnError::BadCbor("attestationObject.authData"));
        }
    }
    Err(WebAuthnError::BadCbor("attestationObject.authData"))
}

fn cose_key_from_attested_credential_data(auth_data: &[u8]) -> Result<Vec<u8>, WebAuthnError> {
    if auth_data.len() < 37 + 16 + 2 {
        return Err(WebAuthnError::AttestedCredentialDataMissing);
    }
    let credential_id_len = u16::from_be_bytes([auth_data[53], auth_data[54]]) as usize;
    let cose_start = 55usize
        .checked_add(credential_id_len)
        .ok_or(WebAuthnError::AttestedCredentialDataMissing)?;
    if cose_start >= auth_data.len() {
        return Err(WebAuthnError::AttestedCredentialDataMissing);
    }
    let cose_key: Value = ciborium::de::from_reader(Cursor::new(&auth_data[cose_start..]))
        .map_err(|_| WebAuthnError::BadCbor("credentialPublicKey"))?;
    compressed_p256_from_cose_key(&cose_key)
}

fn compressed_p256_from_cose_key(cose_key: &Value) -> Result<Vec<u8>, WebAuthnError> {
    let Value::Map(entries) = cose_key else {
        return Err(WebAuthnError::UnsupportedCoseKey);
    };

    let kty = cose_int(entries, 1)?;
    let alg = cose_int(entries, 3)?;
    let crv = cose_int(entries, -1)?;
    let x = cose_bytes(entries, -2)?;
    let y = cose_bytes(entries, -3)?;

    if kty != 2 || alg != -7 || crv != 1 || x.len() != 32 || y.len() != 32 {
        return Err(WebAuthnError::UnsupportedCoseKey);
    }

    let mut out = Vec::with_capacity(33);
    out.push(if y[31] & 1 == 0 { 0x02 } else { 0x03 });
    out.extend_from_slice(x);
    VerifyingKey::from_sec1_bytes(&out).map_err(|_| WebAuthnError::UnsupportedCoseKey)?;
    Ok(out)
}

fn cose_int(entries: &[(Value, Value)], key: i128) -> Result<i128, WebAuthnError> {
    for (entry_key, value) in entries {
        if value_int(entry_key) == Some(key) {
            return value_int(value).ok_or(WebAuthnError::UnsupportedCoseKey);
        }
    }
    Err(WebAuthnError::UnsupportedCoseKey)
}

fn cose_bytes(entries: &[(Value, Value)], key: i128) -> Result<&[u8], WebAuthnError> {
    for (entry_key, value) in entries {
        if value_int(entry_key) == Some(key) {
            if let Value::Bytes(bytes) = value {
                return Ok(bytes);
            }
            return Err(WebAuthnError::UnsupportedCoseKey);
        }
    }
    Err(WebAuthnError::UnsupportedCoseKey)
}

fn value_int(value: &Value) -> Option<i128> {
    let Value::Integer(integer) = value else {
        return None;
    };
    integer_to_i128(*integer)
}

fn integer_to_i128(integer: Integer) -> Option<i128> {
    Some(i128::from(integer))
}

fn decode_b64url(field: &'static str, value: &str) -> Result<Vec<u8>, WebAuthnError> {
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| WebAuthnError::BadBase64(field))
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::SigningKey;

    fn test_config() -> WebAuthnVerifierConfig {
        let rp_id = "localhost".to_string();
        WebAuthnVerifierConfig {
            rp_id_hash: sha256(rp_id.as_bytes()),
            rp_id,
            origin: "http://localhost:3000".to_string(),
            require_user_verification: true,
        }
    }

    fn b64(bytes: &[u8]) -> String {
        URL_SAFE_NO_PAD.encode(bytes)
    }

    fn fixed_key() -> SigningKey {
        SigningKey::from_slice(&[7u8; 32]).expect("fixed scalar")
    }

    fn assertion_for(
        key: &SigningKey,
        config: &WebAuthnVerifierConfig,
        canonical: &[u8],
    ) -> WebAuthnAssertion {
        let mut authenticator_data = Vec::new();
        authenticator_data.extend_from_slice(&config.rp_id_hash);
        authenticator_data.push(FLAG_UP | FLAG_UV);
        authenticator_data.extend_from_slice(&1u32.to_be_bytes());

        let client_data_json = serde_json::json!({
            "type": "webauthn.get",
            "challenge": canonical_challenge_b64url(canonical),
            "origin": config.origin,
            "crossOrigin": false,
        })
        .to_string()
        .into_bytes();

        let mut signed_message = authenticator_data.clone();
        signed_message.extend_from_slice(&sha256(&client_data_json));
        let signature: Signature = key.sign(&signed_message);

        WebAuthnAssertion {
            credential_id_b64url: b64(b"credential-1"),
            authenticator_data_b64url: b64(&authenticator_data),
            client_data_json_b64url: b64(&client_data_json),
            signature_b64url: b64(signature.to_der().as_bytes()),
            user_handle_b64url: None,
        }
    }

    #[test]
    fn verifies_fixed_assertion_over_canonical_hash_challenge() {
        let config = test_config();
        let key = fixed_key();
        let canonical = b"sybil canonical bytes with nonce";
        let assertion = assertion_for(&key, &config, canonical);

        verify_assertion(&config, key.verifying_key(), canonical, &assertion).unwrap();
    }

    #[test]
    fn rejects_assertion_when_canonical_bytes_change() {
        let config = test_config();
        let key = fixed_key();
        let assertion = assertion_for(&key, &config, b"canonical-A");

        assert_eq!(
            verify_assertion(&config, key.verifying_key(), b"canonical-B", &assertion),
            Err(WebAuthnError::ChallengeMismatch)
        );
    }

    #[test]
    fn rejects_assertion_when_rp_id_hash_changes() {
        let config = test_config();
        let key = fixed_key();
        let canonical = b"canonical";
        let mut assertion = assertion_for(&key, &config, canonical);
        let mut auth_data =
            decode_b64url("authenticator_data", &assertion.authenticator_data_b64url).unwrap();
        auth_data[0] ^= 0xff;
        assertion.authenticator_data_b64url = b64(&auth_data);

        assert_eq!(
            verify_assertion(&config, key.verifying_key(), canonical, &assertion),
            Err(WebAuthnError::RpIdHashMismatch)
        );
    }

    #[test]
    fn extracts_compressed_p256_key_from_attestation_cose_ec2() {
        let config = test_config();
        let key = fixed_key();
        let uncompressed = key.verifying_key().to_sec1_point(false);
        let uncompressed_bytes = uncompressed.as_bytes();
        let x = uncompressed_bytes[1..33].to_vec();
        let y = uncompressed_bytes[33..65].to_vec();
        let compressed = key.verifying_key().to_sec1_point(true).as_bytes().to_vec();

        let cose_key = Value::Map(vec![
            (Value::Integer(1.into()), Value::Integer(2.into())),
            (Value::Integer(3.into()), Value::Integer((-7).into())),
            (Value::Integer((-1).into()), Value::Integer(1.into())),
            (Value::Integer((-2).into()), Value::Bytes(x)),
            (Value::Integer((-3).into()), Value::Bytes(y)),
        ]);
        let mut cose_bytes = Vec::new();
        ciborium::ser::into_writer(&cose_key, &mut cose_bytes).unwrap();

        let mut auth_data = Vec::new();
        auth_data.extend_from_slice(&config.rp_id_hash);
        auth_data.push(FLAG_UP | FLAG_UV | FLAG_AT);
        auth_data.extend_from_slice(&1u32.to_be_bytes());
        auth_data.extend_from_slice(&[0x11; 16]);
        auth_data.extend_from_slice(&(12u16).to_be_bytes());
        auth_data.extend_from_slice(b"credential-1");
        auth_data.extend_from_slice(&cose_bytes);

        let attestation_object = Value::Map(vec![
            (
                Value::Text("fmt".to_string()),
                Value::Text("none".to_string()),
            ),
            (Value::Text("authData".to_string()), Value::Bytes(auth_data)),
            (Value::Text("attStmt".to_string()), Value::Map(vec![])),
        ]);
        let mut attestation_object_bytes = Vec::new();
        ciborium::ser::into_writer(&attestation_object, &mut attestation_object_bytes).unwrap();
        let client_data_json = serde_json::json!({
            "type": "webauthn.create",
            "challenge": "registration",
            "origin": config.origin,
            "crossOrigin": false,
        })
        .to_string()
        .into_bytes();

        let registration = WebAuthnRegistration {
            attestation_object_b64url: b64(&attestation_object_bytes),
            client_data_json_b64url: b64(&client_data_json),
        };

        assert_eq!(
            public_key_from_registration(&config, &registration).unwrap(),
            compressed
        );
    }
}
