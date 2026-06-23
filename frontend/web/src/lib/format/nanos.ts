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

/**
 * Format probability nanos as an integer percent 0–100 with a `%` suffix — the
 * "odds" representation used on the markets index. Same underlying number as
 * `formatCents` (a binary price *is* the implied probability), just relabeled so
 * users read it as odds. Edge cases mirror `formatCents`: >99.5% → ">99%",
 * (0, 0.5%) → "<1%".
 */
export const formatPercent = (v: NanosInput): string => {
  const nanos = parseNanos(v);
  const pct = Number(nanos) / 1e7;
  if (pct > 99.5) return ">99%";
  if (pct < 0.5 && pct > 0) return "<1%";
  return `${Math.round(pct)}%`;
};

/**
 * Shared core for the "precise" price/odds formatters: render a 0..100
 * magnitude (cents or percent) with up to one decimal, trimming a trailing
 * ".0" so whole values stay clean ("12", not "12.0"). A tiny positive value
 * that would round to "0" renders "<0.1" so a real sub-tenth price never reads
 * as a flat zero. No >99 / <1 clamp — the actual number always shows.
 */
const formatTenth = (x: number): string => {
  if (x > 0 && x < 0.05) return "<0.1";
  const s = x.toFixed(1);
  return s.endsWith(".0") ? s.slice(0, -2) : s;
};

/**
 * Like `formatCents` but keeps sub-cent precision: "5.5¢", "0.3¢", "99.7¢";
 * whole cents stay clean ("5¢"). No <1¢/>99¢ clamp. Use this for the user's
 * own order / holding / trade price columns (and the per-outcome odds on the
 * market page), where a fill or weighted-average entry is routinely fractional
 * and rounding to whole cents misleads — two different prices collapse to one,
 * and edge prices vanish into "<1¢"/">99¢". The surrounding dollar P&L is
 * computed on raw nanos regardless; this only fixes the displayed price label.
 */
export const formatCentsPrecise = (v: NanosInput): string =>
  `${formatTenth(Number(parseNanos(v)) / 1e7)}¢`;

/**
 * Percent counterpart to `formatCentsPrecise` — the same underlying number with
 * a `%` suffix, for the browse-card odds. Edge markets read "0.3%" instead of
 * collapsing to "<1%".
 */
export const formatPercentPrecise = (v: NanosInput): string =>
  `${formatTenth(Number(parseNanos(v)) / 1e7)}%`;

/**
 * Format an odds change as a signed percent (e.g. +5%, −3%, +0%) — the delta
 * counterpart to `formatPercent`. The flat case (no change, or caller passes 0
 * because there's no 24h history yet) renders "+0%" so the slot reads as "flat"
 * rather than empty/missing.
 */
export const formatPercentDelta = (pct: number): string => {
  const rounded = Math.round(pct);
  if (rounded === 0) return "+0%";
  const sign = rounded > 0 ? "+" : "−";
  return `${sign}${Math.abs(rounded)}%`;
};

/** Format a percent change (e.g. +4.2%, -1.4%) with a leading sign. */
export const formatPctDelta = (pct: number): string => {
  const sign = pct >= 0 ? "+" : "";
  return `${sign}${pct.toFixed(1)}%`;
};

/**
 * Format an absolute price change in cents (e.g. +5¢, −3¢, ±0¢). The flat
 * case renders as "±0¢" (not a bare "0¢") so it reads as a *delta* rather
 * than being mistaken for a price or for missing data.
 */
export const formatCentsDelta = (cents: number): string => {
  const rounded = Math.round(cents);
  if (rounded === 0) return "±0¢";
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
 * Like `formatCompactDollars` but keeps cents for sub-$1k magnitudes, so small
 * values render as "$0.11" instead of truncating to "$0". Large values still
 * compact (K/M/B). Used where the amount is typically sub-dollar — e.g. the
 * welfare/surplus cells in the activity views.
 */
export const formatCompactDollarsCents = (v: NanosInput): string => {
  const nanos = parseNanos(v);
  const negative = nanos < 0n;
  const abs = negative ? -nanos : nanos;
  const sign = negative ? "-" : "";
  const dollars = Number(abs / NANOS_PER_UNIT); // whole dollars (safe)
  if (dollars >= 1_000_000_000) return `${sign}$${(dollars / 1_000_000_000).toFixed(1)}B`;
  if (dollars >= 1_000_000) return `${sign}$${(dollars / 1_000_000).toFixed(1)}M`;
  if (dollars >= 10_000) return `${sign}$${Math.round(dollars / 1_000)}K`;
  if (dollars >= 1_000) return `${sign}$${(dollars / 1_000).toFixed(1)}K`;
  // Sub-$1k: precise float is safe (abs < 1e12 nanos ≪ 2^53), so cents survive.
  return `${sign}$${(Number(abs) / Number(NANOS_PER_UNIT)).toFixed(2)}`;
};

/**
 * Format a batch countdown as one-decimal seconds (e.g. "1.8", "0.3").
 *
 * The app's batch cadence is short (BLOCK_INTERVAL_MS) — an mm:ss format would
 * barely move and visibly jump. One decimal is the honest, smooth
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
