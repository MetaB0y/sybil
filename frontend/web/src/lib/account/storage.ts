"use client";

/**
 * localStorage I/O for the connected demo account.
 *
 * Keys are namespaced under `sybil:auth:`. The private key is stored as JWK
 * — extractable so the user can disconnect/reconnect across sessions. This is
 * the raw-P256 compatibility path; WebAuthn credentials use the browser's
 * authenticator instead.
 */

export const STORAGE_KEYS = {
  ACCOUNT_ID: "sybil:auth:account_id",
  PUBKEY: "sybil:auth:pubkey_hex",
  AUTH_SCHEME: "sybil:auth:auth_scheme",
  JWK: "sybil:auth:private_key_jwk",
  CREDENTIAL_ID: "sybil:auth:credential_id_b64url",
  READ_API_KEY: "sybil:auth:read_api_key",
  REVISION: "sybil:auth:revision",
} as const;

export type AccountAuthScheme = "raw_p256" | "webauthn";

export interface StoredAccount {
  accountId: number;
  publicKeyHex: string;
  authScheme: AccountAuthScheme;
  jwk?: JsonWebKey;
  credentialIdB64url?: string;
  /** Present only while the browser has an active read session. */
  readApiKey?: string;
}

export function readStoredAccount(): StoredAccount | null {
  if (typeof window === "undefined") return null;
  const idRaw = window.localStorage.getItem(STORAGE_KEYS.ACCOUNT_ID);
  const pubHex = window.localStorage.getItem(STORAGE_KEYS.PUBKEY);
  const authSchemeRaw = window.localStorage.getItem(STORAGE_KEYS.AUTH_SCHEME);
  const jwkRaw = window.localStorage.getItem(STORAGE_KEYS.JWK);
  const credentialIdB64url = window.localStorage.getItem(
    STORAGE_KEYS.CREDENTIAL_ID,
  );
  const readApiKey = window.localStorage.getItem(STORAGE_KEYS.READ_API_KEY);
  if (!idRaw || !pubHex) return null;
  try {
    const accountId = Number.parseInt(idRaw, 10);
    if (!Number.isFinite(accountId)) return null;
    const authScheme: AccountAuthScheme =
      authSchemeRaw === "webauthn" ? "webauthn" : "raw_p256";
    if (authScheme === "webauthn") {
      if (!credentialIdB64url) return null;
      return {
        accountId,
        publicKeyHex: pubHex,
        authScheme,
        credentialIdB64url,
        ...(readApiKey ? { readApiKey } : {}),
      };
    }
    if (!jwkRaw) return null;
    const jwk = JSON.parse(jwkRaw) as JsonWebKey;
    return {
      accountId,
      publicKeyHex: pubHex,
      authScheme,
      jwk,
      ...(readApiKey ? { readApiKey } : {}),
    };
  } catch {
    return null;
  }
}

export function writeStoredAccount(s: StoredAccount): void {
  if (s.authScheme === "webauthn" && !s.credentialIdB64url) {
    throw new Error("missing WebAuthn credential id");
  }
  if (s.authScheme === "raw_p256" && !s.jwk) {
    throw new Error("missing raw P256 private JWK");
  }
  window.localStorage.setItem(STORAGE_KEYS.ACCOUNT_ID, String(s.accountId));
  window.localStorage.setItem(STORAGE_KEYS.PUBKEY, s.publicKeyHex);
  window.localStorage.setItem(STORAGE_KEYS.AUTH_SCHEME, s.authScheme);
  if (s.readApiKey) {
    window.localStorage.setItem(STORAGE_KEYS.READ_API_KEY, s.readApiKey);
  } else {
    window.localStorage.removeItem(STORAGE_KEYS.READ_API_KEY);
  }
  if (s.authScheme === "webauthn") {
    window.localStorage.setItem(
      STORAGE_KEYS.CREDENTIAL_ID,
      s.credentialIdB64url!,
    );
    window.localStorage.removeItem(STORAGE_KEYS.JWK);
  } else {
    window.localStorage.setItem(STORAGE_KEYS.JWK, JSON.stringify(s.jwk));
    window.localStorage.removeItem(STORAGE_KEYS.CREDENTIAL_ID);
  }
  bumpStoredAccountRevision();
}

export function clearStoredAccount(): void {
  window.localStorage.removeItem(STORAGE_KEYS.ACCOUNT_ID);
  window.localStorage.removeItem(STORAGE_KEYS.PUBKEY);
  window.localStorage.removeItem(STORAGE_KEYS.AUTH_SCHEME);
  window.localStorage.removeItem(STORAGE_KEYS.JWK);
  window.localStorage.removeItem(STORAGE_KEYS.CREDENTIAL_ID);
  window.localStorage.removeItem(STORAGE_KEYS.READ_API_KEY);
  bumpStoredAccountRevision();
}

export function readStoredReadApiKey(): string | null {
  if (typeof window === "undefined") return null;
  return window.localStorage.getItem(STORAGE_KEYS.READ_API_KEY);
}

/** Invalidate only the revocable read session. Signing credentials remain so
 * the same account can mint a replacement token. */
export function clearStoredReadApiKey(): void {
  if (window.localStorage.getItem(STORAGE_KEYS.READ_API_KEY) === null) return;
  window.localStorage.removeItem(STORAGE_KEYS.READ_API_KEY);
  bumpStoredAccountRevision();
}

/** Opaque version used to reject late async auth/hydration completions after a
 * newer tab has changed or cleared the stored identity/session. */
export function readStoredAccountRevision(): string | null {
  if (typeof window === "undefined") return null;
  return window.localStorage.getItem(STORAGE_KEYS.REVISION);
}

function bumpStoredAccountRevision(): void {
  const revision =
    globalThis.crypto?.randomUUID?.() ??
    `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  window.localStorage.setItem(STORAGE_KEYS.REVISION, revision);
}
