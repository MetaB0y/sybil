---
tags: [infrastructure]
layer: api
crate: sybil-api
status: current
last_verified: 2026-03-15
---

Sybil uses P256 (NIST secp256r1) ECDSA signatures for authenticated order submission. This is the same elliptic curve used by hardware security modules (HSMs), secure enclaves (Apple's Secure Enclave, Android's StrongBox), and WebAuthn/FIDO2 keys. The choice of P256 over secp256k1 (Bitcoin/Ethereum's curve) is deliberate: it enables direct hardware key integration without software key management.

The authentication flow has two steps. First, the user registers a P256 public key via `POST /v1/accounts/{id}/keys`. This associates the key with their account — multiple keys can be registered for operational flexibility. Second, when submitting an order, the user signs the order payload with their private key and submits it via `POST /v1/orders/signed`. The API verifies the signature against the registered keys before forwarding the order to the [[Mempool]].

Unsigned order submission (`POST /v1/orders`) is also available and is the primary path in dev mode. Production deployments would require all orders to be signed, ensuring that only the account holder can submit orders against their balance. The P256 choice also aligns with the [[ZK Integration Path]]: P256 signature verification has efficient implementations in SNARK circuits, enabling on-chain verification of order authenticity as part of the block proof.

## Key Properties
- P256 (secp256r1) ECDSA — same curve as hardware security modules
- Key registration: `POST /accounts/{id}/keys` — multiple keys per account
- Signed submission: `POST /orders/signed` — signature verified against registered keys
- Unsigned path available for dev mode
- Hardware-compatible: Secure Enclave, StrongBox, FIDO2 keys
- ZK-friendly: efficient P256 verification circuits exist

## Where This Lives
> `crates/sybil-api/src/routes/` — signed order endpoint and signature verification

## See Also
- [[REST API]] — the endpoints for key registration and order submission
- [[ZK Integration Path]] — P256 verification in SNARK circuits
