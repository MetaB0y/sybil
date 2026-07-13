use crate::account::{Account, AccountId};
use crate::crypto::{
    AccountAuthScheme, AuthenticatedApiKeyCreate, AuthenticatedApiKeyRevoke,
    AuthenticatedKeyRegistration, AuthenticatedKeyRevocation, AuthenticatedProfileUpdate,
    PublicKey, RegisteredPubkey, SignedApiKeyCreate, SignedApiKeyRevoke, SignedKeyRegistration,
    SignedKeyRevocation, SignedProfileUpdate,
};
use crate::error::SequencerError;

use super::super::SequencerMsg;
use super::SequencerHandle;

impl SequencerHandle {
    pub async fn register_pubkey(
        &self,
        account_id: AccountId,
        pubkey: PublicKey,
    ) -> Result<(), SequencerError> {
        self.register_pubkey_with_scheme(account_id, pubkey, AccountAuthScheme::RawP256)
            .await
    }

    pub async fn register_pubkey_with_scheme(
        &self,
        account_id: AccountId,
        pubkey: PublicKey,
        auth_scheme: AccountAuthScheme,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RegisterPubkey(account_id, pubkey, auth_scheme, reply))
            .await?
    }

    pub async fn lookup_registered_pubkey(
        &self,
        pubkey: PublicKey,
    ) -> Result<Option<RegisteredPubkey>, SequencerError> {
        self.read_query(move |state| state.sequencer.lookup_registered_pubkey(&pubkey))
            .await
    }

    /// Register a signing key with SYB-60 management metadata (label/scope).
    pub async fn register_pubkey_with_meta(
        &self,
        account_id: AccountId,
        pubkey: PublicKey,
        meta: RegisteredPubkey,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RegisterPubkeyWithMeta(account_id, pubkey, meta, reply))
            .await?
    }

    /// Register a NEW signing key from a raw-P256-signed request (SYB-229).
    pub async fn register_key_signed(
        &self,
        signed: SignedKeyRegistration,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RegisterKeySigned(signed, reply))
            .await?
    }

    /// Register a NEW signing key from a WebAuthn-authenticated request (SYB-229).
    pub async fn register_key_authenticated(
        &self,
        authenticated: AuthenticatedKeyRegistration,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RegisterKeyAuthenticated(authenticated, reply))
            .await?
    }

    /// List an account's registered signing keys with metadata (SYB-60).
    pub async fn signing_keys_for_account(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<(Vec<u8>, RegisteredPubkey)>, SequencerError> {
        self.read_query(move |state| state.sequencer.signing_keys_for_account(account_id))
            .await
    }

    /// Apply a raw-P256-signed profile update (SYB-60).
    pub async fn set_profile_signed(
        &self,
        signed: SignedProfileUpdate,
    ) -> Result<Account, SequencerError> {
        self.rpc(|reply| SequencerMsg::SetProfileSigned(signed, reply))
            .await?
    }

    /// Apply a WebAuthn-authenticated profile update (SYB-60).
    pub async fn set_profile_authenticated(
        &self,
        authenticated: AuthenticatedProfileUpdate,
    ) -> Result<Account, SequencerError> {
        self.rpc(|reply| SequencerMsg::SetProfileAuthenticated(authenticated, reply))
            .await?
    }

    /// Revoke a signing key from a raw-P256-signed request (SYB-60).
    pub async fn revoke_signing_key_signed(
        &self,
        signed: SignedKeyRevocation,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RevokeSigningKeySigned(signed, reply))
            .await?
    }

    /// Revoke a signing key from a WebAuthn-authenticated request (SYB-60).
    pub async fn revoke_signing_key_authenticated(
        &self,
        authenticated: AuthenticatedKeyRevocation,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RevokeSigningKeyAuthenticated(authenticated, reply))
            .await?
    }

    /// Create a read API key from a raw-P256-signed request (SYB-60).
    pub async fn create_api_key_signed(
        &self,
        signed: SignedApiKeyCreate,
    ) -> Result<u64, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateApiKeySigned(signed, reply))
            .await?
    }

    /// Create a read API key from a WebAuthn-authenticated request (SYB-60).
    pub async fn create_api_key_authenticated(
        &self,
        authenticated: AuthenticatedApiKeyCreate,
    ) -> Result<u64, SequencerError> {
        self.rpc(|reply| SequencerMsg::CreateApiKeyAuthenticated(authenticated, reply))
            .await?
    }

    /// Revoke a read API key from a raw-P256-signed request (SYB-60).
    pub async fn revoke_api_key_signed(
        &self,
        signed: SignedApiKeyRevoke,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RevokeApiKeySigned(signed, reply))
            .await?
    }

    /// Revoke a read API key from a WebAuthn-authenticated request (SYB-60).
    pub async fn revoke_api_key_authenticated(
        &self,
        authenticated: AuthenticatedApiKeyRevoke,
    ) -> Result<(), SequencerError> {
        self.rpc(|reply| SequencerMsg::RevokeApiKeyAuthenticated(authenticated, reply))
            .await?
    }

    /// List an account's read API keys (metadata only) (SYB-60).
    pub async fn api_keys_for_account(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<crate::account::ApiKeyRecord>, SequencerError> {
        self.read_query(move |state| state.sequencer.api_keys_for_account(account_id))
            .await
    }

    /// Resolve a bearer token hash to its owning account if the key is active
    /// (SYB-60). Read-only; used by the API bearer extractor.
    pub async fn lookup_api_key(
        &self,
        token_hash: [u8; 32],
    ) -> Result<Option<AccountId>, SequencerError> {
        self.read_query(move |state| state.sequencer.lookup_api_key(&token_hash))
            .await
    }

    /// One-time API authorization snapshot. Per-request bearer validation must
    /// use the API-owned copy rather than enqueueing a sequencer query.
    pub async fn active_api_key_owners(
        &self,
    ) -> Result<Vec<([u8; 32], AccountId)>, SequencerError> {
        self.read_query(|state| state.sequencer.active_api_key_owners())
            .await
    }
}
