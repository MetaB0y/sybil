"use client";

/**
 * localStorage I/O for the connected demo account.
 *
 * Keys are namespaced under `sybil:auth:`. The private key is stored as JWK
 * — extractable so the user can disconnect/reconnect across sessions. Move
 * to non-extractable IndexedDB once dev_mode is no longer the onboarding
 * path (see OPEN_QUESTIONS).
 */

export const STORAGE_KEYS = {
  ACCOUNT_ID: "sybil:auth:account_id",
  PUBKEY: "sybil:auth:pubkey_hex",
  AUTH_SCHEME: "sybil:auth:auth_scheme",
  JWK: "sybil:auth:private_key_jwk",
  CREDENTIAL_ID: "sybil:auth:credential_id_b64url",
} as const;

export type AccountAuthScheme = "raw_p256" | "webauthn";

export interface StoredAccount {
  accountId: number;
  publicKeyHex: string;
  authScheme: AccountAuthScheme;
  jwk?: JsonWebKey;
  credentialIdB64url?: string;
}

export function readStoredAccount(): StoredAccount | null {
  if (typeof window === "undefined") return null;
  const idRaw = window.localStorage.getItem(STORAGE_KEYS.ACCOUNT_ID);
  const pubHex = window.localStorage.getItem(STORAGE_KEYS.PUBKEY);
  const authSchemeRaw = window.localStorage.getItem(STORAGE_KEYS.AUTH_SCHEME);
  const jwkRaw = window.localStorage.getItem(STORAGE_KEYS.JWK);
  const credentialIdB64url = window.localStorage.getItem(STORAGE_KEYS.CREDENTIAL_ID);
  if (!idRaw || !pubHex) return null;
  try {
    const accountId = Number.parseInt(idRaw, 10);
    if (!Number.isFinite(accountId)) return null;
    const authScheme: AccountAuthScheme =
      authSchemeRaw === "webauthn" ? "webauthn" : "raw_p256";
    if (authScheme === "webauthn") {
      if (!credentialIdB64url) return null;
      return { accountId, publicKeyHex: pubHex, authScheme, credentialIdB64url };
    }
    if (!jwkRaw) return null;
    const jwk = JSON.parse(jwkRaw) as JsonWebKey;
    return { accountId, publicKeyHex: pubHex, authScheme, jwk };
  } catch {
    return null;
  }
}

export function writeStoredAccount(s: StoredAccount): void {
  window.localStorage.setItem(STORAGE_KEYS.ACCOUNT_ID, String(s.accountId));
  window.localStorage.setItem(STORAGE_KEYS.PUBKEY, s.publicKeyHex);
  window.localStorage.setItem(STORAGE_KEYS.AUTH_SCHEME, s.authScheme);
  if (s.authScheme === "webauthn") {
    if (!s.credentialIdB64url) throw new Error("missing WebAuthn credential id");
    window.localStorage.setItem(
      STORAGE_KEYS.CREDENTIAL_ID,
      s.credentialIdB64url,
    );
    window.localStorage.removeItem(STORAGE_KEYS.JWK);
  } else {
    if (!s.jwk) throw new Error("missing raw P256 private JWK");
    window.localStorage.setItem(STORAGE_KEYS.JWK, JSON.stringify(s.jwk));
    window.localStorage.removeItem(STORAGE_KEYS.CREDENTIAL_ID);
  }
}

export function clearStoredAccount(): void {
  window.localStorage.removeItem(STORAGE_KEYS.ACCOUNT_ID);
  window.localStorage.removeItem(STORAGE_KEYS.PUBKEY);
  window.localStorage.removeItem(STORAGE_KEYS.AUTH_SCHEME);
  window.localStorage.removeItem(STORAGE_KEYS.JWK);
  window.localStorage.removeItem(STORAGE_KEYS.CREDENTIAL_ID);
}
