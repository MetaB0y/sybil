/**
 * Canonical (borsh) byte encoding for signed orders + cancellations.
 *
 * Mirrors `crates/sybil-signing/src/lib.rs` byte-for-byte. Test vectors at
 * `__tests__/canonical.test.ts` pin every field to the Rust insta snapshots —
 * any schema drift fails the tests before a signature ever hits the wire.
 *
 * NOTE: `expires_at_block: Option<u64>` is part of the Rust struct. Test
 * vectors here come from `crates/sybil-signing/src/snapshots/*.snap`.
 */

import { serialize } from "borsh";

export const MAX_MARKETS = 5;
export const MAX_STATES = 32;
export const MARKET_NONE = 0xffffffff;

const PRICE_CONDITION_SCHEMA = {
  struct: {
    market: "u32",
    threshold: "u64",
    direction: {
      enum: [
        { struct: { Above: { struct: {} } } },
        { struct: { Below: { struct: {} } } },
      ],
    },
  },
};

const ORDER_SCHEMA = {
  struct: {
    markets: { array: { type: "u32", len: MAX_MARKETS } },
    num_markets: "u8",
    payoffs: { array: { type: "i8", len: MAX_STATES } },
    num_states: "u8",
    limit_price: "u64",
    max_fill: "u64",
    condition: { option: PRICE_CONDITION_SCHEMA },
    expires_at_block: { option: "u64" },
    nonce: "u64",
  },
};

const CANCEL_SCHEMA = {
  struct: {
    account_id: "u64",
    order_id: "u64",
    nonce: "u64",
  },
};

export type ConditionDirection = "Above" | "Below";

export interface PriceCondition {
  market: number;
  threshold: bigint;
  direction: ConditionDirection;
}

export interface CanonicalOrderInput {
  marketIds: number[];
  payoffs: number[];
  limitPriceNanos: bigint;
  maxFill: bigint;
  condition?: PriceCondition;
  expiresAtBlock?: bigint;
  nonce: bigint;
}

function padMarketIds(ids: readonly number[]): number[] {
  if (ids.length === 0 || ids.length > MAX_MARKETS) {
    throw new Error(
      `order has ${ids.length} markets; must be in [1, ${MAX_MARKETS}]`,
    );
  }
  return Array.from({ length: MAX_MARKETS }, (_, i) => ids[i] ?? MARKET_NONE);
}

function padPayoffs(payoffs: readonly number[]): number[] {
  if (payoffs.length > MAX_STATES) {
    throw new Error(`order has ${payoffs.length} payoffs; max is ${MAX_STATES}`);
  }
  return Array.from({ length: MAX_STATES }, (_, i) => payoffs[i] ?? 0);
}

type EncodedCondition = {
  market: number;
  threshold: bigint;
  direction: { Above: Record<string, never> } | { Below: Record<string, never> };
};

function encodeCondition(c: PriceCondition | undefined): EncodedCondition | null {
  if (!c) return null;
  return {
    market: c.market,
    threshold: c.threshold,
    direction: c.direction === "Above" ? { Above: {} } : { Below: {} },
  };
}

export function canonicalOrderBytes(input: CanonicalOrderInput): Uint8Array {
  const value = {
    markets: padMarketIds(input.marketIds),
    num_markets: input.marketIds.length,
    payoffs: padPayoffs(input.payoffs),
    num_states: 1 << input.marketIds.length,
    limit_price: input.limitPriceNanos,
    max_fill: input.maxFill,
    condition: encodeCondition(input.condition),
    expires_at_block: input.expiresAtBlock ?? null,
    nonce: input.nonce,
  };
  // borsh-js types are loose; ORDER_SCHEMA is structurally correct but its
  // literal type doesn't satisfy the Schema union.
  return new Uint8Array(serialize(ORDER_SCHEMA as never, value));
}

export function canonicalCancelBytes(
  accountId: bigint,
  orderId: bigint,
  nonce: bigint,
): Uint8Array {
  return new Uint8Array(
    serialize(CANCEL_SCHEMA as never, {
      account_id: accountId,
      order_id: orderId,
      nonce,
    }),
  );
}

// --- SYB-60 account-management canonical payloads -------------------------
//
// These mirror `crates/sybil-signing/src/lib.rs` byte-for-byte. borsh mapping:
//   Option<T> → { option: T }  (1-byte tag + value; JS `null` → None)
//   String    → borsh "string"  (u32 len prefix + utf8 bytes)
//   Vec<u8>   → { array: { type: "u8" } }  (u32 len prefix + raw bytes)
//   u64       → bigint (8 bytes little-endian)
// Field ORDER matters — it must match the Rust struct declaration exactly.

const PROFILE_UPDATE_SCHEMA = {
  struct: {
    account_id: "u64",
    display_name: { option: "string" },
    avatar_seed: { option: "string" },
    nonce: "u64",
  },
};

const KEY_REVOCATION_SCHEMA = {
  struct: {
    account_id: "u64",
    target_pubkey: { array: { type: "u8" } },
    nonce: "u64",
  },
};

const API_KEY_CREATE_SCHEMA = {
  struct: {
    account_id: "u64",
    label: { option: "string" },
    nonce: "u64",
  },
};

const API_KEY_REVOKE_SCHEMA = {
  struct: {
    account_id: "u64",
    api_key_id: "u64",
    nonce: "u64",
  },
};

/**
 * Canonical bytes for a signed account-profile update (SYB-60). Mirrors
 * `ProfileUpdate`. Pass `null` for either field to clear it (borsh None).
 */
export function canonicalProfileUpdateBytes(
  accountId: bigint,
  displayName: string | null,
  avatarSeed: string | null,
  nonce: bigint,
): Uint8Array {
  return new Uint8Array(
    serialize(PROFILE_UPDATE_SCHEMA as never, {
      account_id: accountId,
      display_name: displayName,
      avatar_seed: avatarSeed,
      nonce,
    }),
  );
}

/**
 * Canonical bytes for a signed signing-key revocation (SYB-60). Mirrors
 * `KeyRevocation`. `targetPubkey` must be the 33 raw compressed SEC1 bytes of
 * the key being revoked (use `fromHex` on the target hex).
 */
export function canonicalKeyRevocationBytes(
  accountId: bigint,
  targetPubkey: Uint8Array,
  nonce: bigint,
): Uint8Array {
  return new Uint8Array(
    serialize(KEY_REVOCATION_SCHEMA as never, {
      account_id: accountId,
      target_pubkey: targetPubkey,
      nonce,
    }),
  );
}

/**
 * Canonical bytes for a signed read API-key creation (SYB-60). Mirrors
 * `ApiKeyCreate`. `null` label → borsh None.
 */
export function canonicalApiKeyCreateBytes(
  accountId: bigint,
  label: string | null,
  nonce: bigint,
): Uint8Array {
  return new Uint8Array(
    serialize(API_KEY_CREATE_SCHEMA as never, {
      account_id: accountId,
      label,
      nonce,
    }),
  );
}

/**
 * Canonical bytes for a signed read API-key revocation (SYB-60). Mirrors
 * `ApiKeyRevoke`.
 */
export function canonicalApiKeyRevokeBytes(
  accountId: bigint,
  apiKeyId: bigint,
  nonce: bigint,
): Uint8Array {
  return new Uint8Array(
    serialize(API_KEY_REVOKE_SCHEMA as never, {
      account_id: accountId,
      api_key_id: apiKeyId,
      nonce,
    }),
  );
}

export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

export function fromHex(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) throw new Error("hex string must have even length");
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}
