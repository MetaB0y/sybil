//! Shared P-256/WebAuthn authorization verification for validity-critical actions.
//!
//! This module deliberately avoids `serde_json` and `base64` so the exact same
//! parser and message construction run natively and inside OpenVM.

use sha2::{Digest as _, Sha256};

use crate::{
    KeyOpAuth, KeyRecord, MAX_WEBAUTHN_AUTHENTICATOR_DATA_BYTES,
    MAX_WEBAUTHN_CLIENT_DATA_JSON_BYTES,
};

#[cfg(target_os = "zkvm")]
use openvm_p256::ecdsa::{Signature, VerifyingKey, signature::hazmat::PrehashVerifier};
#[cfg(not(target_os = "zkvm"))]
use p256::ecdsa::{Signature, VerifyingKey, signature::hazmat::PrehashVerifier};

/// The RP used by the currently pinned Sybil passkey deployment.
///
/// Changing this is a protocol migration: both the main and escape guests must
/// be rebuilt and repinned together, and the API RP configuration must match.
/// Deliberately the registrable domain, not the `app.` host that serves the UI.
/// A passkey minted under this RP stays valid if the app later moves to the
/// apex or to another subdomain; pinning the exact web host would make any such
/// move another guest repin and fresh genesis. The origin below still restricts
/// assertions to one exact browser origin.
pub const EXPECTED_WEBAUTHN_RP_ID: &str = "sybil.exchange";

/// Exact browser origin accepted by the guest for the pinned deployment.
/// This is deliberately stricter than matching only the RP ID hash: an
/// assertion minted by another origin under the same RP must not authorize a
/// Sybil action.
pub const EXPECTED_WEBAUTHN_ORIGIN: &str = "https://app.sybil.exchange";

/// `SHA256(EXPECTED_WEBAUTHN_RP_ID)`, pinned as bytes for guest use.
pub const EXPECTED_RP_ID_HASH: [u8; 32] = [
    0x0e, 0xa9, 0x94, 0x09, 0xb0, 0x92, 0x3a, 0x99, 0xb1, 0x5b, 0x3a, 0xc8, 0x2e, 0xb1, 0xbb, 0x06,
    0xde, 0xd0, 0x2f, 0xab, 0xfe, 0xcd, 0x5b, 0x7c, 0xc3, 0x0c, 0xda, 0x11, 0xbb, 0xd6, 0x0a, 0x6c,
];

const FLAG_UP: u8 = 0x01;
const FLAG_UV: u8 = 0x04;
const MAX_JSON_DEPTH: usize = 32;

/// Verify an authorization envelope under the current running key set.
///
/// Membership and the declared authentication scheme are checked before any
/// cryptography. An empty active set therefore always fails closed.
pub fn verify_keyop_auth<'a, I>(
    authorization: &KeyOpAuth,
    active_keys: I,
    canonical_bytes: &[u8],
) -> Result<(), String>
where
    I: IntoIterator<Item = &'a KeyRecord>,
{
    let signer = authorization.signer_pubkey();
    let scheme = authorization.signer_auth_scheme();
    if !active_keys
        .into_iter()
        .any(|key| key.pubkey_sec1 == *signer && key.auth_scheme == scheme)
    {
        return Err("key-op signer is not a current scheme-matching key".to_string());
    }

    match authorization {
        KeyOpAuth::RawP256 {
            signer_pubkey,
            signature,
        } => {
            let prehash: [u8; 32] = Sha256::digest(canonical_bytes).into();
            verify_p256_prehash(signer_pubkey, signature, &prehash)
        }
        KeyOpAuth::WebAuthn {
            signer_pubkey,
            authenticator_data,
            client_data_json,
            signature,
        } => {
            verify_webauthn_envelope(canonical_bytes, authenticator_data, client_data_json)?;
            let client_data_hash: [u8; 32] = Sha256::digest(client_data_json).into();
            let mut hasher = Sha256::new();
            hasher.update(authenticator_data);
            hasher.update(client_data_hash);
            let signed_message_prehash: [u8; 32] = hasher.finalize().into();
            verify_p256_prehash(signer_pubkey, signature, &signed_message_prehash)
        }
    }
}

fn verify_p256_prehash(
    signer_pubkey: &[u8; 33],
    signature: &[u8; 64],
    prehash: &[u8; 32],
) -> Result<(), String> {
    let key = VerifyingKey::from_sec1_bytes(signer_pubkey)
        .map_err(|_| "key-op signer is not a valid P-256 SEC1 point".to_string())?;
    let signature = Signature::from_slice(signature)
        .map_err(|_| "key-op signature is not a valid raw P-256 signature".to_string())?;
    key.verify_prehash(prehash, &signature)
        .map_err(|_| "key-op P-256 signature verification failed".to_string())
}

fn verify_webauthn_envelope(
    canonical_bytes: &[u8],
    authenticator_data: &[u8],
    client_data_json: &[u8],
) -> Result<(), String> {
    if authenticator_data.len() > MAX_WEBAUTHN_AUTHENTICATOR_DATA_BYTES {
        return Err("WebAuthn authenticator_data exceeds protocol cap".to_string());
    }
    if client_data_json.len() > MAX_WEBAUTHN_CLIENT_DATA_JSON_BYTES {
        return Err("WebAuthn client_data_json exceeds protocol cap".to_string());
    }
    if authenticator_data.len() < 37 {
        return Err("WebAuthn authenticator_data is shorter than 37 bytes".to_string());
    }
    if authenticator_data[..32] != EXPECTED_RP_ID_HASH {
        return Err("WebAuthn rpIdHash does not match the pinned RP".to_string());
    }
    let flags = authenticator_data[32];
    if flags & FLAG_UP == 0 {
        return Err("WebAuthn user-presence flag is missing".to_string());
    }
    if flags & FLAG_UV == 0 {
        return Err("WebAuthn user-verification flag is missing".to_string());
    }

    let fields = extract_client_data_fields(client_data_json)?;
    if fields.type_ != b"webauthn.get" {
        return Err("WebAuthn clientDataJSON type is not webauthn.get".to_string());
    }
    if fields.origin != EXPECTED_WEBAUTHN_ORIGIN.as_bytes() {
        return Err("WebAuthn clientDataJSON origin does not match the pinned origin".to_string());
    }
    if fields.cross_origin {
        return Err("cross-origin WebAuthn assertion rejected".to_string());
    }
    let digest: [u8; 32] = Sha256::digest(canonical_bytes).into();
    if fields.challenge.as_slice() != base64url_sha256(&digest) {
        return Err("WebAuthn clientDataJSON challenge mismatch".to_string());
    }
    Ok(())
}

struct ClientDataFields {
    type_: Vec<u8>,
    challenge: Vec<u8>,
    origin: Vec<u8>,
    cross_origin: bool,
}

/// Extract the security-critical top-level clientDataJSON members.
///
/// Member names and values are decoded according to RFC 8259, including
/// surrogate-pair handling. Duplicate `type` or `challenge` members are
/// rejected so differing JSON-library duplicate semantics cannot be abused.
fn extract_client_data_fields(bytes: &[u8]) -> Result<ClientDataFields, String> {
    let mut parser = JsonParser { bytes, offset: 0 };
    parser.skip_ws();
    parser.expect(b'{')?;
    parser.skip_ws();

    let mut type_ = None;
    let mut challenge = None;
    let mut origin = None;
    let mut cross_origin = None;
    if parser.take(b'}') {
        return Err("clientDataJSON is missing type and challenge".to_string());
    }
    loop {
        let key = parser.parse_string()?;
        parser.skip_ws();
        parser.expect(b':')?;
        parser.skip_ws();
        if key == b"type" {
            if type_.is_some() {
                return Err("clientDataJSON has duplicate type members".to_string());
            }
            type_ = Some(parser.parse_string()?);
        } else if key == b"challenge" {
            if challenge.is_some() {
                return Err("clientDataJSON has duplicate challenge members".to_string());
            }
            challenge = Some(parser.parse_string()?);
        } else if key == b"origin" {
            if origin.is_some() {
                return Err("clientDataJSON has duplicate origin members".to_string());
            }
            origin = Some(parser.parse_string()?);
        } else if key == b"crossOrigin" {
            if cross_origin.is_some() {
                return Err("clientDataJSON has duplicate crossOrigin members".to_string());
            }
            cross_origin = Some(parser.parse_bool()?);
        } else {
            parser.skip_value(0)?;
        }
        parser.skip_ws();
        if parser.take(b'}') {
            break;
        }
        parser.expect(b',')?;
        parser.skip_ws();
    }
    parser.skip_ws();
    if parser.offset != bytes.len() {
        return Err("clientDataJSON has trailing bytes".to_string());
    }

    Ok(ClientDataFields {
        type_: type_.ok_or_else(|| "clientDataJSON is missing type".to_string())?,
        challenge: challenge.ok_or_else(|| "clientDataJSON is missing challenge".to_string())?,
        origin: origin.ok_or_else(|| "clientDataJSON is missing origin".to_string())?,
        cross_origin: cross_origin.unwrap_or(false),
    })
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl JsonParser<'_> {
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.offset += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.offset).copied()
    }

    fn take(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.offset += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.take(byte) {
            Ok(())
        } else {
            Err(format!("invalid clientDataJSON at byte {}", self.offset))
        }
    }

    fn parse_string(&mut self) -> Result<Vec<u8>, String> {
        self.expect(b'"')?;
        let mut out = Vec::new();
        loop {
            let byte = self
                .peek()
                .ok_or_else(|| "unterminated clientDataJSON string".to_string())?;
            self.offset += 1;
            match byte {
                b'"' => {
                    core::str::from_utf8(&out)
                        .map_err(|_| "clientDataJSON string is not UTF-8".to_string())?;
                    return Ok(out);
                }
                b'\\' => self.parse_escape(&mut out)?,
                0x00..=0x1f => {
                    return Err("clientDataJSON string contains an unescaped control byte".into());
                }
                _ => out.push(byte),
            }
        }
    }

    fn parse_bool(&mut self) -> Result<bool, String> {
        match self.peek() {
            Some(b't') => {
                self.expect_literal(b"true")?;
                Ok(true)
            }
            Some(b'f') => {
                self.expect_literal(b"false")?;
                Ok(false)
            }
            _ => Err(format!(
                "invalid clientDataJSON boolean at byte {}",
                self.offset
            )),
        }
    }

    fn parse_escape(&mut self, out: &mut Vec<u8>) -> Result<(), String> {
        let escaped = self
            .peek()
            .ok_or_else(|| "unterminated clientDataJSON escape".to_string())?;
        self.offset += 1;
        match escaped {
            b'"' | b'\\' | b'/' => out.push(escaped),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'u' => {
                let first = self.parse_hex_quad()?;
                let scalar = if (0xd800..=0xdbff).contains(&first) {
                    self.expect(b'\\')?;
                    self.expect(b'u')?;
                    let second = self.parse_hex_quad()?;
                    if !(0xdc00..=0xdfff).contains(&second) {
                        return Err("clientDataJSON has an invalid Unicode surrogate pair".into());
                    }
                    0x1_0000 + (((first as u32 - 0xd800) << 10) | (second as u32 - 0xdc00))
                } else if (0xdc00..=0xdfff).contains(&first) {
                    return Err("clientDataJSON has an unpaired low surrogate".into());
                } else {
                    first as u32
                };
                let ch = char::from_u32(scalar)
                    .ok_or_else(|| "clientDataJSON has an invalid Unicode escape".to_string())?;
                let mut encoded = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
            }
            _ => return Err("clientDataJSON has an invalid string escape".to_string()),
        }
        Ok(())
    }

    fn parse_hex_quad(&mut self) -> Result<u16, String> {
        let mut value = 0u16;
        for _ in 0..4 {
            let byte = self
                .peek()
                .ok_or_else(|| "truncated clientDataJSON Unicode escape".to_string())?;
            self.offset += 1;
            let nibble = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return Err("clientDataJSON has a non-hex Unicode escape".to_string()),
            };
            value = (value << 4) | u16::from(nibble);
        }
        Ok(value)
    }

    fn skip_value(&mut self, depth: usize) -> Result<(), String> {
        if depth > MAX_JSON_DEPTH {
            return Err("clientDataJSON exceeds maximum nesting depth".to_string());
        }
        self.skip_ws();
        match self.peek() {
            Some(b'"') => self.parse_string().map(|_| ()),
            Some(b'{') => self.skip_object(depth + 1),
            Some(b'[') => self.skip_array(depth + 1),
            Some(b't') => self.expect_literal(b"true"),
            Some(b'f') => self.expect_literal(b"false"),
            Some(b'n') => self.expect_literal(b"null"),
            Some(b'-' | b'0'..=b'9') => self.skip_number(),
            _ => Err(format!(
                "invalid clientDataJSON value at byte {}",
                self.offset
            )),
        }
    }

    fn skip_object(&mut self, depth: usize) -> Result<(), String> {
        self.expect(b'{')?;
        self.skip_ws();
        if self.take(b'}') {
            return Ok(());
        }
        loop {
            self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            self.skip_value(depth)?;
            self.skip_ws();
            if self.take(b'}') {
                return Ok(());
            }
            self.expect(b',')?;
            self.skip_ws();
        }
    }

    fn skip_array(&mut self, depth: usize) -> Result<(), String> {
        self.expect(b'[')?;
        self.skip_ws();
        if self.take(b']') {
            return Ok(());
        }
        loop {
            self.skip_value(depth)?;
            self.skip_ws();
            if self.take(b']') {
                return Ok(());
            }
            self.expect(b',')?;
            self.skip_ws();
        }
    }

    fn expect_literal(&mut self, literal: &[u8]) -> Result<(), String> {
        let end = self.offset.saturating_add(literal.len());
        if self.bytes.get(self.offset..end) == Some(literal) {
            self.offset = end;
            Ok(())
        } else {
            Err(format!(
                "invalid clientDataJSON literal at byte {}",
                self.offset
            ))
        }
    }

    fn skip_number(&mut self) -> Result<(), String> {
        self.take(b'-');
        match self.peek() {
            Some(b'0') => self.offset += 1,
            Some(b'1'..=b'9') => {
                self.offset += 1;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.offset += 1;
                }
            }
            _ => return Err("invalid clientDataJSON number".to_string()),
        }
        if self.take(b'.') {
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err("invalid clientDataJSON fraction".to_string());
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.offset += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.offset += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.offset += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err("invalid clientDataJSON exponent".to_string());
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.offset += 1;
            }
        }
        Ok(())
    }
}

fn base64url_sha256(digest: &[u8; 32]) -> [u8; 43] {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = [0u8; 43];
    let mut input = 0;
    let mut output = 0;
    while input + 3 <= digest.len() {
        let chunk = (u32::from(digest[input]) << 16)
            | (u32::from(digest[input + 1]) << 8)
            | u32::from(digest[input + 2]);
        out[output] = TABLE[((chunk >> 18) & 0x3f) as usize];
        out[output + 1] = TABLE[((chunk >> 12) & 0x3f) as usize];
        out[output + 2] = TABLE[((chunk >> 6) & 0x3f) as usize];
        out[output + 3] = TABLE[(chunk & 0x3f) as usize];
        input += 3;
        output += 4;
    }
    let chunk = u32::from(digest[input]) << 16 | u32::from(digest[input + 1]) << 8;
    out[output] = TABLE[((chunk >> 18) & 0x3f) as usize];
    out[output + 1] = TABLE[((chunk >> 12) & 0x3f) as usize];
    out[output + 2] = TABLE[((chunk >> 6) & 0x3f) as usize];
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Signer as _;
    use p256::ecdsa::{Signature as HostSignature, SigningKey};

    fn raw_record(key: &SigningKey, scheme: u8) -> KeyRecord {
        let mut pubkey_sec1 = [0u8; 33];
        pubkey_sec1.copy_from_slice(key.verifying_key().to_sec1_point(true).as_bytes());
        KeyRecord {
            auth_scheme: scheme,
            pubkey_sec1,
            capability_mask: KeyRecord::FULL_CAPABILITY_MASK,
        }
    }

    #[test]
    fn shared_raw_p256_vector_verifies_and_tampering_fails() {
        let key = SigningKey::from_slice(&[0x31; 32]).unwrap();
        let record = raw_record(&key, 0);
        let canonical = b"sybil/keyop/register/v1 shared host-guest vector";
        let signature: HostSignature = key.sign(canonical);
        let auth = KeyOpAuth::RawP256 {
            signer_pubkey: record.pubkey_sec1,
            signature: signature.to_bytes().into(),
        };
        verify_keyop_auth(&auth, [&record], canonical).unwrap();
        assert!(verify_keyop_auth(&auth, [&record], b"tampered").is_err());
    }

    #[test]
    fn shared_webauthn_vector_verifies_and_tampering_fails() {
        let key = SigningKey::from_slice(&[0x32; 32]).unwrap();
        let record = raw_record(&key, 1);
        let canonical = b"sybil/keyop/revoke/v1 shared host-guest vector";
        let digest: [u8; 32] = Sha256::digest(canonical).into();
        let challenge = core::str::from_utf8(&base64url_sha256(&digest))
            .unwrap()
            .to_string();
        // Build from the origin constant, not `https://{RP_ID}`: the RP is the
        // registrable domain and the origin is the `app.` host, so they are no
        // longer the same string.
        let client_data_json = format!(
            "{{\"type\":\"webauthn.get\",\"challenge\":\"{challenge}\",\"origin\":\"{EXPECTED_WEBAUTHN_ORIGIN}\",\"crossOrigin\":false}}"
        )
        .into_bytes();
        let mut authenticator_data = EXPECTED_RP_ID_HASH.to_vec();
        authenticator_data.push(FLAG_UP | FLAG_UV);
        authenticator_data.extend_from_slice(&7u32.to_be_bytes());
        let client_hash: [u8; 32] = Sha256::digest(&client_data_json).into();
        let mut signed_message = authenticator_data.clone();
        signed_message.extend_from_slice(&client_hash);
        let signature: HostSignature = key.sign(&signed_message);
        let auth = KeyOpAuth::WebAuthn {
            signer_pubkey: record.pubkey_sec1,
            authenticator_data,
            client_data_json,
            signature: signature.to_bytes().into(),
        };
        verify_keyop_auth(&auth, [&record], canonical).unwrap();
        assert!(verify_keyop_auth(&auth, [&record], b"tampered").is_err());
    }

    #[test]
    fn pinned_rp_hash_matches_name() {
        assert_eq!(
            <[u8; 32]>::from(Sha256::digest(EXPECTED_WEBAUTHN_RP_ID)),
            EXPECTED_RP_ID_HASH
        );
    }

    #[test]
    fn browser_client_data_corpus_extracts_exact_fields() {
        let samples: &[&[u8]] = &[
            br#"{"type":"webauthn.get","challenge":"chrome-token","origin":"https://app.sybil.exchange","crossOrigin":false}"#,
            br#"{"type": "webauthn.get", "challenge": "safari-token", "origin": "https://app.sybil.exchange"}"#,
            br#"{"challenge":"firefox-token","origin":"https://app.sybil.exchange","type":"webauthn.get","crossOrigin":false,"tokenBinding":{"status":"supported"}}"#,
        ];
        for (sample, expected) in samples.iter().zip([
            b"chrome-token".as_slice(),
            b"safari-token".as_slice(),
            b"firefox-token".as_slice(),
        ]) {
            let fields = extract_client_data_fields(sample).unwrap();
            assert_eq!(fields.type_, b"webauthn.get");
            assert_eq!(fields.challenge, expected);
            assert_eq!(fields.origin, EXPECTED_WEBAUTHN_ORIGIN.as_bytes());
            assert!(!fields.cross_origin);
        }
    }

    #[test]
    fn challenge_and_member_names_decode_all_json_escape_classes() {
        let json = br#"{"t\u0079pe":"webauthn\u002eget","chall\u0065nge":"a\/b\\c\"d\b\f\n\r\t\u00e9\ud83d\ude00","ori\u0067in":"https:\/\/app.sybil.exchange","cross\u004frigin":false}"#;
        let fields = extract_client_data_fields(json).unwrap();
        assert_eq!(fields.type_, b"webauthn.get");
        assert_eq!(
            core::str::from_utf8(&fields.challenge).unwrap(),
            "a/b\\c\"d\u{8}\u{c}\n\r\té😀"
        );
        assert_eq!(fields.origin, EXPECTED_WEBAUTHN_ORIGIN.as_bytes());
        assert!(!fields.cross_origin);
    }

    #[test]
    fn webauthn_rejects_wrong_missing_or_cross_origin_client_data() {
        let canonical = b"origin-bound canonical action";
        let digest: [u8; 32] = Sha256::digest(canonical).into();
        let encoded_challenge = base64url_sha256(&digest);
        let challenge = core::str::from_utf8(&encoded_challenge).unwrap();
        let mut authenticator_data = EXPECTED_RP_ID_HASH.to_vec();
        authenticator_data.push(FLAG_UP | FLAG_UV);
        authenticator_data.extend_from_slice(&0u32.to_be_bytes());

        for client_data in [
            format!(
                "{{\"type\":\"webauthn.get\",\"challenge\":\"{challenge}\",\"origin\":\"https://evil.example\",\"crossOrigin\":false}}"
            ),
            format!(
                "{{\"type\":\"webauthn.get\",\"challenge\":\"{challenge}\",\"crossOrigin\":false}}"
            ),
            format!(
                "{{\"type\":\"webauthn.get\",\"challenge\":\"{challenge}\",\"origin\":\"{EXPECTED_WEBAUTHN_ORIGIN}\",\"crossOrigin\":true}}"
            ),
        ] {
            assert!(
                verify_webauthn_envelope(canonical, &authenticator_data, client_data.as_bytes())
                    .is_err(),
                "client data unexpectedly accepted: {client_data}"
            );
        }
    }

    #[test]
    fn adversarial_spoof_corpus_is_rejected_or_cannot_override_top_level_fields() {
        let rejected: &[&[u8]] = &[
            br#"{"type":"webauthn.get","challenge":"bad","challenge":"good"}"#,
            br#"{"type":"bad","type":"webauthn.get","challenge":"good"}"#,
            br#"{"type":"webauthn.get","challenge":"good","origin":"https://app.sybil.exchange","origin":"https://evil.example"}"#,
            br#"{"type":"webauthn.get","challenge":"good","origin":"https://app.sybil.exchange","crossOrigin":false,"crossOrigin":true}"#,
            br#"{"type":"webauthn.get","challenge":"bad","nested":{"challenge":"good"}}"#,
            br#"{"type":"webauthn.get","challenge":"bad\u0022,\u0022challenge\u0022:\u0022good"}"#,
            br#"{"type":"webauthn.get","challenge":"good\ud800"}"#,
            br#"{"type":"webauthn.get","challenge":"good\udc00"}"#,
            br#"{"type":"webauthn.get","challenge":"good"} trailing"#,
            br#"[{"type":"webauthn.get","challenge":"good"}]"#,
        ];
        for sample in rejected {
            match extract_client_data_fields(sample) {
                Err(_) => {}
                Ok(fields) => assert_ne!(fields.challenge, b"good"),
            }
        }
    }

    #[test]
    fn base64url_digest_matches_known_sha256_vector() {
        let digest: [u8; 32] = Sha256::digest(b"abc").into();
        assert_eq!(
            &base64url_sha256(&digest),
            b"ungWv48Bz-pBQUDeXa4iI7ADYaOWF3qctBD_YfIAFa0"
        );
    }
}
