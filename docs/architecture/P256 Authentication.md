---
tags: [infrastructure]
layer: api
crate: sybil-api
status: current
last_verified: 2026-07-03
---

Sybil uses P256 (NIST secp256r1) ECDSA signatures for authenticated account actions. This is the same elliptic curve used by hardware security modules (HSMs), secure enclaves (Apple's Secure Enclave, Android's StrongBox), and WebAuthn/FIDO2 keys. The choice of P256 over secp256k1 (Bitcoin/Ethereum's curve) is deliberate: it enables direct hardware key integration without software key management.

The authentication flow has two steps. First, the user registers a P256 public key via `POST /v1/accounts/{id}/keys`. This associates the key with their account — multiple keys can be registered for operational flexibility. Second, when submitting a signed action, the user signs the canonical payload with their private key. Signed orders go to `POST /v1/orders/signed`; signed bridge withdrawals go to `POST /v1/bridge/withdrawals/signed`. The API verifies the signature against the registered keys before forwarding the order to the [[Mempool]] or the withdrawal request to the bridge WAL.

Unsigned order submission (`POST /v1/orders`) is also available and is the primary path in dev mode. Production deployments would require all orders to be signed, ensuring that only the account holder can submit orders against their balance. The P256 choice also aligns with the [[ZK Integration Path]]: P256 signature verification has efficient implementations in SNARK circuits, enabling on-chain verification of order authenticity as part of the block proof.

Signed bridge withdrawals are scaffolding for [[L1 Settlement and Vault]] rather than the final L1 authorization story. The signature proves account intent and covers `account_id`, destination chain/vault, recipient, token, amount, and `expiry_height`; the signed route requires `expiry_height` so the server cannot inject an unsigned default. SYB-178/SYB-188 still need the proof-backed vault release path before signatures alone can be treated as complete withdrawal authorization.

## Key Properties
- P256 (secp256r1) ECDSA — same curve as hardware security modules
- Key registration: `POST /accounts/{id}/keys` — multiple keys per account
- Signed order submission: `POST /orders/signed` — signature verified against registered keys
- Signed withdrawal scaffold: `POST /bridge/withdrawals/signed` — signature verified against registered keys and service-gated
- Unsigned path available for dev mode
- Hardware-compatible: Secure Enclave, StrongBox, FIDO2 keys
- ZK-friendly: efficient P256 verification circuits exist

## Where This Lives
> `crates/sybil-api/src/routes/` — signed order and bridge-withdrawal endpoints
> `crates/matching-sequencer/src/crypto.rs` — canonical payload conversion and P256 verification

## See Also
- [[REST API]] — the endpoints for key registration and order submission
- [[ZK Integration Path]] — P256 verification in SNARK circuits
