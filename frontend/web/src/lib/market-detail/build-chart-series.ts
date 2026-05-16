/**
 * Aligns per-market price histories onto a shared time grid for the SVG
 * price chart.
 *
 * Each market's `/prices/history` reports points at its own irregular
 * timestamps. To draw a stacked-area (or any multi-line) chart they must
 * share an x-axis, so every lane is forward-filled — step interpolation —
 * onto the union of all timestamps.
 *
 * Two value sets come out:
 *  - `raw`  — each outcome's real YES probability (0..1). Used for tooltips.
 *  - `norm` — `raw` normalized so every column sums to 1. Used for the
 *    100%-stacked band heights, so the stack always fills 0–100% even though
 *    independently-mirrored binaries don't price-sum to exactly 1.
 */

import type { components } from "@/lib/api/schema";
import type { PricePoint } from "@/lib/markets/use-price-history";

type Block = components["schemas"]["BlockResponse"];

export type ChartSeries = {
  /** Grid timestamps (ms), ascending. */
  times: number[];
  /** `raw[outcomeIdx][timeIdx]` — real YES probability, 0..1. */
  raw: number[][];
  /** `norm[outcomeIdx][timeIdx]` — column-normalized, sums to 1 per column. */
  norm: number[][];
};

/** Cap on grid resolution — SVG paths past this add nothing visible. */
const MAX_POINTS = 360;

function probFromNanos(nanos: string | number | bigint): number {
  const n =
    typeof nanos === "bigint"
      ? nanos
      : BigInt(typeof nanos === "number" ? Math.round(nanos) : nanos);
  return Number(n) / 1e9;
}

type Pt = { t: number; v: number };

/** Merge a market's history + live block ticks into one sorted, deduped lane. */
function laneFor(
  marketId: number,
  history: PricePoint[],
  blocks: Block[],
): Pt[] {
  const pts: Pt[] = [];
  for (const p of history) {
    pts.push({ t: p.timestamp_ms, v: probFromNanos(p.yes_price_nanos) });
  }
  for (const b of blocks) {
    const yes = b.clearing_prices_nanos?.[String(marketId)]?.[0];
    if (yes == null) continue;
    pts.push({ t: b.timestamp_ms, v: probFromNanos(yes) });
  }
  pts.sort((a, b) => a.t - b.t);
  // Dedupe by timestamp, keeping the last value seen for that instant.
  const out: Pt[] = [];
  for (const p of pts) {
    const prev = out[out.length - 1];
    if (prev && prev.t === p.t) prev.v = p.v;
    else out.push(p);
  }
  return out;
}

export function buildChartSeries(
  outcomes: { marketId: number; yesPriceNanos: bigint | null }[],
  byMarket: Map<number, PricePoint[]>,
  recentBlocks: Block[],
  sinceMs: number | null,
): ChartSeries {
  const lanes = outcomes.map((o) =>
    laneFor(o.marketId, byMarket.get(o.marketId) ?? [], recentBlocks),
  );

  // Union of every timestamp, then window to the selected range.
  const timeSet = new Set<number>();
  for (const lane of lanes) for (const p of lane) timeSet.add(p.t);
  let times = [...timeSet].sort((a, b) => a - b);
  if (sinceMs != null) times = times.filter((t) => t >= sinceMs);

  // Downsample evenly if the grid is denser than we can usefully draw.
  if (times.length > MAX_POINTS) {
    const stride = Math.ceil(times.length / MAX_POINTS);
    const last = times[times.length - 1]!;
    times = times.filter((_, i) => i % stride === 0);
    if (times[times.length - 1] !== last) times.push(last);
  }

  if (times.length === 0) {
    return { times: [], raw: outcomes.map(() => []), norm: outcomes.map(() => []) };
  }

  // Forward-fill each lane onto the grid. Before a lane's first point we
  // back-fill with its earliest value so bands have no holes; a lane with no
  // history at all falls back to the outcome's current YES price.
  const raw: number[][] = lanes.map((lane, k) => {
    const fallback =
      outcomes[k]?.yesPriceNanos != null
        ? Number(outcomes[k]!.yesPriceNanos) / 1e9
        : 0;
    if (lane.length === 0) return times.map(() => fallback);
    const row: number[] = [];
    let cursor = 0;
    for (const t of times) {
      while (cursor + 1 < lane.length && lane[cursor + 1]!.t <= t) cursor++;
      const pt = lane[cursor]!;
      row.push(pt.t <= t ? pt.v : lane[0]!.v);
    }
    return row;
  });

  // Column-normalize for the stacked-band geometry.
  const norm: number[][] = outcomes.map(() => new Array(times.length).fill(0));
  for (let i = 0; i < times.length; i++) {
    let sum = 0;
    for (let k = 0; k < raw.length; k++) sum += raw[k]![i]!;
    for (let k = 0; k < raw.length; k++) {
      norm[k]![i] = sum > 0 ? raw[k]![i]! / sum : 1 / raw.length;
    }
  }

  return { times, raw, norm };
}
