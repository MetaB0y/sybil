import { describe, it, expect } from "vitest";
import { p256 } from "@noble/curves/nist.js";
import {
  generateKeyPair,
  exportPublicKeyCompressedHex,
  exportPrivateJwk,
  importPrivateKey,
  signBytes,
} from "../p256";
import { canonicalOrderBytes, fromHex } from "../canonical";

/**
 * These tests exercise the WebCrypto wrappers end-to-end and verify the
 * output with @noble/curves/nist (P-256) — an independent implementation —
 * to catch any drift between what we produce and what the Rust server
 * (using the `p256` crate) will accept.
 *
 * Noble v2 `p256.verify(sig, msg, pubkey)` hashes `msg` with SHA-256
 * internally, matching WebCrypto's ECDSA P-256 + SHA-256 default. So we
 * pass raw bytes, not a pre-computed digest.
 */
describe("p256 sign + verify roundtrip", () => {
  it("signs canonical bytes; noble/curves verifies", async () => {
    const kp = await generateKeyPair();
    const pubHex = await exportPublicKeyCompressedHex(kp.publicKey);

    const bytes = canonicalOrderBytes({
      marketIds: [7],
      payoffs: [1, 0],
      limitPriceNanos: 550_000_000n,
      maxFill: 10n,
    });

    const sigHex = await signBytes(kp.privateKey, bytes);
    // `lowS: false` — WebCrypto emits high-S signatures ~50% of the time;
    // the Rust `p256` crate accepts both, so noble must too for parity.
    const ok = p256.verify(fromHex(sigHex), bytes, fromHex(pubHex), {
      lowS: false,
    });
    expect(ok).toBe(true);
  });

  it("JWK export → import preserves signing", async () => {
    const kp = await generateKeyPair();
    const jwk = await exportPrivateJwk(kp.privateKey);
    const reimported = await importPrivateKey(jwk);

    const bytes = new TextEncoder().encode("hello sybil");
    const sigHex = await signBytes(reimported, bytes);

    const pubHex = await exportPublicKeyCompressedHex(kp.publicKey);
    // `lowS: false` — WebCrypto emits high-S signatures ~50% of the time;
    // the Rust `p256` crate accepts both, so noble must too for parity.
    const ok = p256.verify(fromHex(sigHex), bytes, fromHex(pubHex), {
      lowS: false,
    });
    expect(ok).toBe(true);
  });

  it("compressed public key is 33 bytes with 0x02 or 0x03 prefix", async () => {
    const kp = await generateKeyPair();
    const pubHex = await exportPublicKeyCompressedHex(kp.publicKey);
    expect(pubHex.length).toBe(66);
    const prefix = pubHex.slice(0, 2);
    expect(prefix === "02" || prefix === "03").toBe(true);
  });

  it("raw signature is exactly 64 bytes (r||s, not DER)", async () => {
    const kp = await generateKeyPair();
    const sigHex = await signBytes(
      kp.privateKey,
      new TextEncoder().encode("x"),
    );
    expect(sigHex.length).toBe(128);
  });
});
