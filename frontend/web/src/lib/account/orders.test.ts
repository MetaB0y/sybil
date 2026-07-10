/**
 * `submitSignedOrder` builds the exact `/v1/orders/signed` POST body the server
 * expects, with the replay nonce covered by the P256 signature — the same shape
 * proven end-to-end in `scripts/smoke-signed-flow.test.ts`.
 *
 * We mock only the HTTP client; the signing path (canonical borsh bytes +
 * WebCrypto ECDSA) runs for real against an ephemeral key, and each test
 * re-verifies the produced signature against the canonical bytes it should
 * cover — including `nonce` — so a schema/nonce drift fails here before the wire.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { getMock, postMock } = vi.hoisted(() => ({
  getMock: vi.fn(),
  postMock: vi.fn(),
}));
vi.mock("@/lib/api/client", () => ({ api: { GET: getMock, POST: postMock } }));

import { canonicalOrderBytes } from "@/lib/auth/canonical";
import { exportPublicKeyCompressedHex, generateKeyPair } from "@/lib/auth/p256";
import { submitSignedOrder } from "./orders";
import { setKeyHandle } from "./store";

const ACCOUNT_ID = 42;
const MARKET_ID = 7;
const GENESIS_HASH = new Uint8Array(32).fill(0xab);
const GENESIS_HASH_HEX = "ab".repeat(32);

let pubHex: string;
let publicKey: CryptoKey;

beforeEach(async () => {
  getMock.mockReset();
  getMock.mockResolvedValue({
    data: { status: "ok", height: 1, genesis_hash: GENESIS_HASH_HEX },
    error: undefined,
    response: { status: 200 },
  });
  postMock.mockReset();
  postMock.mockResolvedValue({
    data: { accepted: true },
    error: undefined,
    response: { status: 200 },
  });
  const kp = await generateKeyPair();
  publicKey = kp.publicKey;
  pubHex = await exportPublicKeyCompressedHex(kp.publicKey);
  setKeyHandle(ACCOUNT_ID, kp.privateKey);
});

function bodyOf(): Record<string, unknown> {
  expect(postMock).toHaveBeenCalledTimes(1);
  const call = postMock.mock.calls[0]!;
  expect(call[0]).toBe("/v1/orders/signed");
  return (call[1] as { body: Record<string, unknown> }).body;
}

async function assertSignatureCovers(
  sigHex: string,
  bytes: Uint8Array,
): Promise<void> {
  const sig = Uint8Array.from(
    sigHex.match(/.{2}/g)!.map((h) => parseInt(h, 16)),
  );
  const ok = await crypto.subtle.verify(
    { name: "ECDSA", hash: "SHA-256" },
    publicKey,
    sig as unknown as BufferSource,
    bytes as unknown as BufferSource,
  );
  expect(ok).toBe(true);
}

describe("submitSignedOrder", () => {
  it("posts the exact GTC body with the nonce covered by the signature", async () => {
    const nonce = 1234567890n;
    const limitPriceNanos = 500_000_000n; // 50¢
    const maxFill = 3000n; // 3 shares

    const res = await submitSignedOrder({
      accountId: ACCOUNT_ID,
      publicKeyHex: pubHex,
      marketId: MARKET_ID,
      side: "BuyYes",
      limitPriceNanos,
      maxFill,
      nonce,
    });
    // No `order_ids` in this response → orderIds falls back to [] (older API).
    expect(res).toEqual({ accepted: true, orderIds: [] });

    const body = bodyOf();
    expect(body.signer_pubkey_hex).toBe(pubHex);
    expect(body.order).toEqual({
      market_ids: [MARKET_ID],
      payoffs: [1, 0],
      limit_price_nanos: 500_000_000, // JSON number on the wire
      max_fill: 3000,
    });
    expect(body.nonce).toBe(Number(nonce));
    // GTC signs no expiry and sends neither field.
    expect(body.expires_at_block).toBeUndefined();
    expect(body.time_in_force).toBeUndefined();

    await assertSignatureCovers(
      body.signature_hex as string,
      canonicalOrderBytes({
        marketIds: [MARKET_ID],
        payoffs: [1, 0],
        limitPriceNanos,
        maxFill,
        nonce,
        genesisHash: GENESIS_HASH,
      }),
    );
  });

  it("sends time_in_force IOC + signed expires_at_block", async () => {
    const nonce = 99n;
    const expiresAtBlock = 5001n;
    await submitSignedOrder({
      accountId: ACCOUNT_ID,
      publicKeyHex: pubHex,
      marketId: MARKET_ID,
      side: "BuyNo",
      limitPriceNanos: 300_000_000n,
      maxFill: 1000n,
      nonce,
      timeInForce: "IOC",
      expiresAtBlock,
    });

    const body = bodyOf();
    expect(body.time_in_force).toBe("IOC");
    expect(body.expires_at_block).toBe(5001);
    expect((body.order as { payoffs: number[] }).payoffs).toEqual([0, 1]);

    await assertSignatureCovers(
      body.signature_hex as string,
      canonicalOrderBytes({
        marketIds: [MARKET_ID],
        payoffs: [0, 1],
        limitPriceNanos: 300_000_000n,
        maxFill: 1000n,
        nonce,
        genesisHash: GENESIS_HASH,
        expiresAtBlock,
      }),
    );
  });

  it("sends time_in_force GTD when expires_at_block is set without an explicit tif", async () => {
    await submitSignedOrder({
      accountId: ACCOUNT_ID,
      publicKeyHex: pubHex,
      marketId: MARKET_ID,
      side: "SellYes",
      limitPriceNanos: 600_000_000n,
      maxFill: 2000n,
      nonce: 7n,
      expiresAtBlock: 8080n,
    });
    const body = bodyOf();
    expect(body.time_in_force).toBe("GTD");
    expect(body.expires_at_block).toBe(8080);
  });

  it("rejects IOC/GTD without an expires_at_block", async () => {
    await expect(
      submitSignedOrder({
        accountId: ACCOUNT_ID,
        publicKeyHex: pubHex,
        marketId: MARKET_ID,
        side: "BuyYes",
        limitPriceNanos: 500_000_000n,
        maxFill: 1000n,
        nonce: 1n,
        timeInForce: "IOC",
      }),
    ).rejects.toThrow(/expires_at_block/);
    expect(postMock).not.toHaveBeenCalled();
  });

  it("surfaces a server rejection with status + detail", async () => {
    postMock.mockResolvedValue({
      data: undefined,
      error: { message: "replay nonce is stale or duplicate" },
      response: { status: 409 },
    });
    await expect(
      submitSignedOrder({
        accountId: ACCOUNT_ID,
        publicKeyHex: pubHex,
        marketId: MARKET_ID,
        side: "BuyYes",
        limitPriceNanos: 500_000_000n,
        maxFill: 1000n,
        nonce: 2n,
      }),
    ).rejects.toThrow(/HTTP 409/);
  });
});
