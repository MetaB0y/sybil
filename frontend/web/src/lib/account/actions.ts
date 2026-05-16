"use client";

/**
 * High-level account actions: create demo, import existing, disconnect.
 *
 * Each action mutates both localStorage (via `storage.ts`) and the Zustand
 * store (via `store.ts`). On success the connect modal is closed.
 */

import { api } from "@/lib/api/client";
import {
  exportPrivateJwk,
  exportPublicKeyCompressedHex,
  generateKeyPair,
  importPrivateKey,
} from "@/lib/auth/p256";
import {
  clearKeyHandle,
  setKeyHandle,
  useAccountStore,
} from "./store";
import {
  clearStoredAccount,
  readStoredAccount,
  writeStoredAccount,
} from "./storage";

export class AccountError extends Error {
  constructor(
    message: string,
    public readonly kind:
      | "dev_mode_off"
      | "network"
      | "invalid_jwk"
      | "account_not_found"
      | "key_register_failed"
      | "unknown",
  ) {
    super(message);
    this.name = "AccountError";
  }
}

/**
 * 1. POST /v1/accounts (dev-mode) with chosen initial balance.
 * 2. Generate P-256 keypair locally.
 * 3. POST /v1/accounts/{id}/keys with the compressed pubkey.
 * 4. Persist + open session.
 *
 * Throws AccountError("dev_mode_off") if the server rejects step 1, so the
 * modal can show a "bridge deposits coming soon" message.
 */
export async function createDemoAccount(
  initialBalanceNanos: bigint,
): Promise<void> {
  const kp = await generateKeyPair();
  const publicKeyHex = await exportPublicKeyCompressedHex(kp.publicKey);

  const created = await api.POST("/v1/accounts", {
    // Schema marks *_nanos as `string` (patch-bigints) but wire wants a
    // JSON number for u64 deserialization. Cast through unknown.
    body: {
      initial_balance_nanos: Number(
        initialBalanceNanos,
      ) as unknown as string,
    },
  });
  if (created.error || !created.data) {
    const status = created.response?.status;
    if (status === 403) {
      throw new AccountError(
        "Demo account creation is disabled on this server",
        "dev_mode_off",
      );
    }
    throw new AccountError(
      `create_account failed (HTTP ${status ?? "?"})`,
      "network",
    );
  }
  const accountId = created.data.account_id;

  const registered = await api.POST("/v1/accounts/{id}/keys", {
    params: { path: { id: accountId } },
    body: { public_key_hex: publicKeyHex },
  });
  if (registered.error) {
    throw new AccountError(
      `register_key failed (HTTP ${registered.response?.status ?? "?"})`,
      "key_register_failed",
    );
  }

  const jwk = await exportPrivateJwk(kp.privateKey);
  writeStoredAccount({ accountId, publicKeyHex, jwk });
  setKeyHandle(accountId, kp.privateKey);
  useAccountStore.getState().setSession({ accountId, publicKeyHex });
  useAccountStore.getState().setConnectModalOpen(false);
}

/**
 * Import a previously generated account by pasting its id + JWK. The
 * compressed public key is derived from the JWK's `x` and `y` fields, so
 * the user only needs to paste two values.
 *
 * We verify the account exists on the server before persisting. We do NOT
 * verify the JWK matches the registered pubkey on the server (no API for
 * that) — a wrong key will surface as "signature invalid" on the first
 * order submission, which is fine.
 */
export async function importExistingAccount(
  accountId: number,
  jwk: JsonWebKey,
): Promise<void> {
  let privateKey: CryptoKey;
  try {
    privateKey = await importPrivateKey(jwk);
  } catch {
    throw new AccountError("Invalid JWK", "invalid_jwk");
  }

  const publicKeyHex = pubHexFromJwk(jwk);

  const account = await api.GET("/v1/accounts/{id}", {
    params: { path: { id: accountId } },
  });
  if (account.error || !account.data) {
    const status = account.response?.status;
    throw new AccountError(
      `Account #${accountId} not found (HTTP ${status ?? "?"})`,
      "account_not_found",
    );
  }

  writeStoredAccount({ accountId, publicKeyHex, jwk });
  setKeyHandle(accountId, privateKey);
  useAccountStore.getState().setSession({ accountId, publicKeyHex });
  useAccountStore.getState().setConnectModalOpen(false);
}

export function disconnect(): void {
  const cur = useAccountStore.getState().session;
  if (cur) clearKeyHandle(cur.accountId);
  clearStoredAccount();
  useAccountStore.getState().setSession(null);
}

/** Re-hydrate from localStorage (called by AccountProvider on mount and on
 * `storage` events from other tabs). */
export async function rehydrateFromStorage(): Promise<void> {
  const stored = readStoredAccount();
  if (!stored) {
    useAccountStore.getState().setSession(null);
    return;
  }
  const current = useAccountStore.getState().session;
  if (current && current.accountId === stored.accountId) return;
  try {
    const privateKey = await importPrivateKey(stored.jwk);
    setKeyHandle(stored.accountId, privateKey);
    useAccountStore.getState().setSession({
      accountId: stored.accountId,
      publicKeyHex: stored.publicKeyHex,
    });
  } catch (e) {
    console.warn("[account] stored JWK corrupt; clearing", e);
    clearStoredAccount();
    useAccountStore.getState().setSession(null);
  }
}

// --- helpers --------------------------------------------------------------

function pubHexFromJwk(jwk: JsonWebKey): string {
  if (jwk.kty !== "EC" || !jwk.x || !jwk.y) {
    throw new AccountError(
      "JWK must be an EC key with x and y coordinates",
      "invalid_jwk",
    );
  }
  const x = base64UrlToBytes(jwk.x);
  const y = base64UrlToBytes(jwk.y);
  if (x.length !== 32 || y.length !== 32) {
    throw new AccountError(
      "JWK x and y must each be 32 bytes (P-256)",
      "invalid_jwk",
    );
  }
  const last = y[31];
  if (last === undefined) throw new AccountError("malformed JWK", "invalid_jwk");
  const prefix = (last & 1) === 0 ? 0x02 : 0x03;
  const out = new Uint8Array(33);
  out[0] = prefix;
  out.set(x, 1);
  return Array.from(out)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function base64UrlToBytes(s: string): Uint8Array {
  const pad = (4 - (s.length % 4)) % 4;
  const b64 = s.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat(pad);
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
