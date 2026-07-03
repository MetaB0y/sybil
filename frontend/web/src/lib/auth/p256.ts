/**
 * WebCrypto P-256 ECDSA wrappers.
 *
 * Server expects:
 *  - public keys as SEC1-compressed 33 bytes (hex) — see
 *    `crates/sybil-api/src/routes/orders.rs:32-40` (Sec1Point::from_bytes).
 *  - signatures as raw r||s 64 bytes (hex) — see
 *    `crates/sybil-api/src/routes/orders.rs:42-47` (Signature::from_slice).
 *    WebCrypto ECDSA outputs raw r||s natively; DO NOT DER-encode.
 *
 * The matching browser-side implementation lives in this package.
 */

import { toHex } from "./canonical";

export async function generateKeyPair(): Promise<CryptoKeyPair> {
  return crypto.subtle.generateKey(
    { name: "ECDSA", namedCurve: "P-256" },
    true,
    ["sign", "verify"],
  );
}

export async function exportPublicKeyCompressedHex(
  publicKey: CryptoKey,
): Promise<string> {
  const raw = await crypto.subtle.exportKey("raw", publicKey);
  return toHex(compressUncompressedP256(new Uint8Array(raw)));
}

export async function exportPrivateJwk(
  privateKey: CryptoKey,
): Promise<JsonWebKey> {
  return crypto.subtle.exportKey("jwk", privateKey);
}

/** Re-import a previously exported JWK as a non-extractable signing key. */
export async function importPrivateKey(jwk: JsonWebKey): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    "jwk",
    jwk,
    { name: "ECDSA", namedCurve: "P-256" },
    false,
    ["sign"],
  );
}

export async function signBytes(
  privateKey: CryptoKey,
  bytes: Uint8Array,
): Promise<string> {
  // TS 5.7 narrowed BufferSource to exclude SharedArrayBuffer-backed views;
  // borsh's Uint8Array<ArrayBufferLike> doesn't satisfy that constraint
  // even though it's a plain ArrayBuffer at runtime. Cast through unknown.
  const sig = await crypto.subtle.sign(
    { name: "ECDSA", hash: "SHA-256" },
    privateKey,
    bytes as unknown as BufferSource,
  );
  return toHex(new Uint8Array(sig));
}

function compressUncompressedP256(raw65: Uint8Array): Uint8Array {
  if (raw65.length !== 65 || raw65[0] !== 0x04) {
    throw new Error(
      "expected 65-byte uncompressed P256 key starting with 0x04",
    );
  }
  const x = raw65.subarray(1, 33);
  const y = raw65.subarray(33, 65);
  const yLast = y[31];
  if (yLast === undefined) throw new Error("malformed P256 public key");
  const prefix = (yLast & 1) === 0 ? 0x02 : 0x03;
  const out = new Uint8Array(33);
  out[0] = prefix;
  out.set(x, 1);
  return out;
}
