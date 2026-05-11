/**
 * Helpers for Sybil's nanos values (1 unit = 1e9 nanos).
 *
 * ⚠ The OpenAPI schema declares `*_nanos` fields as `string` in TypeScript via
 * the `scripts/patch-bigints.mjs` post-process — but the live backend still
 * serializes them as JSON numbers on the wire. That means values above 2^53
 * can already be corrupted by the time the JSON arrives. `parseNanos` accepts
 * `number | string | bigint` so we cope at runtime, but the proper fix is
 * server-side (utoipa → emit u64 as JSON string). See frontend/KNOWN_ISSUES.md.
 */

export const NANOS_PER_UNIT = 1_000_000_000n;

export type NanosInput = string | number | bigint;

export const parseNanos = (v: NanosInput): bigint => {
  if (typeof v === "bigint") return v;
  if (typeof v === "string") return BigInt(v);
  // JSON number on the wire. Safe up to Number.MAX_SAFE_INTEGER.
  return BigInt(v);
};

/** Format nanos as a dollar string with N decimal places (default 2). */
export const formatDollars = (
  v: NanosInput,
  opts?: { decimals?: number; sign?: boolean }
): string => {
  const nanos = parseNanos(v);
  const decimals = opts?.decimals ?? 2;
  const negative = nanos < 0n;
  const abs = negative ? -nanos : nanos;
  const whole = abs / NANOS_PER_UNIT;
  const frac = abs % NANOS_PER_UNIT;
  const fracStr = frac.toString().padStart(9, "0").slice(0, decimals);
  const sign = negative ? "-" : opts?.sign ? "+" : "";
  return decimals > 0 ? `${sign}$${whole.toString()}.${fracStr}` : `${sign}$${whole}`;
};

/** Format probability nanos (range 0..1e9 = 0..100%). */
export const formatProbability = (v: NanosInput): string => {
  const nanos = parseNanos(v);
  // Range is bounded 0..1e9 → safe to convert to Number.
  const pct = (Number(nanos) / 1e7).toFixed(1);
  return `${pct}%`;
};

/** Plain integer formatter for height / block / count values that aren't money. */
export const formatInt = (v: NanosInput): string =>
  parseNanos(v).toLocaleString("en-US");
