---
tags: [api, attestation, security, tee]
layer: api
crate: sybil-api
status: current
last_verified: 2026-07-11
---

# Enclave attestation

`GET /v1/attestation` is currently a **development-only shape stub**. It is
mounted only when `SYBIL_DEV_MODE=true`; production returns `404` rather than a
payload that could be mistaken for evidence from a trusted execution
environment. The current server does not run in a Nitro Enclave and does not
produce or verify a Nitro attestation document.

The response is a stable JSON projection:

```json
{
  "pcr_values": {},
  "enclave_pubkey": "",
  "report_data": "",
  "signature": "",
  "is_stub": true
}
```

Empty PCR, key, report-data, and signature fields are intentional. They avoid
inventing measurements or a signature that resembles cryptographic evidence.
`is_stub: true` is the authoritative trust signal.

## Relationship to AWS Nitro

This JSON object is not the literal Nitro wire document. AWS specifies the
attestation document as a CBOR map inside a COSE_Sign1 object. Its signed CBOR
payload contains `module_id`, a millisecond `timestamp`, `digest` (`SHA384`), a
PCR map, the signing certificate, an ordered CA bundle, and optional
`public_key`, `user_data`, and `nonce`. The outer COSE_Sign1 object carries the
protected algorithm header, payload, and signature. See AWS's
[attestation document specification and validation flow](https://docs.aws.amazon.com/enclaves/latest/user/verify-root.html)
and [PCR definitions](https://docs.aws.amazon.com/enclaves/latest/user/set-up-attestation.html).

The development projection maps planned values as follows:

| Sybil JSON field | Nitro source | Encoding |
| --- | --- | --- |
| `pcr_values` | signed `pcrs` map | decimal index to lowercase hex bytes |
| `enclave_pubkey` | optional signed `public_key` | lowercase hex DER bytes |
| `report_data` | optional signed `user_data` | lowercase hex protocol bytes |
| `signature` | outer COSE_Sign1 signature | unpadded base64url |
| `is_stub` | Sybil transport metadata | not part of the Nitro document |

`report_data` is Sybil terminology, not an AWS field name. The protocol must
define exactly what it binds before a real attestation can authorize anything.

## Client trust boundary

The shared Rust client's `verify_attestation()` accepts this response only as
an explicit `StubAccepted` result and emits a warning that no Nitro signature,
certificate chain, or PCR was verified. It fails closed if a server returns
`is_stub: false`, because real verification is not implemented. A caller must
never turn `StubAccepted` into an authorization decision.

## Stub-to-real transition

The real endpoint cannot safely be implemented by filling these strings and
flipping `is_stub`. It must also provide the complete signed CBOR/COSE document
(or every equivalent certificate-chain input) and a verifier must:

1. decode CBOR and COSE_Sign1 without ambiguous encodings;
2. validate the AWS Nitro certificate path against the pinned AWS root;
3. verify the COSE signature over the exact payload;
4. check certificate and document time/freshness policy;
5. compare required PCRs with an operator-approved image policy;
6. check protocol bindings in `public_key`, `user_data`, and `nonce`; and
7. return a distinct trusted result only after every check succeeds.

Until that transition lands, this endpoint is useful only for exercising the
API and client control flow from day one.

## Where this lives

> `crates/sybil-api/src/routes/system.rs` — development stub handler  
> `crates/sybil-api/src/app.rs` — dev-only mount policy and OpenAPI registration  
> `crates/sybil-api-types/src/response.rs` — shared response DTO  
> `crates/sybil-client/src/client.rs` — explicit stub classification

## See also

- [[REST API]]
- [[Deployment Profiles]]
- [[Threat Model]]
