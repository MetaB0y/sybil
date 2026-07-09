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
  /** Grid timestamps (ms), ascending. Starts at the first real clearing —
   *  there is no line before the market/server existed. */
  times: number[];
  /** `raw[outcomeIdx][timeIdx]` — real YES probability, 0..1. */
  raw: number[][];
  /** True when the chart can draw anything — at least one outcome has either a
   *  real clearing point (history / live block) OR a known current price to
   *  hold flat. False only for a market that has never been priced at all. */
  hasData: boolean;
  /** Axis x-range — the selected window. The plotted line may start later
   *  than `domainStart` (blank left = before the market started). */
  domainStart: number;
  domainEnd: number;
};

/** Cap on grid resolution — SVG paths past this add nothing visible. */
const MAX_POINTS = 360;

/** When a market has no time-series at all, we hold its current price flat
 *  across the window. A fixed range (1H/1D/…) already defines that window;
 *  ALL has no natural start, so fall back to a day so the held line is wide
 *  enough to actually see. */
const FLAT_FALLBACK_SPAN_MS = 24 * 60 * 60 * 1000;

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
  outcomes: {
    marketId: number;
    yesPriceNanos?: bigint | null;
    createdAtMs?: number | null;
  }[],
  byMarket: Map<number, PricePoint[]>,
  recentBlocks: Block[],
  sinceMs: number | null,
  nowMs: number,
): ChartSeries {
  const lanes = outcomes.map((o) =>
    laneFor(o.marketId, byMarket.get(o.marketId) ?? [], recentBlocks),
  );
  const hasReal = lanes.some((l) => l.length > 0);

  // Current YES probability per outcome, from the live price store / markets
  // snapshot. A quiet market accrues no clearing history (it's in-memory and
  // rebuilds on restart), but its price is still known — so we hold it flat
  // across the window rather than show a blank chart. `null` only for a market
  // that has truly never been priced.
  const currentProbs = outcomes.map((o) =>
    o.yesPriceNanos != null ? probFromNanos(o.yesPriceNanos) : null,
  );
  const hasCurrent = currentProbs.some((p) => p != null);

  // Drawable when we have either a real series OR a current price to hold flat.
  // Only a never-priced market falls through to the empty-state message.
  const hasData = hasReal || hasCurrent;

  // Earliest real creation across the drawn outcomes (mirrored siblings share
  // one). This — NOT `firstReal` — is when the market actually existed: history
  // is in-memory and rebuilds on restart, so `firstReal` is only "first
  // clearing this session". We use this to hold the line flat back to the
  // window edge without ever drawing before the market was created.
  let creationMs: number | null = null;
  for (const o of outcomes) {
    if (o.createdAtMs != null) {
      creationMs = creationMs == null ? o.createdAtMs : Math.min(creationMs, o.createdAtMs);
    }
  }

  const timeSet = new Set<number>();
  for (const lane of lanes) for (const p of lane) timeSet.add(p.t);
  const union = [...timeSet].sort((a, b) => a - b);

  const firstReal = union[0] ?? nowMs;
  const lastReal = union[union.length - 1] ?? nowMs;
  // Axis spans the selected window; right edge is real "now".
  const domainEnd = Math.max(nowMs || 0, lastReal);
  let domainStart = sinceMs != null ? sinceMs : firstReal;

  if (!hasData) {
    return { times: [], raw: outcomes.map(() => []), hasData, domainStart, domainEnd };
  }

  // ALL spans the market's whole lifetime — creation → now — not merely this
  // session's clearing history. `firstReal` is only the first clearing THIS
  // session: the series lives in the in-memory recent-block ring buffer (~the
  // last several minutes) and rebuilds on restart, so leaving `domainStart` at
  // `firstReal` collapses ALL to those few minutes even for a days-old market.
  // Extend the axis back to real creation and let the line hold flat across the
  // gap, exactly as the fixed windows hold flat back to their left edge. With
  // no creation time AND no real series, span a day so the held flat line is
  // wide enough to see rather than an invisible dot.
  if (sinceMs == null) {
    if (creationMs != null && creationMs < domainEnd) {
      domainStart = creationMs;
    } else if (!hasReal) {
      domainStart = domainEnd - FLAT_FALLBACK_SPAN_MS;
    }
  }

  // Where the drawn line starts. It spans the full selected window — held flat
  // back to the left edge so a quiet market (or one with only seconds of
  // post-restart history) reads as a flatline, not a blank panel — but never
  // before the market was created, so a genuinely new market keeps its honest
  // "this is when it started" gap on the left.
  const startBound = creationMs != null ? Math.max(domainStart, creationMs) : domainStart;
  const lineStart = Math.min(startBound, domainEnd);
  let times = union.filter((t) => t >= lineStart && t <= domainEnd);
  if (times[0] !== lineStart) times.unshift(lineStart);
  if (times[times.length - 1] !== domainEnd) times.push(domainEnd);

  if (times.length > MAX_POINTS) {
    const stride = Math.ceil(times.length / MAX_POINTS);
    const last = times[times.length - 1]!;
    times = times.filter((_, i) => i % stride === 0);
    if (times[times.length - 1] !== last) times.push(last);
  }

  // Forward-fill each lane onto the grid. An outcome with no series of its own
  // (a quiet market, or a sibling that hasn't cleared this session) holds its
  // current price flat — `0.5` only as a last resort when even that is unknown.
  // The grid may extend left of a lane's first point (back to the window edge);
  // there the lane holds its earliest known value flat (`lane[0].v`).
  const raw: number[][] = lanes.map((lane, k) => {
    if (lane.length === 0) {
      const flat = currentProbs[k] ?? 0.5;
      return times.map(() => flat);
    }
    const row: number[] = [];
    let cursor = 0;
    for (const t of times) {
      while (cursor + 1 < lane.length && lane[cursor + 1]!.t <= t) cursor++;
      const pt = lane[cursor]!;
      row.push(pt.t <= t ? pt.v : lane[0]!.v);
    }
    return row;
  });

  return { times, raw, hasData, domainStart, domainEnd };
}
