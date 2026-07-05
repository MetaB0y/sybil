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
import { canonicalCancelBytes, canonicalOrderBytes } from "@/lib/auth/canonical";
import { signBytes } from "@/lib/auth/p256";
import { getKeyHandle } from "./store";
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
}

export async function submitSignedOrder(
  args: SubmitSignedOrderArgs,
): Promise<{ accepted: boolean }> {
  const key = getKeyHandle(args.accountId);
  if (!key) {
    throw new Error(
      `No private key for account ${args.accountId} in this browser — reconnect`,
    );
  }

  // Resolve the effective TIF. IOC/GTD sign `expires_at_block`; GTC signs None.
  const tif: SubmitTimeInForce =
    args.timeInForce ?? (args.expiresAtBlock !== undefined ? "GTD" : "GTC");
  if (tif !== "GTC" && args.expiresAtBlock === undefined) {
    throw new Error(`${tif} orders require expires_at_block`);
  }
  const expiresAtBlock = tif === "GTC" ? undefined : args.expiresAtBlock;

  const payoffs = PAYOFFS[args.side];
  const nonce = args.nonce ?? nextReplayNonce(args.accountId);
  const canonical = canonicalOrderBytes({
    marketIds: [args.marketId],
    payoffs,
    limitPriceNanos: args.limitPriceNanos,
    maxFill: args.maxFill,
    nonce,
    ...(expiresAtBlock !== undefined ? { expiresAtBlock } : {}),
  });
  const signature_hex = await signBytes(key, canonical);

  const res = await api.POST("/v1/orders/signed", {
    body: {
      signer_pubkey_hex: args.publicKeyHex,
      order: {
        market_ids: [args.marketId],
        payoffs,
        // patched schema says string; wire wants JSON number.
        limit_price_nanos: Number(args.limitPriceNanos) as unknown as string,
        max_fill: Number(args.maxFill),
      },
      nonce: u64JsonNumber(nonce),
      signature_hex,
      ...(expiresAtBlock !== undefined
        ? {
            expires_at_block: Number(expiresAtBlock),
            time_in_force: tif,
          }
        : {}),
    },
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
}

export async function cancelSignedOrder(
  args: CancelSignedOrderArgs,
): Promise<{ cancelled: boolean }> {
  const key = getKeyHandle(args.accountId);
  if (!key) {
    throw new Error(
      `No private key for account ${args.accountId} in this browser — reconnect`,
    );
  }

  const nonce = args.nonce ?? nextReplayNonce(args.accountId);
  const canonical = canonicalCancelBytes(
    BigInt(args.accountId),
    BigInt(args.orderId),
    nonce,
  );
  const signature_hex = await signBytes(key, canonical);

  const res = await api.POST("/v1/orders/cancel/signed", {
    body: {
      account_id: args.accountId,
      order_id: args.orderId,
      signer_pubkey_hex: args.publicKeyHex,
      nonce: u64JsonNumber(nonce),
      signature_hex,
    },
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
