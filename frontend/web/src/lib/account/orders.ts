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
  /** GTD horizon. Omit for GTC. Demo flows should set ~+5 blocks (≈10s). */
  expiresAtBlock?: bigint;
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

  const payoffs = PAYOFFS[args.side];
  const canonical = canonicalOrderBytes({
    marketIds: [args.marketId],
    payoffs,
    limitPriceNanos: args.limitPriceNanos,
    maxFill: args.maxFill,
    ...(args.expiresAtBlock !== undefined
      ? { expiresAtBlock: args.expiresAtBlock }
      : {}),
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
      signature_hex,
      ...(args.expiresAtBlock !== undefined
        ? {
            expires_at_block: Number(args.expiresAtBlock),
            time_in_force: "GTD" as const,
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

  const canonical = canonicalCancelBytes(
    BigInt(args.accountId),
    BigInt(args.orderId),
  );
  const signature_hex = await signBytes(key, canonical);

  const res = await api.POST("/v1/orders/cancel/signed", {
    body: {
      account_id: args.accountId,
      order_id: args.orderId,
      signer_pubkey_hex: args.publicKeyHex,
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

function serverErrorMessage(err: unknown): string {
  if (err && typeof err === "object") {
    const e = err as Record<string, unknown>;
    if (typeof e.message === "string") return e.message;
    if (typeof e.error === "string") return e.error;
    return JSON.stringify(err);
  }
  return String(err);
}
