/**
 * Aligns per-market price histories onto a shared time grid for the SVG
 * price chart.
 *
 * Each market's `/prices/history` reports points at its own irregular
 * timestamps. To draw any multi-line chart they must share an x-axis, so
 * every lane is forward-filled — step interpolation — onto the union of all
 * timestamps. The output `raw` holds each outcome's real YES probability
 * (0..1); the chart layers stacking / normalization on top per mode.
 */

import type { components } from "@/lib/api/schema";
import type { PricePoint } from "@/lib/markets/use-price-history";

type Block = components["schemas"]["BlockResponse"];

export type ChartSeries = {
  /** Grid timestamps (ms), ascending. Spans the full selected window — the
   *  range bounds are injected as endpoints even where no point exists. */
  times: number[];
  /** `raw[outcomeIdx][timeIdx]` — real YES probability, 0..1. */
  raw: number[][];
  /** True when at least one outcome has a real clearing point (history or
   *  live block) — distinguishes "empty market" from "empty window". */
  hasData: boolean;
};

/** Cap on grid resolution — SVG paths past this add nothing visible. */
const MAX_POINTS = 360;

/**
 * Heuristic NegRisk detector. A mutually-exclusive (NegRisk) event's outcome
 * YES prices partition probability — they sum to ~100¢. Independent binaries
 * that merely share an `event_id` do not. The frontend has no NegRisk flag
 * (the mirror knows it but `MarketResponse` doesn't expose it), so we infer:
 * every outcome priced AND the sum within tolerance of 1 ⇒ stackable.
 *
 * Conservative on purpose — anything ambiguous (partial pricing, off-sum)
 * falls through to `false`, and the chart defaults such groups to overlaid
 * lines, which never falsely implies a partition.
 *
 * TODO(backend): replace with a real `neg_risk` field on `MarketResponse`
 * (off-block, mirror-populated like `event_id`).
 */
export function detectStackable(outcomes: { yesCents: number | null }[]): boolean {
  if (outcomes.length < 2) return false;
  if (outcomes.some((o) => o.yesCents == null)) return false;
  const sum = outcomes.reduce((a, o) => a + (o.yesCents ?? 0) / 100, 0);
  return Math.abs(sum - 1) <= 0.12;
}

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
  const out: Pt[] = [];
  for (const p of pts) {
    const prev = out[out.length - 1];
    if (prev && prev.t === p.t) prev.v = p.v;
    else out.push(p);
  }
  return out;
}

/**
 * @param sinceMs Window start (ms), or `null` for ALL.
 * @param nowMs   Reference "now" — the latest committed block time. Used as
 *                the right edge so the axis ends at the present, not at the
 *                last point that happens to exist.
 */
export function buildChartSeries(
  outcomes: { marketId: number; yesPriceNanos: bigint | null }[],
  byMarket: Map<number, PricePoint[]>,
  recentBlocks: Block[],
  sinceMs: number | null,
  nowMs: number,
): ChartSeries {
  const lanes = outcomes.map((o) =>
    laneFor(o.marketId, byMarket.get(o.marketId) ?? [], recentBlocks),
  );
  const hasData = lanes.some((l) => l.length > 0);

  const timeSet = new Set<number>();
  for (const lane of lanes) for (const p of lane) timeSet.add(p.t);
  let times = [...timeSet].sort((a, b) => a - b);

  // Right edge = real "now" (or the last point if no block time known).
  const lastPoint = times[times.length - 1] ?? 0;
  const domainEnd = Math.max(nowMs || 0, lastPoint);

  // Inject the window bounds as grid endpoints so the line spans the whole
  // selected range — a sparse market then reads as flat-held, not squeezed.
  if (sinceMs != null) {
    times = times.filter((t) => t >= sinceMs && t <= domainEnd);
    if (times[0] !== sinceMs) times.unshift(sinceMs);
  }
  if (times.length === 0 || times[times.length - 1] !== domainEnd) {
    times.push(domainEnd);
  }

  if (times.length > MAX_POINTS) {
    const stride = Math.ceil(times.length / MAX_POINTS);
    const last = times[times.length - 1]!;
    times = times.filter((_, i) => i % stride === 0);
    if (times[times.length - 1] !== last) times.push(last);
  }

  if (times.length === 0) {
    return { times: [], raw: outcomes.map(() => []), hasData };
  }

  // Forward-fill each lane onto the grid; back-fill before its first point so
  // lines have no holes; a lane with no history falls back to current price.
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

  return { times, raw, hasData };
}
