"use client";

/**
 * High-level signed-order submission + cancellation.
 *
 * Each call pulls the private CryptoKey from the module-level registry
 * (`store.ts:KEY_HANDLES`), builds the canonical bytes, signs with
 * WebCrypto, and POSTs to the server. Shared between `/portfolio-dev` and
 * the live BuyBox (Phase 6).
 */

import { api } from "@/lib/api/client";
import { canonicalCancelBytes, canonicalOrderBytes, fromHex } from "@/lib/auth/canonical";
import { signBytes } from "@/lib/auth/p256";
import { signWebAuthnBytes } from "@/lib/auth/webauthn";
import { getKeyHandle, useAccountStore } from "./store";
import type { AccountAuthScheme } from "./storage";
import { recordCancel } from "./use-cancelled-orders";

export type OrderSide = "BuyYes" | "BuyNo" | "SellYes" | "SellNo";

/**
 * Time-in-force policies the signed endpoint accepts (see `TimeInForce` in the
 * OpenAPI schema and `apply_time_in_force` in `crates/sybil-api/src/convert.rs`):
 *   - GTC — rests until cancelled; `expires_at_block` must be absent.
 *   - GTD — rests until an explicit `expires_at_block` (covered by the signature).
 *   - IOC — same wire shape as GTD, but the client commits to the *next* block
 *     as the last-eligible height, so it only takes the very next batch.
 * IOC/GTD both sign `expires_at_block`; GTC signs `None`.
 */
export type SubmitTimeInForce = "GTC" | "IOC" | "GTD";

const PAYOFFS: Record<OrderSide, [number, number]> = {
  BuyYes: [1, 0],
  BuyNo: [0, 1],
  SellYes: [-1, 0],
  SellNo: [0, -1],
};

export interface SubmitSignedOrderArgs {
  accountId: number;
  publicKeyHex: string;
  marketId: number;
  side: OrderSide;
  limitPriceNanos: bigint;
  maxFill: bigint;
  /** Strictly increasing per-account replay nonce. Defaults to a browser-local monotonic nonce. */
  nonce?: bigint;
  /**
   * Last-eligible block height, covered by the P256 signature. Required for
   * IOC and GTD; must be absent for GTC. For IOC the caller passes the next
   * block height.
   */
  expiresAtBlock?: bigint;
  /**
   * Explicit time-in-force. When omitted it's inferred for backwards
   * compatibility: GTD if `expiresAtBlock` is set, otherwise GTC.
   */
  timeInForce?: SubmitTimeInForce;
  authScheme?: AccountAuthScheme;
  credentialIdB64url?: string;
}

export async function submitSignedOrder(
  args: SubmitSignedOrderArgs,
): Promise<{ accepted: boolean }> {
  // Resolve the effective TIF. IOC/GTD sign `expires_at_block`; GTC signs None.
  const tif: SubmitTimeInForce =
    args.timeInForce ?? (args.expiresAtBlock !== undefined ? "GTD" : "GTC");
  if (tif !== "GTC" && args.expiresAtBlock === undefined) {
    throw new Error(`${tif} orders require expires_at_block`);
  }
  const expiresAtBlock = tif === "GTC" ? undefined : args.expiresAtBlock;

  const payoffs = PAYOFFS[args.side];
  const nonce = args.nonce ?? nextReplayNonce(args.accountId);
  const genesisHash = await getGenesisHashBytes();
  const canonical = canonicalOrderBytes({
    marketIds: [args.marketId],
    payoffs,
    limitPriceNanos: args.limitPriceNanos,
    maxFill: args.maxFill,
    nonce,
    genesisHash,
    ...(expiresAtBlock !== undefined ? { expiresAtBlock } : {}),
  });
  const auth = resolveAuthContext(args);

  const body = {
      signer_pubkey_hex: args.publicKeyHex,
      order: {
        market_ids: [args.marketId],
        payoffs,
        // patched schema says string; wire wants JSON number.
        limit_price_nanos: Number(args.limitPriceNanos) as unknown as string,
        max_fill: Number(args.maxFill),
      },
      nonce: u64JsonNumber(nonce),
      ...(expiresAtBlock !== undefined
        ? {
            expires_at_block: Number(expiresAtBlock),
            time_in_force: tif,
          }
        : {}),
  };

  const signedBody =
    auth.authScheme === "webauthn"
      ? {
          ...body,
          auth_scheme: "webauthn" as const,
          webauthn_assertion: await signWebAuthnBytes(
            auth.credentialIdB64url,
            canonical,
          ),
        }
      : {
          ...body,
          signature_hex: await signRawBytes(args.accountId, canonical),
        };

  const res = await api.POST("/v1/orders/signed", {
    body: signedBody,
  });

  if (res.error) {
    const status = res.response?.status;
    const detail = serverErrorMessage(res.error);
    throw new Error(`submit_signed failed (HTTP ${status ?? "?"}): ${detail}`);
  }
  return { accepted: res.data?.accepted ?? false };
}

export interface CancelSignedOrderArgs {
  accountId: number;
  publicKeyHex: string;
  orderId: number;
  /** Strictly increasing per-account replay nonce. Defaults to a browser-local monotonic nonce. */
  nonce?: bigint;
  /** Optional context cached locally for the Activity-tab CANCELLED row. */
  context?: {
    marketId: number;
    side: string;
    qty: number;
    limitPriceNanos: string;
  };
  authScheme?: AccountAuthScheme;
  credentialIdB64url?: string;
}

export async function cancelSignedOrder(
  args: CancelSignedOrderArgs,
): Promise<{ cancelled: boolean }> {
  const nonce = args.nonce ?? nextReplayNonce(args.accountId);
  const genesisHash = await getGenesisHashBytes();
  const canonical = canonicalCancelBytes(
    BigInt(args.accountId),
    BigInt(args.orderId),
    nonce,
    genesisHash,
  );
  const auth = resolveAuthContext(args);

  const body = {
      account_id: args.accountId,
      order_id: args.orderId,
      signer_pubkey_hex: args.publicKeyHex,
      nonce: u64JsonNumber(nonce),
  };

  const signedBody =
    auth.authScheme === "webauthn"
      ? {
          ...body,
          auth_scheme: "webauthn" as const,
          webauthn_assertion: await signWebAuthnBytes(
            auth.credentialIdB64url,
            canonical,
          ),
        }
      : {
          ...body,
          signature_hex: await signRawBytes(args.accountId, canonical),
        };

  const res = await api.POST("/v1/orders/cancel/signed", {
    body: signedBody,
  });

  if (res.error) {
    const status = res.response?.status;
    const detail = serverErrorMessage(res.error);
    throw new Error(`cancel_signed failed (HTTP ${status ?? "?"}): ${detail}`);
  }
  const cancelled = res.data?.cancelled ?? false;
  if (cancelled && args.context) {
    recordCancel({
      accountId: args.accountId,
      orderId: args.orderId,
      marketId: args.context.marketId,
      side: args.context.side,
      qty: args.context.qty,
      limitPriceNanos: args.context.limitPriceNanos,
      timestampMs: Date.now(),
    });
  }
  return { cancelled };
}

function resolveAuthContext(args: {
  accountId: number;
  authScheme?: AccountAuthScheme;
  credentialIdB64url?: string;
}): { authScheme: "raw_p256" } | { authScheme: "webauthn"; credentialIdB64url: string } {
  const session = useAccountStore.getState().session;
  const authScheme =
    args.authScheme ??
    (session?.accountId === args.accountId ? session.authScheme : undefined) ??
    "raw_p256";
  if (authScheme === "webauthn") {
    const credentialIdB64url =
      args.credentialIdB64url ??
      (session?.accountId === args.accountId ? session.credentialIdB64url : undefined);
    if (!credentialIdB64url) {
      throw new Error(`No passkey credential for account ${args.accountId} in this browser`);
    }
    return { authScheme, credentialIdB64url };
  }
  return { authScheme: "raw_p256" };
}

let genesisHashPromise: Promise<Uint8Array> | null = null;

async function getGenesisHashBytes(): Promise<Uint8Array> {
  genesisHashPromise ??= (async () => {
    const { data, error } = await api.GET("/v1/health");
    if (error || !data) {
      throw new Error("health request failed while loading genesis hash");
    }
    if (!data.genesis_hash) {
      throw new Error("genesis_hash is unavailable until the genesis block is committed");
    }
    return fromHex(data.genesis_hash);
  })();
  try {
    return await genesisHashPromise;
  } catch (err) {
    genesisHashPromise = null;
    throw err;
  }
}

async function signRawBytes(accountId: number, canonical: Uint8Array): Promise<string> {
  const key = getKeyHandle(accountId);
  if (!key) {
    throw new Error(`No private key for account ${accountId} in this browser — reconnect`);
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

function serverErrorMessage(err: unknown): string {
  if (err && typeof err === "object") {
    const e = err as Record<string, unknown>;
    if (typeof e.message === "string") return e.message;
    if (typeof e.error === "string") return e.error;
    return JSON.stringify(err);
  }
  return String(err);
}
