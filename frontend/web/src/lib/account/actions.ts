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
  createPasskeyForAccount,
  discoverPasskeyAccount,
  isWebAuthnAvailable,
} from "@/lib/auth/webauthn";
import {
  createApiKey,
  registerPasskey,
  revokeSigningKey,
  SettingsActionError,
} from "./settings";
import {
  clearKeyHandle,
  clearKeyHandleIfMatches,
  setKeyHandle,
  useAccountStore,
  type AccountSession,
} from "./store";
import {
  clearStoredAccount,
  clearStoredReadApiKey,
  readStoredAccount,
  readStoredAccountRevision,
  writeStoredAccount,
  type AccountAuthScheme,
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
      | "account_created_recovery"
      | "webauthn_unavailable"
      | "session_changed"
      | "unknown",
    public readonly createdAccountId?: number,
    public readonly createdPublicKeyHex?: string,
    public readonly createdAuthScheme?: AccountAuthScheme,
  ) {
    super(message);
    this.name = "AccountError";
  }
}

export type CreateAccountKeyMode = "passkey" | "local_key";

type PendingCreatedAccount = {
  accountId: number;
  publicKeyHex: string;
  jwk: JsonWebKey;
  privateKey: CryptoKey;
  storageRevisionBeforeCheckpoint: string | null;
};

const pendingCreatedAccounts = new Map<number, PendingCreatedAccount>();
const MAX_BOOTSTRAP_REVOKE_ATTEMPTS = 3;

async function revokeOnboardingBootstrapKey(args: {
  accountId: number;
  publicKeyHex: string;
}): Promise<void> {
  for (let attempt = 0; attempt < MAX_BOOTSTRAP_REVOKE_ATTEMPTS; attempt += 1) {
    try {
      await revokeSigningKey({
        accountId: args.accountId,
        publicKeyHex: args.publicKeyHex,
        authScheme: "raw_p256",
        targetPubkeyHex: args.publicKeyHex,
        targetAuthScheme: "raw_p256",
      });
      return;
    } catch (error) {
      // A newly-created account's pending create event can enter a block after
      // revokeSigningKey reads its validity binding but before the signed POST.
      // Refetch and re-sign that expected optimistic-concurrency conflict only.
      const staleBinding =
        error instanceof SettingsActionError &&
        error.status === 409 &&
        error.message.includes("stale key-operation state binding");
      if (!staleBinding || attempt === MAX_BOOTSTRAP_REVOKE_ATTEMPTS - 1) {
        throw error;
      }
    }
  }
}

/**
 * Account creation always registers an initial key in the same request. The
 * passkey path uses a short-lived raw P-256 bootstrap key because WebAuthn's
 * discoverable user handle needs the server-assigned account id. The bootstrap
 * credential is persisted before any passkey prompt so a cancellation or
 * network failure cannot strand the newly-created account. The passkey only
 * replaces it after registration succeeds, then mints its own read token.
 *
 * Throws AccountError("dev_mode_off") if the server rejects step 1, so the
 * modal can show a "bridge deposits coming soon" message.
 */
export async function createDemoAccount(
  initialBalanceNanos: bigint,
  mode: CreateAccountKeyMode = isWebAuthnAvailable() ? "passkey" : "local_key",
): Promise<void> {
  if (mode === "passkey" && !isWebAuthnAvailable()) {
    throw new AccountError(
      "Passkeys are not available in this browser",
      "webauthn_unavailable",
    );
  }

  const bootstrap = await generateKeyPair();
  const bootstrapPublicKeyHex = await exportPublicKeyCompressedHex(
    bootstrap.publicKey,
  );
  const bootstrapJwk = await exportPrivateJwk(bootstrap.privateKey);
  let storageRevisionBeforeCreate: string | null;
  try {
    storageRevisionBeforeCreate = readStoredAccountRevision();
  } catch {
    throw new AccountError(
      "Browser storage is unavailable. Enable site storage before creating an account.",
      "unknown",
    );
  }
  const created = await api.POST("/v1/accounts", {
    body: {
      initial_balance_nanos: Number(initialBalanceNanos) as unknown as string,
      initial_key: {
        public_key_hex: bootstrapPublicKeyHex,
        auth_scheme: "raw_p256",
      },
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
  setKeyHandle(accountId, bootstrap.privateKey);
  const bootstrapAccount = {
    accountId,
    publicKeyHex: bootstrapPublicKeyHex,
    authScheme: "raw_p256" as const,
    jwk: bootstrapJwk,
  };
  pendingCreatedAccounts.set(accountId, {
    accountId,
    publicKeyHex: bootstrapPublicKeyHex,
    jwk: bootstrapJwk,
    privateKey: bootstrap.privateKey,
    storageRevisionBeforeCheckpoint: storageRevisionBeforeCreate,
  });
  let installedSession: AccountSession | null = null;
  let storageRevision: string | null | undefined;
  let initialCheckpointWritten = false;

  try {
    const revisionBeforeCheckpoint = readStoredAccountRevision();
    pendingCreatedAccounts.get(accountId)!.storageRevisionBeforeCheckpoint =
      revisionBeforeCheckpoint;
    if (revisionBeforeCheckpoint !== storageRevisionBeforeCreate) {
      throw new AccountError(
        `Account #${accountId} was created, but the saved identity changed while the request was in flight. Recover it explicitly instead of creating another account.`,
        "account_created_recovery",
        accountId,
        bootstrapPublicKeyHex,
        "raw_p256",
      );
    }
    writeStoredAccount(bootstrapAccount);
    initialCheckpointWritten = true;
    pendingCreatedAccounts.delete(accountId);
    storageRevision = readStoredAccountRevision();

    if (mode === "local_key") {
      const bootstrapReadKey = await createApiKey({
        accountId,
        publicKeyHex: bootstrapPublicKeyHex,
        authScheme: "raw_p256",
        label: "web session",
      });
      requireUnchangedStorageRevision(storageRevision);
      writeStoredAccount({
        ...bootstrapAccount,
        readApiKey: bootstrapReadKey.token,
      });
      installedSession = {
        accountId,
        publicKeyHex: bootstrapPublicKeyHex,
        authScheme: "raw_p256",
        readApiKey: bootstrapReadKey.token,
      };
      useAccountStore.getState().setSession(installedSession);
      useAccountStore.getState().setConnectModalOpen(false);
      return;
    }

    const passkey = await createPasskeyForAccount(accountId);
    requireUnchangedStorageRevision(storageRevision);
    await registerPasskey({
      accountId,
      publicKeyHex: bootstrapPublicKeyHex,
      authScheme: "raw_p256",
      passkey,
      label: "browser passkey",
    });
    requireUnchangedStorageRevision(storageRevision);
    const passkeyCheckpoint = {
      accountId,
      publicKeyHex: passkey.publicKeyHex,
      authScheme: "webauthn" as const,
      credentialIdB64url: passkey.credentialIdB64url,
    };
    writeStoredAccount(passkeyCheckpoint);
    storageRevision = readStoredAccountRevision();
    const passkeyReadKey = await createApiKey({
      accountId,
      publicKeyHex: passkey.publicKeyHex,
      authScheme: "webauthn",
      credentialIdB64url: passkey.credentialIdB64url,
      label: "web session",
    });
    requireUnchangedStorageRevision(storageRevision);
    const passkeyAccount = {
      ...passkeyCheckpoint,
      readApiKey: passkeyReadKey.token,
    };
    writeStoredAccount(passkeyAccount);
    storageRevision = readStoredAccountRevision();
    installedSession = passkeyAccount;
    useAccountStore.getState().setSession(installedSession);

    // Revoke only after the passkey is independently usable and durable. If
    // cleanup fails, the passkey session remains recoverable and the modal
    // offers a reconnect instead of allocating another funded account.
    await revokeOnboardingBootstrapKey({
      accountId,
      publicKeyHex: bootstrapPublicKeyHex,
    });
    requireUnchangedStorageRevision(storageRevision);
    clearKeyHandleIfMatches(accountId, bootstrap.privateKey);
    useAccountStore.getState().setConnectModalOpen(false);
  } catch (error) {
    const storageChanged =
      storageRevision !== undefined &&
      readStoredAccountRevision() !== storageRevision;
    if (
      storageChanged ||
      (error instanceof AccountError && error.kind === "session_changed")
    ) {
      clearKeyHandleIfMatches(accountId, bootstrap.privateKey);
      const current = useAccountStore.getState().session;
      if (
        installedSession &&
        current?.accountId === installedSession.accountId &&
        current.publicKeyHex === installedSession.publicKeyHex &&
        current.authScheme === installedSession.authScheme &&
        current.credentialIdB64url === installedSession.credentialIdB64url &&
        current.readApiKey === installedSession.readApiKey
      ) {
        useAccountStore.getState().setSession(null);
      }
      if (error instanceof AccountError && error.kind === "session_changed") {
        throw error;
      }
      throw new AccountError(
        "The saved account changed while account setup was in progress. Reopen Connect before continuing.",
        "session_changed",
      );
    }
    if (!initialCheckpointWritten) {
      if (
        error instanceof AccountError &&
        error.kind === "account_created_recovery"
      ) {
        throw error;
      }
      const detail =
        error instanceof Error ? ` ${error.message}` : " Setup did not finish.";
      throw new AccountError(
        `Account #${accountId} was created, but its browser checkpoint could not be saved.${detail} Keep this page open, restore browser storage, and retry recovery instead of creating another account.`,
        "account_created_recovery",
        accountId,
        bootstrapPublicKeyHex,
        "raw_p256",
      );
    }
    let stored: ReturnType<typeof readStoredAccount> = null;
    try {
      stored = readStoredAccount();
    } catch {
      stored = null;
    }
    if (stored?.accountId === accountId) {
      if (stored.authScheme === "webauthn") {
        clearKeyHandleIfMatches(accountId, bootstrap.privateKey);
        const current = useAccountStore.getState().session;
        if (
          current?.accountId === accountId &&
          current.publicKeyHex === bootstrapPublicKeyHex &&
          current.authScheme !== "webauthn"
        ) {
          useAccountStore.getState().setSession(null);
        }
      }
      const credential =
        stored.authScheme === "webauthn" ? "passkey" : "local browser key";
      const detail =
        error instanceof Error ? ` ${error.message}` : " Setup did not finish.";
      throw new AccountError(
        `Account #${accountId} was created and saved with its ${credential}.${detail} Reconnect the saved account instead of creating another one.`,
        "account_created_recovery",
        accountId,
        stored.publicKeyHex,
        stored.authScheme,
      );
    }
    const detail =
      error instanceof Error ? ` ${error.message}` : " Setup did not finish.";
    throw new AccountError(
      `Account #${accountId} was created, but its browser checkpoint could not be saved.${detail} Keep this page open, restore browser storage, and retry recovery instead of creating another account.`,
      "account_created_recovery",
      accountId,
      bootstrapPublicKeyHex,
      "raw_p256",
    );
  }
}

/** Recover a just-created account without issuing another account-creation
 * request. Normally this delegates to the durable Saved account path. If the
 * initial localStorage write failed, the bootstrap credential remains only in
 * memory long enough for the user to restore storage and retry safely. */
export async function recoverCreatedAccount(
  accountId: number,
  expectedPublicKeyHex: string,
  expectedAuthScheme: AccountAuthScheme,
): Promise<void> {
  const pending = pendingCreatedAccounts.get(accountId);
  if (pending) {
    if (
      pending.publicKeyHex !== expectedPublicKeyHex ||
      expectedAuthScheme !== "raw_p256" ||
      readStoredAccountRevision() !== pending.storageRevisionBeforeCheckpoint
    ) {
      throw new AccountError(
        `Recovery state for account #${accountId} changed in another tab. Reopen Connect before continuing.`,
        "session_changed",
      );
    }
    setKeyHandle(accountId, pending.privateKey);
    try {
      writeStoredAccount({
        accountId,
        publicKeyHex: pending.publicKeyHex,
        authScheme: "raw_p256",
        jwk: pending.jwk,
      });
    } catch {
      throw new AccountError(
        `Browser storage is still unavailable for account #${accountId}. Enable site storage and try recovery again.`,
        "account_created_recovery",
        accountId,
        pending.publicKeyHex,
        "raw_p256",
      );
    }
    pendingCreatedAccounts.delete(accountId);
    await signInWithStoredAccount();
    return;
  }

  const stored = readStoredAccount();
  if (
    stored?.accountId === accountId &&
    stored.publicKeyHex === expectedPublicKeyHex &&
    stored.authScheme === expectedAuthScheme
  ) {
    await signInWithStoredAccount();
    return;
  }
  if (stored) {
    throw new AccountError(
      `The saved identity no longer matches newly-created account #${accountId}. Reopen Connect before continuing.`,
      "session_changed",
    );
  }

  throw new AccountError(
    `Recovery for account #${accountId} is no longer available in this tab.`,
    "session_changed",
  );
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

  setKeyHandle(accountId, privateKey);
  let readKey;
  try {
    readKey = await createApiKey({
      accountId,
      publicKeyHex,
      authScheme: "raw_p256",
      label: "web session",
    });
  } catch (error) {
    clearKeyHandle(accountId);
    throw error;
  }
  writeStoredAccount({
    accountId,
    publicKeyHex,
    authScheme: "raw_p256",
    jwk,
    readApiKey: readKey.token,
  });
  useAccountStore.getState().setSession({
    accountId,
    publicKeyHex,
    authScheme: "raw_p256",
    readApiKey: readKey.token,
  });
  useAccountStore.getState().setConnectModalOpen(false);
}

export async function signInWithStoredPasskey(): Promise<void> {
  const storageRevision = readStoredAccountRevision();
  const stored = readStoredAccount();
  if (stored?.authScheme !== "webauthn" || !stored.credentialIdB64url) {
    throw new AccountError("No saved passkey account", "account_not_found");
  }
  let readKey;
  try {
    readKey = await createApiKey({
      accountId: stored.accountId,
      publicKeyHex: stored.publicKeyHex,
      authScheme: "webauthn",
      credentialIdB64url: stored.credentialIdB64url,
      label: "web session",
    });
  } catch (error) {
    requireUnchangedStorageRevision(storageRevision);
    if (
      error instanceof SettingsActionError &&
      (error.status === 401 || error.status === 404)
    ) {
      throw new AccountError(
        `Saved account #${stored.accountId} is not registered on this devnet. Its passkey was kept.`,
        "account_not_found",
      );
    }
    throw error;
  }
  requireUnchangedStorageRevision(storageRevision);
  openPasskeySession({
    accountId: stored.accountId,
    publicKeyHex: stored.publicKeyHex,
    authScheme: "webauthn",
    credentialIdB64url: stored.credentialIdB64url,
    readApiKey: readKey.token,
  });
}

/** Mint a new read session from whichever signing identity is already saved in
 * this browser. This is the recovery path for a revoked/stale read token; it
 * never replaces or deletes the underlying signing credential. */
export async function signInWithStoredAccount(): Promise<void> {
  const storageRevision = readStoredAccountRevision();
  const stored = readStoredAccount();
  if (!stored) {
    throw new AccountError(
      "No saved account in this browser",
      "account_not_found",
    );
  }
  if (stored.authScheme === "webauthn") {
    await signInWithStoredPasskey();
    return;
  }
  if (!stored.jwk) {
    throw new AccountError(
      "Saved local account is missing its private key",
      "invalid_jwk",
    );
  }

  let privateKey: CryptoKey;
  try {
    privateKey = await importPrivateKey(stored.jwk);
  } catch {
    throw new AccountError("Saved local account key is invalid", "invalid_jwk");
  }
  requireUnchangedStorageRevision(storageRevision);
  setKeyHandle(stored.accountId, privateKey);

  try {
    const readKey = await createApiKey({
      accountId: stored.accountId,
      publicKeyHex: stored.publicKeyHex,
      authScheme: "raw_p256",
      label: "web session",
    });
    const session = {
      accountId: stored.accountId,
      publicKeyHex: stored.publicKeyHex,
      authScheme: "raw_p256" as const,
      readApiKey: readKey.token,
    };
    requireUnchangedStorageRevision(storageRevision);
    writeStoredAccount({ ...stored, readApiKey: readKey.token });
    useAccountStore.getState().setSession(session);
    useAccountStore.getState().setConnectModalOpen(false);
  } catch (error) {
    clearKeyHandleIfMatches(stored.accountId, privateKey);
    requireUnchangedStorageRevision(storageRevision);
    if (
      error instanceof SettingsActionError &&
      (error.status === 401 || error.status === 404)
    ) {
      throw new AccountError(
        `Saved account #${stored.accountId} is not registered on this devnet. Its local signing credential was kept.`,
        "account_not_found",
      );
    }
    throw error;
  }
}

/**
 * Sign in without relying on localStorage. The authenticator returns the
 * account id as the discoverable credential's user handle; the API then
 * restores the registered public key needed by the local account session.
 */
export async function signInWithDiscoverablePasskey(): Promise<void> {
  const discovered = await discoverPasskeyAccount();
  let readKey;
  try {
    readKey = await createApiKey({
      accountId: discovered.accountId,
      authScheme: "webauthn",
      credentialIdB64url: discovered.credentialIdB64url,
      label: "web session",
    });
  } catch (error) {
    if (
      error instanceof SettingsActionError &&
      (error.status === 401 || error.status === 404)
    ) {
      throw new AccountError(
        `This passkey is not registered for account #${discovered.accountId}`,
        "account_not_found",
      );
    }
    throw new AccountError(
      `Could not sign in to account #${discovered.accountId}. Please try again.`,
      "network",
    );
  }

  openPasskeySession({
    accountId: discovered.accountId,
    publicKeyHex: readKey.signerPublicKeyHex,
    authScheme: "webauthn",
    credentialIdB64url: discovered.credentialIdB64url,
    readApiKey: readKey.token,
  });
}

export function disconnect(): void {
  const cur = useAccountStore.getState().session;
  const stored = readStoredAccount();
  const accountId = cur?.accountId ?? stored?.accountId;
  if (accountId !== undefined) clearKeyHandle(accountId);
  clearStoredAccount();
  useAccountStore.getState().setSession(null);
}

/** Drop a rejected read token while preserving the signing identity. The user
 * is sent to the reconnect flow, which can mint a replacement read token with
 * the saved local key or passkey. Explicit `disconnect()` remains destructive. */
export function invalidateReadSession(): void {
  const cur = useAccountStore.getState().session;
  if (cur) clearKeyHandle(cur.accountId);
  clearStoredReadApiKey();
  useAccountStore.getState().setSession(null);
  useAccountStore.getState().openReadAuthRecovery();
}

/** Re-hydrate from localStorage (called by AccountProvider on mount and on
 * `storage` events from other tabs). */
export async function rehydrateFromStorage(): Promise<void> {
  const storageRevision = readStoredAccountRevision();
  const stored = readStoredAccount();
  const current = useAccountStore.getState().session;
  if (!stored) {
    if (current) clearKeyHandle(current.accountId);
    useAccountStore.getState().setSession(null);
    if (useAccountStore.getState().connectModalRecovery) {
      useAccountStore.getState().setConnectModalOpen(false);
    }
    return;
  }
  if (!stored.readApiKey) {
    if (current) clearKeyHandle(current.accountId);
    useAccountStore.getState().setSession(null);
    return;
  }
  if (
    current &&
    current.accountId === stored.accountId &&
    current.readApiKey === stored.readApiKey
  )
    return;
  if (current && current.accountId !== stored.accountId) {
    clearKeyHandle(current.accountId);
  }
  try {
    if (stored.authScheme === "webauthn") {
      if (!stored.credentialIdB64url)
        throw new Error("missing WebAuthn credential id");
      useAccountStore.getState().setSession({
        accountId: stored.accountId,
        publicKeyHex: stored.publicKeyHex,
        authScheme: "webauthn",
        credentialIdB64url: stored.credentialIdB64url,
        readApiKey: stored.readApiKey,
      });
      useAccountStore.getState().setConnectModalOpen(false);
    } else {
      if (!stored.jwk) throw new Error("missing raw P256 private key");
      const privateKey = await importPrivateKey(stored.jwk);
      if (readStoredAccountRevision() !== storageRevision) return;
      setKeyHandle(stored.accountId, privateKey);
      useAccountStore.getState().setSession({
        accountId: stored.accountId,
        publicKeyHex: stored.publicKeyHex,
        authScheme: "raw_p256",
        readApiKey: stored.readApiKey,
      });
      useAccountStore.getState().setConnectModalOpen(false);
    }
  } catch (e) {
    if (readStoredAccountRevision() !== storageRevision) return;
    console.warn("[account] stored account corrupt; clearing", e);
    clearKeyHandle(stored.accountId);
    clearStoredAccount();
    useAccountStore.getState().setSession(null);
  }
}

function requireUnchangedStorageRevision(expected: string | null): void {
  if (readStoredAccountRevision() === expected) return;
  throw new AccountError(
    "The saved account changed in another tab. Reopen Connect and try again.",
    "session_changed",
  );
}

// --- helpers --------------------------------------------------------------

function openPasskeySession(account: StoredPasskeyAccount): void {
  writeStoredAccount(account);
  useAccountStore.getState().setSession(account);
  useAccountStore.getState().setConnectModalOpen(false);
}

type StoredPasskeyAccount = {
  accountId: number;
  publicKeyHex: string;
  authScheme: "webauthn";
  credentialIdB64url: string;
  readApiKey: string;
};

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
  if (last === undefined)
    throw new AccountError("malformed JWK", "invalid_jwk");
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
