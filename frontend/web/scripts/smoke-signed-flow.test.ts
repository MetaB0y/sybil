/**
 * End-to-end smoke test for the signed-order flow against a live Sybil API.
 *
 * Run:  pnpm smoke
 * Override server: SYBIL_API_BASE=https://other.example.com pnpm smoke
 *
 * The live suite is skipped during the default `pnpm test`; only `pnpm smoke`
 * enables it. Run the live suite only when you've changed canonical-byte
 * encoding or the auth flow and want to confirm a server accepts what we
 * produce.
 *
 * Each run:
 *   1. Generate an ephemeral P-256 keypair (never persisted)
 *   2. Atomically POST /v1/accounts with that initial key
 *   3. Mint a signed read bearer for owner-scoped account reads
 *   4. GET  /v1/markets/summary → pick first active binary market
 *   5. Build canonical bytes + sign + POST /v1/orders/signed
 *   6. Wait one batch (2.5 s), authenticate GET portfolio / orders / fills
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
  canonicalApiKeyCreateBytes,
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
  bearerToken?: string,
): Promise<RestResult<T>> {
  const url = `${BASE}${path}`;
  const headers = new Headers(init?.headers);
  if (bearerToken) headers.set("Authorization", `Bearer ${bearerToken}`);
  const res = await fetch(url, { ...init, headers });
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

interface CreateApiKeyResponse {
  token: string;
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

function initialAccountBody(publicKeyHex: string) {
  return {
    initial_balance_nanos: Number(INITIAL_BALANCE_NANOS),
    initial_key: {
      public_key_hex: publicKeyHex,
      auth_scheme: "raw_p256",
    },
  };
}

function ownerReadHeaders(token: string): HeadersInit {
  return { Authorization: `Bearer ${token}` };
}

describe("signed-flow smoke request contracts", () => {
  it("uses atomic public onboarding with an initial signing key", () => {
    expect(initialAccountBody("02abcd")).toEqual({
      initial_balance_nanos: Number(INITIAL_BALANCE_NANOS),
      initial_key: {
        public_key_hex: "02abcd",
        auth_scheme: "raw_p256",
      },
    });
  });

  it("authenticates owner-scoped reads with the minted bearer", () => {
    expect(ownerReadHeaders("sybk_test")).toEqual({
      Authorization: "Bearer sybk_test",
    });
  });
});

// Gate the smoke off `pnpm test` runs — only `pnpm smoke` sets SYBIL_SMOKE=1.
const RUN = process.env.SYBIL_SMOKE === "1";

describe.skipIf(!RUN)("signed-flow smoke (live)", () => {
  it(
    "full account → key → order → portfolio → cancel cycle",
    async () => {
      const log = (msg: string) => console.log(`[smoke] ${msg}`);
      log(`server: ${BASE}`);

      // 1. Generate an ephemeral keypair before atomic onboarding.
      const kp = await generateKeyPair();
      const pubHex = await exportPublicKeyCompressedHex(kp.publicKey);
      log(`pubkey = ${pubHex}`);

      // 2. Create the account and initial key in one public request.
      const created = await rest<AccountResponse>("/v1/accounts", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(initialAccountBody(pubHex)),
      });
      expect(
        created.ok,
        `POST /v1/accounts returned ${created.status}: ${JSON.stringify(created.body)}`,
      ).toBe(true);
      const accountId = created.body.account_id;
      log(`account_id = ${accountId}, balance = ${created.body.balance_nanos}`);

      const health = await rest<HealthResp>("/v1/health");
      expect(health.ok).toBe(true);
      const genesisHashHex = health.body.genesis_hash;
      if (!genesisHashHex) throw new Error("/v1/health did not return genesis_hash");
      const genesisHash = fromHex(genesisHashHex);

      let nonce = BigInt(Date.now());
      const nextNonce = () => ++nonce;

      // 3. Mint a read-only bearer for the owner-scoped verification reads.
      const readKeyNonce = nextNonce();
      const readKeyLabel = "signed-flow smoke";
      const readKeyCanonical = canonicalApiKeyCreateBytes(
        BigInt(accountId),
        readKeyLabel,
        readKeyNonce,
      );
      const readKeySignature = await signBytes(
        kp.privateKey,
        readKeyCanonical,
      );
      const readKey = await rest<CreateApiKeyResponse>(
        `/v1/accounts/${accountId}/api-keys`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            label: readKeyLabel,
            signer_pubkey_hex: pubHex,
            auth_scheme: "raw_p256",
            nonce: Number(readKeyNonce),
            signature_hex: readKeySignature,
          }),
        },
      );
      expect(
        readKey.ok,
        `POST /v1/accounts/{id}/api-keys returned ${readKey.status}: ${JSON.stringify(readKey.body)}`,
      ).toBe(true);
      expect(readKey.body.token).toMatch(/^sybk_[0-9a-f]{64}$/);
      log("owner read bearer minted");

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

      // 5. Sign + submit a BuyYes @ $0.50 × 1 share (1,000 units)
      const priceNanos = 500_000_000n;
      const qty = 1_000n;
      const orderNonce = nextNonce();
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
        rest<PortfolioResponse>(
          `/v1/accounts/${accountId}/portfolio`,
          { headers: ownerReadHeaders(readKey.body.token) },
        ),
        rest<PendingOrder[]>(
          `/v1/accounts/${accountId}/orders`,
          { headers: ownerReadHeaders(readKey.body.token) },
        ),
        rest<unknown[]>(
          `/v1/accounts/${accountId}/fills?limit=10`,
          { headers: ownerReadHeaders(readKey.body.token) },
        ),
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
        const cancelNonce = nextNonce();
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
