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

/**
 * Format probability nanos as integer cents 0–100 with ¢ suffix.
 * Edge cases: values >99.5% render as `>99¢`; values <0.5% render as `<1¢`.
 */
export const formatCents = (v: NanosInput): string => {
  const nanos = parseNanos(v);
  const cents = Number(nanos) / 1e7;
  if (cents > 99.5) return ">99¢";
  if (cents < 0.5 && cents > 0) return "<1¢";
  return `${Math.round(cents)}¢`;
};

/** Format a percent change (e.g. +4.2%, -1.4%) with a leading sign. */
export const formatPctDelta = (pct: number): string => {
  const sign = pct >= 0 ? "+" : "";
  return `${sign}${pct.toFixed(1)}%`;
};

/** Format an absolute price change in cents (e.g. +5¢, -3¢, 0¢). */
export const formatCentsDelta = (cents: number): string => {
  const rounded = Math.round(cents);
  if (rounded === 0) return "0¢";
  const sign = rounded > 0 ? "+" : "−";
  return `${sign}${Math.abs(rounded)}¢`;
};

/** Plain integer formatter for height / block / count values that aren't money. */
export const formatInt = (v: NanosInput): string =>
  parseNanos(v).toLocaleString("en-US");

/**
 * Compact integer: 1_234 → "1.2K", 1_234_567 → "1.2M", 1_234_567_890 → "1.2B".
 * Drops trailing zero in the decimal (1.0K → "1K"). Negatives keep their sign.
 * Use for stat-strip / hero numbers where space matters; use formatInt for
 * tables where alignment matters more than width.
 */
export const formatCompactInt = (n: number): string => {
  const abs = Math.abs(n);
  const sign = n < 0 ? "-" : "";
  if (abs >= 1_000_000_000)
    return `${sign}${trimTrailingZero((abs / 1_000_000_000).toFixed(1))}B`;
  if (abs >= 1_000_000)
    return `${sign}${trimTrailingZero((abs / 1_000_000).toFixed(2))}M`;
  if (abs >= 1_000)
    return `${sign}${trimTrailingZero((abs / 1_000).toFixed(1))}K`;
  return `${sign}${abs}`;
};

const trimTrailingZero = (s: string): string =>
  s.replace(/\.?0+$/, "");

/** Compact dollar formatter: $4.2M, $312K, $84.5K, $12. No decimal under $10. */
export const formatCompactDollars = (v: NanosInput): string => {
  const nanos = parseNanos(v);
  const negative = nanos < 0n;
  const abs = negative ? -nanos : nanos;
  const sign = negative ? "-" : "";
  const dollars = Number(abs / NANOS_PER_UNIT); // safe for compact display
  if (dollars >= 1_000_000_000) return `${sign}$${(dollars / 1_000_000_000).toFixed(1)}B`;
  if (dollars >= 1_000_000) return `${sign}$${(dollars / 1_000_000).toFixed(1)}M`;
  if (dollars >= 10_000) return `${sign}$${Math.round(dollars / 1_000)}K`;
  if (dollars >= 1_000) return `${sign}$${(dollars / 1_000).toFixed(1)}K`;
  return `${sign}$${dollars}`;
};

/**
 * Format a batch countdown as one-decimal seconds (e.g. "1.8", "0.3").
 *
 * The whole app runs a 2s batch cadence — an mm:ss format would only ever
 * cycle 0:00–0:02 and visibly jump. One decimal is the honest, smooth
 * representation; this is the single formatter every batch clock shares.
 */
export const formatBatchSeconds = (seconds: number): string =>
  Math.max(0, seconds).toFixed(1);

/**
 * Format an elapsed duration as a compact human age — "30s", "45m", "19h",
 * "3d" (largest whole unit only). Pure: the caller passes elapsed ms (e.g.
 * `latestBlock.timestamp_ms − created_at_ms`) so render stays deterministic.
 */
export const formatAge = (elapsedMs: number): string => {
  const s = Math.max(0, Math.floor(elapsedMs / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86_400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86_400)}d`;
};

/** Format an epoch-ms timestamp as "MMM D, YYYY" in en-US. */
export const formatDate = (epochMs: number | null | undefined): string => {
  if (epochMs == null) return "—";
  try {
    return new Date(epochMs).toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
      year: "numeric",
    });
  } catch {
    return "—";
  }
};
