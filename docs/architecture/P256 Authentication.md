---
tags: [infrastructure]
layer: api
crate: sybil-api
status: current
last_verified: 2026-07-06
---

Sybil uses P256 (NIST secp256r1) ECDSA signatures for authenticated account actions. This is the same elliptic curve used by hardware security modules (HSMs), secure enclaves (Apple's Secure Enclave, Android's StrongBox), and WebAuthn/FIDO2 keys. The choice of P256 over secp256k1 (Bitcoin/Ethereum's curve) is deliberate: it enables direct hardware key integration without software key management.

The authentication flow has two schemes. Raw P256 keys register a compressed
P256 public key via `POST /v1/accounts/{id}/keys` with `auth_scheme =
raw_p256` (the default for bots, SDKs, and arena clients). Passkey accounts use
the same endpoint with `auth_scheme = webauthn`; the client sends the WebAuthn
registration payload, the API extracts the COSE EC2 P256 public key, and the
registered public key is tagged as WebAuthn. Multiple keys can be registered
for operational flexibility and backup passkeys.

New self-service accounts register their first key in `POST /v1/accounts` via
the `initial_key` field. The legacy bare create and unsigned first-key bootstrap
forms are service-only. Browser passkey onboarding uses a short-lived raw P256
bootstrap key because the discoverable WebAuthn user handle contains the newly
allocated account id; it registers the passkey through the signed additional-key
flow and then revokes the bootstrap key.

Account reads use a separate read-scoped bearer. Passkey login signs the
existing API-key-creation canonical payload, receives a one-time bearer token,
and stores it with the browser session. Discoverable login may omit the signer
public key: the API verifies the assertion against the claimed account's active
WebAuthn keys and returns the matching public key without exposing the gated key
list.

For raw P256, clients sign the canonical payload directly. For WebAuthn, the
assertion challenge is `base64url(SHA-256(canonical_payload_bytes))`, where the
canonical payload is the same order, cancel, or withdrawal byte string used by
raw P256, including the replay nonce. The authenticator signs
`authenticatorData || SHA-256(clientDataJSON)`. The API verifies the P256
signature, `clientDataJSON.type`, challenge, origin, `rpIdHash`, user presence,
user verification, and the registered auth scheme before forwarding an
already-authenticated action into the sequencer.

Signed orders go to `POST /v1/orders/signed`; signed cancellations go to `POST
/v1/orders/cancel/signed`; signed bridge withdrawals go to
`POST /v1/bridge/withdrawals/signed`. The raw signed path still verifies inside
the sequencer exactly as before. The WebAuthn path verifies the envelope at the
API boundary, then uses the same registered-key lookup, durable nonce advance,
admission, cancellation, or bridge WAL machinery. The block witness and OpenVM
guest do not contain WebAuthn envelopes or raw signatures; they continue to
verify accepted orders, rejections, fills, events, and state transitions.

Every signed order, signed cancellation, and signed bridge withdrawal carries a per-account `nonce: u64` covered by the canonical P256 payload. The sequencer stores each account's highest accepted signed-action nonce and requires strict increase; gaps are allowed. Stale or duplicate nonces are rejected at the API boundary as `409 REPLAY_NONCE_STALE`. The nonce advance is durably logged before the signed action becomes live, so a process restart cannot reopen the replay window for an already acknowledged signed payload.

Unsigned order submission (`POST /v1/orders`) is also available and is the primary path in dev mode. Production deployments would require all orders to be signed, ensuring that only the account holder can submit orders against their balance. The P256 choice also aligns with the [[ZK Integration Path]]: P256 signature verification has efficient implementations in SNARK circuits, enabling on-chain verification of order authenticity as part of the block proof.

Signed bridge withdrawals are scaffolding for [[L1 Settlement and Vault]] rather than the final L1 authorization story. The signature proves account intent and covers `account_id`, destination chain/vault, recipient, token, amount, `expiry_height`, and `nonce`; the signed route requires `expiry_height` and `nonce` so the server cannot inject unsigned defaults. SYB-178/SYB-188 still need the proof-backed vault release path before signatures alone can be treated as complete withdrawal authorization.

## Key Properties
- P256 (secp256r1) ECDSA — same curve as hardware security modules
- Key registration: `POST /accounts/{id}/keys` — multiple keys per account
- Signed order submission: `POST /orders/signed` — signature verified against registered keys
- Signed cancellation: `POST /orders/cancel/signed` — signature verified against registered keys
- Signed withdrawal scaffold: `POST /bridge/withdrawals/signed` — signature verified against registered keys and service-gated
- Passkey support: WebAuthn assertions over the hash of the same canonical bytes
- Replay protection: per-account strictly increasing signed-action nonce, persisted through restart
- Unsigned path available for dev mode
- Hardware-compatible: Secure Enclave, StrongBox, FIDO2 keys
- ZK-friendly: efficient P256 verification circuits exist
- Recovery: register a second passkey while one existing account key still works; see `docs/passkey-recovery.md`

## Where This Lives
> `crates/sybil-api/src/routes/` — signed order and bridge-withdrawal endpoints
> `crates/sybil-api/src/webauthn.rs` — WebAuthn assertion and COSE EC2 registration checks
> `crates/matching-sequencer/src/crypto.rs` — canonical payload conversion, raw P256 verification, and auth-scheme tags

## See Also
- [[REST API]] — the endpoints for key registration and order submission
- [[ZK Integration Path]] — P256 verification in SNARK circuits
