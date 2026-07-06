/**
 * End-to-end smoke test for the signed-order flow against a live Sybil API.
 *
 * Run:  pnpm smoke
 * Override server: SYBIL_API_BASE=https://other.example.com pnpm smoke
 *
 * Excluded from the default `pnpm test` glob (vitest.config.ts only picks up
 * src/**). Run this only when you've changed the canonical-bytes encoding
 * or the auth flow and want to confirm the deployed server still accepts
 * what we produce.
 *
 * Each run:
 *   1. POST /v1/accounts (dev-mode) → fresh demo account
 *   2. Generate ephemeral P-256 keypair (never persisted)
 *   3. POST /v1/accounts/{id}/keys
 *   4. GET  /v1/markets/summary → pick first active binary market
 *   5. Build canonical bytes + sign + POST /v1/orders/signed
 *   6. Wait one batch (2.5 s), GET portfolio / orders / fills
 *   7. If a pending order remains, cancel-signed it
 *
 * Step 6 has three acceptable outcomes:
 *   - order pending  (next batch will clear it)
 *   - order filled   (already cleared this batch)
 *   - balance decreased (reserved against the open order)
 * Any of those proves the signature verified; otherwise the test fails.
 */

import { describe, expect, it } from "vitest";
import {
  generateKeyPair,
  exportPublicKeyCompressedHex,
  signBytes,
} from "../src/lib/auth/p256";
import {
  canonicalOrderBytes,
  canonicalCancelBytes,
  fromHex,
} from "../src/lib/auth/canonical";

const BASE = process.env.SYBIL_API_BASE ?? "https://172-104-31-54.nip.io";
const INITIAL_BALANCE_NANOS = 1_000_000_000_000n; // $1000

interface RestResult<T = unknown> {
  status: number;
  ok: boolean;
  body: T;
}

async function rest<T = unknown>(
  path: string,
  init?: RequestInit,
): Promise<RestResult<T>> {
  const url = `${BASE}${path}`;
  const res = await fetch(url, init);
  const text = await res.text();
  let body: unknown;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    body = text;
  }
  return { status: res.status, ok: res.ok, body: body as T };
}

interface MarketSummary {
  market_id: number;
  name?: string;
  state?: string;
  outcome_count?: number;
}

interface AccountResponse {
  account_id: number;
  balance_nanos: number;
}

interface PortfolioResponse {
  balance_nanos: number;
  positions?: unknown[];
  pnl_nanos?: number;
  portfolio_value_nanos?: number;
  total_deposited_nanos?: number;
}

interface PendingOrder {
  order_id: number;
  account_id: number;
  market_id: number;
  side?: string;
  limit_price_nanos: number;
  remaining_quantity: number;
}

interface OrderAccepted {
  accepted: boolean;
}

interface CancelResp {
  cancelled: boolean;
}

interface HealthResp {
  status: string;
  height?: number | null;
  genesis_hash?: string | null;
}

// Gate the smoke off `pnpm test` runs — only `pnpm smoke` sets SYBIL_SMOKE=1.
const RUN = process.env.SYBIL_SMOKE === "1";

describe.skipIf(!RUN)("signed-flow smoke (live)", () => {
  it(
    "full account → key → order → portfolio → cancel cycle",
    async () => {
      const log = (msg: string) => console.log(`[smoke] ${msg}`);
      log(`server: ${BASE}`);

      // 1. Create demo account
      const created = await rest<AccountResponse>("/v1/accounts", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          initial_balance_nanos: Number(INITIAL_BALANCE_NANOS),
        }),
      });
      expect(
        created.ok,
        `POST /v1/accounts returned ${created.status}: ${JSON.stringify(created.body)}`,
      ).toBe(true);
      const accountId = created.body.account_id;
      log(`account_id = ${accountId}, balance = ${created.body.balance_nanos}`);

      // 2. Generate ephemeral keypair
      const kp = await generateKeyPair();
      const pubHex = await exportPublicKeyCompressedHex(kp.publicKey);
      log(`pubkey = ${pubHex}`);

      // 3. Register pubkey
      const reg = await rest(`/v1/accounts/${accountId}/keys`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ public_key_hex: pubHex }),
      });
      expect(
        reg.ok,
        `POST /v1/accounts/{id}/keys returned ${reg.status}: ${JSON.stringify(reg.body)}`,
      ).toBe(true);
      log("pubkey registered");

      const health = await rest<HealthResp>("/v1/health");
      expect(health.ok).toBe(true);
      const genesisHashHex = health.body.genesis_hash;
      if (!genesisHashHex) throw new Error("/v1/health did not return genesis_hash");
      const genesisHash = fromHex(genesisHashHex);

      // 4. Pick a market — prefer Active binary
      const summary = await rest<MarketSummary[]>("/v1/markets/summary");
      expect(summary.ok).toBe(true);
      const candidates = summary.body.filter(
        (m) =>
          (m.state === undefined || m.state === "Active") &&
          (m.outcome_count === undefined || m.outcome_count === 2),
      );
      const market = candidates[0] ?? summary.body[0];
      if (!market) throw new Error("no markets returned from server");
      log(`market = ${market.market_id} (${market.name ?? "unnamed"})`);

      // 5. Sign + submit a BuyYes @ $0.50 × 1
      const priceNanos = 500_000_000n;
      const qty = 1n;
      const orderNonce = BigInt(Date.now());
      const canonical = canonicalOrderBytes({
        marketIds: [market.market_id],
        payoffs: [1, 0],
        limitPriceNanos: priceNanos,
        maxFill: qty,
        nonce: orderNonce,
        genesisHash,
      });
      const sigHex = await signBytes(kp.privateKey, canonical);
      log(`signed ${canonical.length} bytes, sig = ${sigHex.slice(0, 32)}...`);

      const submitted = await rest<OrderAccepted>("/v1/orders/signed", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          signer_pubkey_hex: pubHex,
          order: {
            market_ids: [market.market_id],
            payoffs: [1, 0],
            limit_price_nanos: Number(priceNanos),
            max_fill: Number(qty),
          },
          nonce: Number(orderNonce),
          signature_hex: sigHex,
        }),
      });
      expect(
        submitted.ok,
        `POST /v1/orders/signed returned ${submitted.status}: ${JSON.stringify(submitted.body)}`,
      ).toBe(true);
      expect(submitted.body.accepted).toBe(true);
      log("order accepted");

      // 6. Wait one batch + query state
      await new Promise((r) => setTimeout(r, 2500));
      const [portfolio, openOrders, fills] = await Promise.all([
        rest<PortfolioResponse>(`/v1/accounts/${accountId}/portfolio`),
        rest<PendingOrder[]>(`/v1/accounts/${accountId}/orders`),
        rest<unknown[]>(`/v1/accounts/${accountId}/fills?limit=10`),
      ]);
      expect(portfolio.ok).toBe(true);
      expect(openOrders.ok).toBe(true);
      expect(fills.ok).toBe(true);

      const balanceAfter = BigInt(portfolio.body.balance_nanos);
      log(
        `portfolio: balance=${balanceAfter}, positions=${portfolio.body.positions?.length ?? 0}`,
      );
      log(`open orders: ${openOrders.body.length}`);
      log(`fills: ${fills.body.length}`);

      const balanceChanged = balanceAfter < INITIAL_BALANCE_NANOS;
      const hasPending = openOrders.body.length > 0;
      const hasFill = fills.body.length > 0;
      expect(
        balanceChanged || hasPending || hasFill,
        "order had no effect — signature may have failed verify",
      ).toBe(true);

      // 7. If still pending, cancel
      if (hasPending) {
        const orderId = openOrders.body[0]!.order_id;
        const cancelNonce = BigInt(Date.now());
        const cancelCanonical = canonicalCancelBytes(
          BigInt(accountId),
          BigInt(orderId),
          cancelNonce,
          genesisHash,
        );
        const cancelSig = await signBytes(kp.privateKey, cancelCanonical);
        const cancelled = await rest<CancelResp>("/v1/orders/cancel/signed", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            account_id: accountId,
            order_id: orderId,
            nonce: Number(cancelNonce),
            signer_pubkey_hex: pubHex,
            signature_hex: cancelSig,
          }),
        });
        expect(
          cancelled.ok,
          `cancel returned ${cancelled.status}: ${JSON.stringify(cancelled.body)}`,
        ).toBe(true);
        expect(cancelled.body.cancelled).toBe(true);
        log(`cancelled order ${orderId}`);
      } else {
        log("nothing to cancel (already filled or expired)");
      }

      log(`✅ smoke passed for account ${accountId}`);
    },
    30_000,
  );
});
