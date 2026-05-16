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
  JWK: "sybil:auth:private_key_jwk",
} as const;

export interface StoredAccount {
  accountId: number;
  publicKeyHex: string;
  jwk: JsonWebKey;
}

export function readStoredAccount(): StoredAccount | null {
  if (typeof window === "undefined") return null;
  const idRaw = window.localStorage.getItem(STORAGE_KEYS.ACCOUNT_ID);
  const pubHex = window.localStorage.getItem(STORAGE_KEYS.PUBKEY);
  const jwkRaw = window.localStorage.getItem(STORAGE_KEYS.JWK);
  if (!idRaw || !pubHex || !jwkRaw) return null;
  try {
    const accountId = Number.parseInt(idRaw, 10);
    if (!Number.isFinite(accountId)) return null;
    const jwk = JSON.parse(jwkRaw) as JsonWebKey;
    return { accountId, publicKeyHex: pubHex, jwk };
  } catch {
    return null;
  }
}

export function writeStoredAccount(s: StoredAccount): void {
  window.localStorage.setItem(STORAGE_KEYS.ACCOUNT_ID, String(s.accountId));
  window.localStorage.setItem(STORAGE_KEYS.PUBKEY, s.publicKeyHex);
  window.localStorage.setItem(STORAGE_KEYS.JWK, JSON.stringify(s.jwk));
}

export function clearStoredAccount(): void {
  window.localStorage.removeItem(STORAGE_KEYS.ACCOUNT_ID);
  window.localStorage.removeItem(STORAGE_KEYS.PUBKEY);
  window.localStorage.removeItem(STORAGE_KEYS.JWK);
}
