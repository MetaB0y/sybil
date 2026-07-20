"use client";

/**
 * SYB-60 account-management actions: profile, signing keys, read API keys.
 *
 * Profile/read-key mutations use canonical borsh bytes plus a replay nonce.
 * Signing-key operations use verifier-owned bytes bound to current account
 * digests. Both sign with raw P256 or WebAuthn according to the session.
 *
 * SECURITY: read API keys (bearer tokens, `sybk_…`) are READ-ONLY and cannot
 * trade. Trade authority comes only from a registered P256 signing key — add an
 * agent trade key with `scope: "agent"`.
 */

import { api } from "@/lib/api/client";
import {
  canonicalApiKeyCreateBytes,
  canonicalApiKeyRevokeBytes,
  canonicalKeyRegistrationBytes,
  canonicalKeyRevocationBytes,
  canonicalProfileUpdateBytes,
  fromHex,
} from "@/lib/auth/canonical";
import {
  exportPrivateJwk,
  exportPublicKeyCompressedHex,
  generateKeyPair,
  signBytes,
} from "@/lib/auth/p256";
import { signWebAuthnBytes, type CreatedPasskey } from "@/lib/auth/webauthn";
import { getGenesisHashBytes } from "./orders";
import { getKeyHandle, useAccountStore } from "./store";
import type { AccountAuthScheme } from "./storage";

export interface SettingsSignerArgs {
  accountId: number;
  publicKeyHex: string;
  /** Defaults to a browser-local monotonic nonce. */
  nonce?: bigint;
  authScheme?: AccountAuthScheme;
  credentialIdB64url?: string;
}

// --- Profile --------------------------------------------------------------

export interface SetProfileArgs extends SettingsSignerArgs {
  /** New display name, or `null` to clear it. */
  displayName: string | null;
  /** New identicon seed, or `null` to clear it. */
  avatarSeed: string | null;
}

/**
 * POST /v1/accounts/{id}/profile — set or clear the opt-in profile (signed).
 * Pass `null` for a field to clear it.
 */
export async function setProfile(args: SetProfileArgs): Promise<void> {
  const nonce = args.nonce ?? nextReplayNonce(args.accountId);
  const genesisHash = await getGenesisHashBytes();
  const canonical = canonicalProfileUpdateBytes(
    BigInt(args.accountId),
    args.displayName,
    args.avatarSeed,
    nonce,
    genesisHash,
  );

  const body = {
    ...(args.displayName !== null ? { display_name: args.displayName } : {}),
    ...(args.avatarSeed !== null ? { avatar_seed: args.avatarSeed } : {}),
    signer_pubkey_hex: args.publicKeyHex,
    nonce: u64JsonNumber(nonce),
  };

  const res = await api.POST("/v1/accounts/{id}/profile", {
    params: { path: { id: args.accountId } },
    body: await attachSignature(args, body, canonical),
  });
  throwIfError(res, "set_profile");
}

// --- Signing keys ---------------------------------------------------------

export interface AddAgentKeyArgs extends SettingsSignerArgs {
  label?: string;
}

export interface AddAgentKeyResult {
  publicKeyHex: string;
  /** The private JWK — show ONCE, never persisted server-side. */
  jwk: JsonWebKey;
}

async function getKeyOpBinding(accountId: number): Promise<{
  keysDigestHex: string;
  eventsDigestHex: string;
}> {
  const { data, error } = await api.GET("/v1/accounts/{id}/keyop-state", {
    params: { path: { id: accountId } },
  });
  if (error || !data)
    throw new Error("failed to load key-operation signing state");
  return {
    keysDigestHex: data.keys_digest_hex,
    eventsDigestHex: data.events_digest_hex,
  };
}

/**
 * POST /v1/accounts/{id}/keys/register — register a NEW agent P256 signing key,
 * authorized by the session's existing key (SYB-229).
 *
 * The registration is SIGNED (raw P256 or WebAuthn, per the session scheme) over
 * canonical bytes domain-separated by the chain `genesis_hash` (SYB-224). Public
 * unsigned registration is gone; only the first key is bootstrapped over the
 * service tier during onboarding. The generated private JWK is returned so the
 * caller can display it exactly once; it is not stored anywhere by this function.
 */
export async function addAgentKey(
  args: AddAgentKeyArgs,
): Promise<AddAgentKeyResult> {
  const kp = await generateKeyPair();
  const publicKeyHex = await exportPublicKeyCompressedHex(kp.publicKey);
  const genesisHash = await getGenesisHashBytes();
  const binding = await getKeyOpBinding(args.accountId);
  // The new agent key is always raw P256; the SIGNER may be raw or WebAuthn.
  const canonical = canonicalKeyRegistrationBytes(
    BigInt(args.accountId),
    "raw_p256",
    fromHex(publicKeyHex),
    genesisHash,
    fromHex(binding.keysDigestHex),
    fromHex(binding.eventsDigestHex),
  );

  const body = {
    public_key_hex: publicKeyHex,
    auth_scheme: "raw_p256" as const,
    scope: "agent" as const,
    ...(args.label ? { label: args.label } : {}),
    signer_pubkey_hex: args.publicKeyHex,
    bound_keys_digest_hex: binding.keysDigestHex,
    bound_events_digest_hex: binding.eventsDigestHex,
  };

  const res = await api.POST("/v1/accounts/{id}/keys/register", {
    params: { path: { id: args.accountId } },
    body: await attachSignerAuth(args, body, canonical),
  });
  throwIfError(res, "register_agent_key");
  const jwk = await exportPrivateJwk(kp.privateKey);
  return { publicKeyHex, jwk };
}

export interface RegisterPasskeyArgs extends SettingsSignerArgs {
  passkey: CreatedPasskey;
  label?: string;
}

/** Register a newly-created passkey using an existing account signer. */
export async function registerPasskey(
  args: RegisterPasskeyArgs,
): Promise<void> {
  const genesisHash = await getGenesisHashBytesWithRetry();
  const binding = await getKeyOpBinding(args.accountId);
  const canonical = canonicalKeyRegistrationBytes(
    BigInt(args.accountId),
    "webauthn",
    fromHex(args.passkey.publicKeyHex),
    genesisHash,
    fromHex(binding.keysDigestHex),
    fromHex(binding.eventsDigestHex),
  );
  const body = {
    public_key_hex: args.passkey.publicKeyHex,
    auth_scheme: "webauthn" as const,
    credential_id_b64url: args.passkey.credentialIdB64url,
    webauthn_registration: {
      attestation_object_b64url: args.passkey.attestationObjectB64url,
      client_data_json_b64url: args.passkey.clientDataJSONB64url,
    },
    scope: "primary" as const,
    ...(args.label ? { label: args.label } : {}),
    signer_pubkey_hex: args.publicKeyHex,
    bound_keys_digest_hex: binding.keysDigestHex,
    bound_events_digest_hex: binding.eventsDigestHex,
  };
  const res = await api.POST("/v1/accounts/{id}/keys/register", {
    params: { path: { id: args.accountId } },
    body: await attachSignerAuth(args, body, canonical),
  });
  throwIfError(res, "register_passkey");
}

async function getGenesisHashBytesWithRetry(): Promise<Uint8Array> {
  let lastError: unknown;
  for (let attempt = 0; attempt < 30; attempt += 1) {
    try {
      return await getGenesisHashBytes();
    } catch (error) {
      lastError = error;
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
  }
  throw lastError instanceof Error
    ? lastError
    : new Error("genesis_hash is unavailable");
}

/**
 * Append the SIGNER's auth fields to a signed key-registration body. Unlike
 * `attachSignature`, the signer scheme lives in `signer_auth_scheme` (the plain
 * `auth_scheme` field describes the NEW key being registered).
 */
async function attachSignerAuth<T extends Record<string, unknown>>(
  args: SettingsSignerArgs,
  body: T,
  canonical: Uint8Array,
): Promise<T & Record<string, unknown>> {
  const auth = resolveAuthContext(args);
  if (auth.authScheme === "webauthn") {
    return {
      ...body,
      signer_auth_scheme: "webauthn" as const,
      webauthn_assertion: await signWebAuthnBytes(
        auth.credentialIdB64url,
        canonical,
      ),
    };
  }
  return {
    ...body,
    signer_auth_scheme: "raw_p256" as const,
    signature_hex: await signRawBytes(args.accountId, canonical),
  };
}

export interface RevokeSigningKeyArgs extends SettingsSignerArgs {
  /** Hex-encoded compressed P256 pubkey of the key to revoke. */
  targetPubkeyHex: string;
  /** Auth scheme committed in the target key record. */
  targetAuthScheme: AccountAuthScheme;
}

/**
 * POST /v1/accounts/{id}/keys/revoke — revoke a signing key (signed). The
 * backend refuses to revoke the last remaining key (HTTP 409); surface that
 * gracefully.
 */
export async function revokeSigningKey(
  args: RevokeSigningKeyArgs,
): Promise<void> {
  const genesisHash = await getGenesisHashBytes();
  const binding = await getKeyOpBinding(args.accountId);
  const canonical = canonicalKeyRevocationBytes(
    BigInt(args.accountId),
    fromHex(args.targetPubkeyHex),
    args.targetAuthScheme,
    genesisHash,
    fromHex(binding.keysDigestHex),
    fromHex(binding.eventsDigestHex),
  );

  const body = {
    target_pubkey_hex: args.targetPubkeyHex,
    signer_pubkey_hex: args.publicKeyHex,
    bound_keys_digest_hex: binding.keysDigestHex,
    bound_events_digest_hex: binding.eventsDigestHex,
  };

  const res = await api.POST("/v1/accounts/{id}/keys/revoke", {
    params: { path: { id: args.accountId } },
    body: await attachSignature(args, body, canonical),
  });
  throwIfError(res, "revoke_key");
}

// --- Read API keys --------------------------------------------------------

export interface CreateApiKeyArgs {
  accountId: number;
  publicKeyHex?: string;
  nonce?: bigint;
  authScheme?: AccountAuthScheme;
  credentialIdB64url?: string;
  label?: string;
}

export interface CreatedApiKey {
  id: number;
  /** The bearer token, format `sybk_<hex>`. Shown ONCE. */
  token: string;
  label?: string;
  createdAtMs: number;
  signerPublicKeyHex: string;
}

/**
 * POST /v1/accounts/{id}/api-keys — create a READ-ONLY bearer API key (signed).
 * Returns the plaintext token exactly once.
 */
export async function createApiKey(
  args: CreateApiKeyArgs,
): Promise<CreatedApiKey> {
  const nonce = args.nonce ?? nextReplayNonce(args.accountId);
  const genesisHash = await getGenesisHashBytes();
  const canonical = canonicalApiKeyCreateBytes(
    BigInt(args.accountId),
    args.label ?? null,
    nonce,
    genesisHash,
  );

  const unsignedBody = {
    ...(args.label ? { label: args.label } : {}),
    nonce: u64JsonNumber(nonce),
  };

  const auth = resolveAuthContext(args);
  const body =
    auth.authScheme === "webauthn" && !args.publicKeyHex
      ? {
          ...unsignedBody,
          auth_scheme: "webauthn" as const,
          webauthn_assertion: await signWebAuthnBytes(
            auth.credentialIdB64url,
            canonical,
          ),
        }
      : await attachSignature(
          {
            ...args,
            publicKeyHex:
              args.publicKeyHex ??
              (() => {
                throw new Error("Missing signer public key");
              })(),
          },
          { ...unsignedBody, signer_pubkey_hex: args.publicKeyHex },
          canonical,
        );

  const res = await api.POST("/v1/accounts/{id}/api-keys", {
    params: { path: { id: args.accountId } },
    body,
  });
  throwIfError(res, "create_api_key");
  const data = res.data!;
  return {
    id: Number(data.id),
    token: data.token,
    ...(data.label != null ? { label: data.label } : {}),
    createdAtMs: Number(data.created_at_ms),
    signerPublicKeyHex: data.signer_pubkey_hex,
  };
}

export interface RevokeApiKeyArgs extends SettingsSignerArgs {
  apiKeyId: number;
}

/** POST /v1/accounts/{id}/api-keys/revoke — revoke a read API key (signed). */
export async function revokeApiKey(args: RevokeApiKeyArgs): Promise<void> {
  const nonce = args.nonce ?? nextReplayNonce(args.accountId);
  const genesisHash = await getGenesisHashBytes();
  const canonical = canonicalApiKeyRevokeBytes(
    BigInt(args.accountId),
    BigInt(args.apiKeyId),
    nonce,
    genesisHash,
  );

  const body = {
    api_key_id: args.apiKeyId,
    signer_pubkey_hex: args.publicKeyHex,
    nonce: u64JsonNumber(nonce),
  };

  const res = await api.POST("/v1/accounts/{id}/api-keys/revoke", {
    params: { path: { id: args.accountId } },
    body: await attachSignature(args, body, canonical),
  });
  throwIfError(res, "revoke_api_key");
}

// --- shared signing helpers (mirrors orders.ts) ---------------------------

/**
 * Append the auth-scheme-specific signature fields to a request body. Uses the
 * session's auth scheme unless overridden on `args`.
 */
async function attachSignature<T extends Record<string, unknown>>(
  args: SettingsSignerArgs,
  body: T,
  canonical: Uint8Array,
): Promise<T & Record<string, unknown>> {
  const auth = resolveAuthContext(args);
  if (auth.authScheme === "webauthn") {
    return {
      ...body,
      auth_scheme: "webauthn" as const,
      webauthn_assertion: await signWebAuthnBytes(
        auth.credentialIdB64url,
        canonical,
      ),
    };
  }
  return {
    ...body,
    signature_hex: await signRawBytes(args.accountId, canonical),
  };
}

function resolveAuthContext(args: {
  accountId: number;
  authScheme?: AccountAuthScheme;
  credentialIdB64url?: string;
}):
  | { authScheme: "raw_p256" }
  | { authScheme: "webauthn"; credentialIdB64url: string } {
  const session = useAccountStore.getState().session;
  const authScheme =
    args.authScheme ??
    (session?.accountId === args.accountId ? session.authScheme : undefined) ??
    "raw_p256";
  if (authScheme === "webauthn") {
    const credentialIdB64url =
      args.credentialIdB64url ??
      (session?.accountId === args.accountId
        ? session.credentialIdB64url
        : undefined);
    if (!credentialIdB64url) {
      throw new Error(
        `No passkey credential for account ${args.accountId} in this browser`,
      );
    }
    return { authScheme, credentialIdB64url };
  }
  return { authScheme: "raw_p256" };
}

async function signRawBytes(
  accountId: number,
  canonical: Uint8Array,
): Promise<string> {
  const key = getKeyHandle(accountId);
  if (!key) {
    throw new Error(
      `No private key for account ${accountId} in this browser — reconnect`,
    );
  }
  return signBytes(key, canonical);
}

function nextReplayNonce(accountId: number): bigint {
  const now = BigInt(Date.now());
  const storageKey = `sybil:account:${accountId}:lastReplayNonce`;
  let previous = 0n;
  try {
    const raw = globalThis.localStorage?.getItem(storageKey);
    if (raw) previous = BigInt(raw);
  } catch {
    previous = 0n;
  }
  const next = now > previous ? now : previous + 1n;
  try {
    globalThis.localStorage?.setItem(storageKey, next.toString());
  } catch {
    // Best effort only; the signed payload still carries the returned nonce.
  }
  return next;
}

function u64JsonNumber(value: bigint): number {
  if (value < 0n || value > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new Error("nonce exceeds JavaScript's safe JSON integer range");
  }
  return Number(value);
}

function throwIfError(
  res: { error?: unknown; response?: { status?: number } },
  label: string,
): void {
  if (res.error) {
    const status = res.response?.status;
    const detail = serverErrorMessage(res.error);
    throw new SettingsActionError(
      `${label} failed (HTTP ${status ?? "?"}): ${detail}`,
      status,
    );
  }
}

export class SettingsActionError extends Error {
  constructor(
    message: string,
    public readonly status?: number,
  ) {
    super(message);
    this.name = "SettingsActionError";
  }
}

function serverErrorMessage(err: unknown): string {
  if (err && typeof err === "object") {
    const e = err as Record<string, unknown>;
    if (typeof e.message === "string") return e.message;
    if (typeof e.error === "string") return e.error;
    return JSON.stringify(err);
  }
  return String(err);
}
