//! P256 signing key used by the Polymarket-mirror resolution actor.
//!
//! The key is loaded from disk (SEC1 scalar hex) once at startup and used to
//! sign resolution attestations. The corresponding compressed SEC1 public key
//! must be pre-registered as a data feed on the sybil-api side (via
//! `--polymarket-feed-pubkey-hex`).

use std::fs;
use std::path::Path;

use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use sybil_api_types::SignedAttestationDto;
use sybil_signing::{
    MarketId as CanonicalMarketId, ResolutionAttestation as CanonicalAttestation,
    canonical_attestation_bytes,
};

#[derive(thiserror::Error, Debug)]
pub enum SignerError {
    #[error("read key file {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("decode hex: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("invalid P256 scalar")]
    InvalidKey,
}

pub struct ResolutionSigner {
    key: SigningKey,
    pubkey_hex: String,
    genesis_hash: [u8; 32],
}

impl ResolutionSigner {
    /// Load a signing key from a file containing the SEC1 scalar hex-encoded.
    /// If the file doesn't exist, generates one and writes it. This is a
    /// developer convenience; production deployments should pre-provision the
    /// key out-of-band.
    pub fn load_or_create(path: &Path, genesis_hash: [u8; 32]) -> Result<Self, SignerError> {
        use p256::elliptic_curve::rand_core::UnwrapErr;

        let key = if path.exists() {
            let hex_str = fs::read_to_string(path).map_err(|e| SignerError::Read {
                path: path.display().to_string(),
                source: e,
            })?;
            let bytes = hex::decode(hex_str.trim())?;
            SigningKey::from_slice(&bytes).map_err(|_| SignerError::InvalidKey)?
        } else {
            let key = <SigningKey as p256::elliptic_curve::Generate>::generate_from_rng(
                &mut UnwrapErr(getrandom::SysRng),
            );
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            fs::write(path, hex::encode(key.to_bytes())).map_err(|e| SignerError::Read {
                path: path.display().to_string(),
                source: e,
            })?;
            key
        };

        let vk = key.verifying_key();
        let pubkey_hex = hex::encode(vk.to_sec1_point(true).as_bytes());
        Ok(Self {
            key,
            pubkey_hex,
            genesis_hash,
        })
    }

    pub fn pubkey_hex(&self) -> &str {
        &self.pubkey_hex
    }

    /// Produce a [`SignedAttestationDto`] over `(market_id, payout_nanos, nonce)`.
    pub fn sign_attestation(
        &self,
        market_id: u32,
        payout_nanos: u64,
        nonce: u64,
    ) -> SignedAttestationDto {
        let att = CanonicalAttestation {
            market_id: CanonicalMarketId(market_id),
            payout_nanos,
            nonce,
        };
        let msg = canonical_attestation_bytes(&att, self.genesis_hash);
        let signature: Signature = self.key.sign(&msg);
        SignedAttestationDto {
            pubkey_hex: self.pubkey_hex.clone(),
            signature_hex: hex::encode(signature.to_der().as_bytes()),
            nonce,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signer_roundtrips_through_tempfile() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("sybil-test-signer-{}.key", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let signer_a = ResolutionSigner::load_or_create(&path, [0xab; 32]).unwrap();
        let signer_b = ResolutionSigner::load_or_create(&path, [0xab; 32]).unwrap();
        assert_eq!(signer_a.pubkey_hex(), signer_b.pubkey_hex());

        let att = signer_a.sign_attestation(3, 1_000_000_000, 42);
        assert_eq!(att.nonce, 42);
        assert!(!att.signature_hex.is_empty());

        let _ = std::fs::remove_file(&path);
    }
}
