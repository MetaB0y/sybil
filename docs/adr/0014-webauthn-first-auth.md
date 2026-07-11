---
adr: 0014
title: WebAuthn / passkeys as the primary auth, verified in-guest
status: Accepted
date: 2026-07-07
validity_critical: true
supersedes: []
superseded_by: []
---

# ADR-0014 — WebAuthn-first authentication (in-guest)

Reverses the ratification-packet **D0** recommendation (raw-P256-only v1, WebAuthn
deferred). Founder steer 2026-07-07: UX is paramount, passkeys are the best UX,
and "we use a zkVM, so this should be simple — what's the catch?"

## Context — the actual catch

The catch is **not** the cryptography. OpenVM's accelerated secp256r1 makes the
**P256 ECDSA verify itself cheap** in-guest ([ADR-0008](0008-in-guest-p256-openvm-ecc.md)).
The catch is the **WebAuthn envelope**: a passkey does not sign our message; it
signs `authenticatorData ‖ SHA-256(clientDataJSON)`, where `clientDataJSON` is a
browser-produced blob like
`{"type":"webauthn.get","challenge":"<base64url(our_hash)>","origin":…}`. So an
in-guest verify must: base64url-encode our challenge, confirm it appears in the
**actual** clientDataJSON bytes, `SHA-256` those bytes, concatenate with
`authenticatorData`, then ECDSA-verify. `SHA-256` is already a guest extension
(cheap); the only real friction is that clientDataJSON is **not byte-identical
across browsers/authenticators** (field order, origin, extra keys), so the guest
must extract the challenge from the real bytes rather than assume a template.
That is **fiddly parsing + a little extra witness data, not prohibitive cost** —
which is why deferring it was the wrong call.

The API already does exactly this verification host-side
(`crates/sybil-api/src/webauthn.rs`: checks `challenge ==
base64url(SHA-256(canonical_bytes))`, type, origin, then P256 over
`authenticatorData ‖ SHA-256(clientDataJSON)`), so the in-guest version is a port,
not new design.

## Decision

**Passkeys/WebAuthn are the primary authentication path, verified in-guest.** Raw
P256 is kept for programmatic / agent keys (see
[capability-mask-keys](https://github.com/MetaB0y/sybil/blob/main/design/capability-mask-keys.md)). The guest verifies a
WebAuthn assertion by replicating `webauthn.rs`: reconstruct `authenticatorData`,
`SHA-256(clientDataJSON)`, verify the embedded challenge equals
`base64url(SHA-256(canonical_bytes))`, check `type`/`origin`, then ECDSA-verify
with OpenVM secp256r1. The extra per-op witness is the `authenticatorData` +
`clientDataJSON` bytes.

**Consequence for escape:** because the guest can verify WebAuthn, passkey-only
users can sign escape claims directly — so the earlier "WebAuthn accounts need a
raw backup key" requirement is **dropped**.

## Alternatives considered

- **Raw-P256-only v1 (the deferred D0 recommendation).** Rejected — it forces
  users onto seed-phrase-style key management, the worst UX, for a modest saving
  in guest code. Wrong trade for a product that lives or dies on UX.
- **Assume a fixed clientDataJSON template in-guest.** Rejected — breaks across
  browsers/authenticators; the guest must parse the real bytes for compatibility.

## Consequences

**Good:** best-in-class onboarding (passkeys — no seed phrase, phishing-resistant,
browser-native, broad support); one auth story from login to escape; the API's
existing verifier is the reference implementation.

**Costs / constraints:** more guest code (a bounded clientDataJSON parse + an
extra SHA-256) and more witness data per WebAuthn op than a raw signature — real
but modest, and the SHA-256 is on an already-enabled cheap extension. Cross-browser
clientDataJSON variability must be handled by extracting-not-templating, and
tested against real authenticator outputs. Raw-P256 stays for agents/automation
where there's no browser.

**Follow-ups:** in-guest WebAuthn verify rides the same OpenVM P256 work as
[ADR-0008](0008-in-guest-p256-openvm-ecc.md) / SYB-225; keys_digest key records
carry `auth_scheme` (raw_p256 | webauthn) already.
