"use client";

import { toHex } from "./canonical";

export interface WebAuthnAssertionPayload {
  credential_id_b64url: string;
  authenticator_data_b64url: string;
  client_data_json_b64url: string;
  signature_b64url: string;
  user_handle_b64url?: string;
}

export interface CreatedPasskey {
  publicKeyHex: string;
  credentialIdB64url: string;
  attestationObjectB64url: string;
  clientDataJSONB64url: string;
}

export interface DiscoveredPasskey {
  accountId: number;
  credentialIdB64url: string;
}

export interface CreatePasskeyOptions {
  /**
   * Onboarding defaults to the local platform authenticator. Backup creation
   * uses `any` so the browser may choose a second device, security key, or
   * different passkey provider.
   */
  authenticatorAttachment?: AuthenticatorAttachment | "any";
}

export function isWebAuthnAvailable(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof navigator !== "undefined" &&
    "credentials" in navigator &&
    typeof window.PublicKeyCredential !== "undefined"
  );
}

export async function createPasskeyForAccount(
  accountId: number,
  options: CreatePasskeyOptions = {},
): Promise<CreatedPasskey> {
  if (!isWebAuthnAvailable()) {
    throw new Error("WebAuthn is not available in this browser");
  }

  const challenge = randomBytes(32);
  const rp: PublicKeyCredentialRpEntity = { name: "Sybil" };
  const configuredRpId = rpId();
  if (configuredRpId) rp.id = configuredRpId;
  const attachment = options.authenticatorAttachment ?? "platform";
  const credential = (await navigator.credentials.create({
    publicKey: {
      challenge: challenge as unknown as BufferSource,
      rp,
      user: {
        id: accountUserId(accountId) as unknown as BufferSource,
        name: `sybil-${accountId}`,
        displayName: `Sybil #${accountId}`,
      },
      pubKeyCredParams: [{ type: "public-key", alg: -7 }],
      authenticatorSelection: {
        ...(attachment === "any"
          ? {}
          : { authenticatorAttachment: attachment }),
        residentKey: "preferred",
        userVerification: "required",
      },
      attestation: "none",
      timeout: 60_000,
    },
  })) as PublicKeyCredential | null;

  if (!credential) throw new Error("Passkey creation was cancelled");
  const response = credential.response as AuthenticatorAttestationResponse;
  if (typeof response.getPublicKey !== "function") {
    throw new Error("Browser did not expose the passkey public key");
  }
  const spki = response.getPublicKey();
  if (!spki) throw new Error("Passkey public key is missing");
  const publicKeyHex = await publicKeyHexFromSpki(spki);

  return {
    publicKeyHex,
    credentialIdB64url: base64UrlEncode(new Uint8Array(credential.rawId)),
    attestationObjectB64url: base64UrlEncode(
      new Uint8Array(response.attestationObject),
    ),
    clientDataJSONB64url: base64UrlEncode(
      new Uint8Array(response.clientDataJSON),
    ),
  };
}

export async function signWebAuthnBytes(
  credentialIdB64url: string,
  canonicalBytes: Uint8Array,
): Promise<WebAuthnAssertionPayload> {
  if (!isWebAuthnAvailable()) {
    throw new Error("WebAuthn is not available in this browser");
  }

  const challenge = new Uint8Array(
    await crypto.subtle.digest(
      "SHA-256",
      canonicalBytes as unknown as BufferSource,
    ),
  );
  const publicKey: PublicKeyCredentialRequestOptions = {
    challenge: challenge as unknown as BufferSource,
    allowCredentials: [
      {
        type: "public-key",
        id: base64UrlDecode(credentialIdB64url) as unknown as BufferSource,
      },
    ],
    userVerification: "required",
    timeout: 60_000,
  };
  const configuredRpId = rpId();
  if (configuredRpId) publicKey.rpId = configuredRpId;
  const credential = (await navigator.credentials.get({
    publicKey,
  })) as PublicKeyCredential | null;

  if (!credential) throw new Error("Passkey signing was cancelled");
  const response = credential.response as AuthenticatorAssertionResponse;
  const payload: WebAuthnAssertionPayload = {
    credential_id_b64url: base64UrlEncode(new Uint8Array(credential.rawId)),
    authenticator_data_b64url: base64UrlEncode(
      new Uint8Array(response.authenticatorData),
    ),
    client_data_json_b64url: base64UrlEncode(
      new Uint8Array(response.clientDataJSON),
    ),
    signature_b64url: base64UrlEncode(new Uint8Array(response.signature)),
  };
  if (response.userHandle) {
    payload.user_handle_b64url = base64UrlEncode(
      new Uint8Array(response.userHandle),
    );
  }
  return payload;
}

export async function verifyStoredPasskey(
  credentialIdB64url: string,
): Promise<void> {
  if (!isWebAuthnAvailable()) {
    throw new Error("WebAuthn is not available in this browser");
  }
  const publicKey: PublicKeyCredentialRequestOptions = {
    challenge: randomBytes(32) as unknown as BufferSource,
    allowCredentials: [
      {
        type: "public-key",
        id: base64UrlDecode(credentialIdB64url) as unknown as BufferSource,
      },
    ],
    userVerification: "required",
    timeout: 60_000,
  };
  const configuredRpId = rpId();
  if (configuredRpId) publicKey.rpId = configuredRpId;
  const credential = await navigator.credentials.get({ publicKey });
  if (!credential) throw new Error("Passkey sign-in was cancelled");
}

/**
 * Ask the authenticator to choose a discoverable credential. Passkeys created
 * by Sybil store the account id as their 8-byte WebAuthn user handle, allowing
 * a signed-out browser to recover the account without localStorage.
 */
export async function discoverPasskeyAccount(): Promise<DiscoveredPasskey> {
  if (!isWebAuthnAvailable()) {
    throw new Error("WebAuthn is not available in this browser");
  }

  const publicKey: PublicKeyCredentialRequestOptions = {
    challenge: randomBytes(32) as unknown as BufferSource,
    allowCredentials: [],
    userVerification: "required",
    timeout: 60_000,
  };
  const configuredRpId = rpId();
  if (configuredRpId) publicKey.rpId = configuredRpId;

  const credential = (await navigator.credentials.get({
    publicKey,
  })) as PublicKeyCredential | null;
  if (!credential) throw new Error("Passkey sign-in was cancelled");

  const response = credential.response as AuthenticatorAssertionResponse;
  if (!response.userHandle) {
    throw new Error(
      "This passkey predates usernameless login; use the same browser it was created in",
    );
  }

  return {
    accountId: accountIdFromUserHandle(new Uint8Array(response.userHandle)),
    credentialIdB64url: base64UrlEncode(new Uint8Array(credential.rawId)),
  };
}

function rpId(): string | undefined {
  const value = process.env.NEXT_PUBLIC_WEBAUTHN_RP_ID?.trim();
  return value ? value : undefined;
}

function accountUserId(accountId: number): Uint8Array {
  const out = new Uint8Array(8);
  const view = new DataView(out.buffer);
  view.setBigUint64(0, BigInt(accountId), false);
  return out;
}

export function accountIdFromUserHandle(userHandle: Uint8Array): number {
  if (userHandle.length !== 8) {
    throw new Error("Passkey contains an invalid Sybil account id");
  }
  const accountId = new DataView(
    userHandle.buffer,
    userHandle.byteOffset,
    userHandle.byteLength,
  ).getBigUint64(0, false);
  if (accountId > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new Error("Passkey account id is too large for this browser");
  }
  return Number(accountId);
}

async function publicKeyHexFromSpki(spki: ArrayBuffer): Promise<string> {
  const key = await crypto.subtle.importKey(
    "spki",
    spki,
    { name: "ECDSA", namedCurve: "P-256" },
    true,
    ["verify"],
  );
  const raw = await crypto.subtle.exportKey("raw", key);
  return toHex(compressUncompressedP256(new Uint8Array(raw)));
}

function compressUncompressedP256(raw65: Uint8Array): Uint8Array {
  if (raw65.length !== 65 || raw65[0] !== 0x04) {
    throw new Error("expected 65-byte uncompressed P256 key");
  }
  const x = raw65.subarray(1, 33);
  const y = raw65.subarray(33, 65);
  const yLast = y[31];
  if (yLast === undefined) throw new Error("malformed P256 public key");
  const out = new Uint8Array(33);
  out[0] = (yLast & 1) === 0 ? 0x02 : 0x03;
  out.set(x, 1);
  return out;
}

function randomBytes(len: number): Uint8Array {
  const out = new Uint8Array(len);
  crypto.getRandomValues(out);
  return out;
}

export function base64UrlEncode(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary)
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/g, "");
}

export function base64UrlDecode(value: string): Uint8Array {
  const pad = (4 - (value.length % 4)) % 4;
  const b64 = value.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat(pad);
  const binary = atob(b64);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
  return out;
}
