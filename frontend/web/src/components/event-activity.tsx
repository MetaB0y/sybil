"use client";

/**
 * EventActivity — bottom-of-page section with an Activity / Comments switcher.
 *
 * Activity (default): matched-volume bars for ALL of the event's outcomes —
 * the rail's BatchHero mini-bars grown up, reading per-market volume candles.
 * With all outcomes shown the bars stack per outcome, colored to match the
 * chart legend; filtering to one outcome switches to BatchHero's price-move
 * coloring. Each range/resolution pair sits far inside that resolution's
 * server retention (1m→30d, 5m→180d, 1h→forever), so `retention_min_bucket_ms`
 * can never clip these views.
 *
 * Comments: no backend yet — the tab renders faded and floats the same "soon"
 * cursor tooltip as the markets page's empty categories (CategoryTabs).
 */

import { useEffect, useRef, useState } from "react";
import { FloatingTooltip } from "@/components/floating-tooltip";
import { useSoonTooltip } from "@/components/soon-tooltip";
import { colorForOutcome } from "@/components/outcome-legend";
import {
  formatAge,
  formatCentsPrecise,
  formatCompactDollars,
  parseNanos,
} from "@/lib/format/nanos";
import {
  type EventOutcome,
  useEventGroup,
} from "@/lib/market-detail/use-event-group";
import {
  type PriceCandle,
  useEventCandles,
} from "@/lib/market-detail/use-event-candles";
import { selectLatestBlock, useStore } from "@/lib/store";
import { BLOCK_INTERVAL_MS } from "@/lib/constants";

type ActivityRange = "1H" | "6H" | "24H" | "ALL";

const RANGES: ActivityRange[] = ["1H", "6H", "24H", "ALL"];

const DEFAULT_RANGE: ActivityRange = "24H";

/**
 * Most bars we'll draw. Wider windows don't truncate — `buildBars` folds
 * consecutive buckets together until the count fits, so ALL always spans the
 * whole chain (at a coarser bucket) instead of silently dropping its tail.
 */
const MAX_BARS = 180;

type RangeCfg = {
  resolution: string;
  resolutionMs: number;
  /** Window length; null = every bucket the chain has (ALL). */
  windowMs: number | null;
  /** Where the time ticks sit across the plot, left → right. */
  tickFractions: number[];
};

const CANDLE_CFG: Record<ActivityRange, RangeCfg> = {
  "1H": {
    resolution: "1m",
    resolutionMs: 60_000,
    windowMs: 3_600_000,
    tickFractions: [0, 0.25, 0.5, 0.75, 1],
  },
  "6H": {
    resolution: "5m",
    resolutionMs: 300_000,
    windowMs: 21_600_000,
    tickFractions: [0, 1 / 3, 2 / 3, 1],
  },
  "24H": {
    resolution: "1h",
    resolutionMs: 3_600_000,
    windowMs: 86_400_000,
    tickFractions: [0, 0.25, 0.5, 0.75, 1],
  },
  ALL: {
    resolution: "1h",
    resolutionMs: 3_600_000,
    windowMs: null,
    tickFractions: [0, 0.25, 0.5, 0.75, 1],
  },
};

/** One outcome's slice of a bar. */
type BarSegment = {
  marketId: number;
  volNanos: bigint;
  /** Outcome's YES price at the bar's close, null when it never cleared. */
  yesNanos: bigint | null;
  /** YES move across the bar in percentage points, null when it never cleared. */
  ppChange: number | null;
};

type ActivityBar = {
  key: string;
  /** Tooltip's first line — the bar's bucket width ("1h bucket"). */
  title: string;
  /** Bar end timestamp (ms) for the "ended N ago" tooltip line. */
  endMs: number;
  totalVolNanos: bigint;
  /** Outcome order (favourite-first) — matches `colorForOutcome` indices. */
  segments: BarSegment[];
};

export function EventActivity({ marketId }: { marketId: number }) {
  const { group } = useEventGroup(marketId);
  const [range, setRange] = useState<ActivityRange>(DEFAULT_RANGE);
  // The range still on screen while `range`'s candles are in flight. Switching
  // ranges then blurs the old chart out and the new one in (as the outcome swap
  // does) instead of blanking to a spinner.
  const [prevRange, setPrevRange] = useState<ActivityRange | null>(null);
  const [selected, setSelected] = useState<number | null>(null);

  const outcomes = group?.outcomes ?? [];
  const marketIds = outcomes.map((o) => o.marketId);
  const enabled = marketIds.length > 0;

  // "Now" for ago-labels and the bucket grid — the newest committed batch, so
  // the section shares the page's block clock (never Date.now() in render).
  const nowMs = useStore(selectLatestBlock)?.timestamp_ms ?? 0;

  const cfg = CANDLE_CFG[range];
  const candles = useEventCandles(marketIds, cfg.resolution, enabled);

  // The outgoing range's candles, served straight from the query cache (the
  // fetch already happened) and parked as soon as the incoming range lands.
  const prevCfg = prevRange != null ? CANDLE_CFG[prevRange] : cfg;
  const prevCandles = useEventCandles(
    marketIds,
    prevCfg.resolution,
    enabled && prevRange != null && candles.isPending,
  );

  // Ranges that share a resolution (24H and ALL both ride 1h) are a cache hit,
  // so `isPending` is false and the swap is instant.
  const showPrev = candles.isPending && prevRange != null;
  const shownRange = showPrev ? prevRange : range;
  const shownCfg = showPrev ? prevCfg : cfg;
  const shownCandles = showPrev ? prevCandles : candles;

  // Cheap enough to derive every render (outcomes × buckets ≈ 10³ ops) — and
  // `byMarket` is a fresh Map each render, so a useMemo here would never hit.
  const bars = enabled
    ? buildBars(shownCandles.byMarket, outcomes, shownCfg, nowMs)
    : [];

  // A single binary market has nothing to filter — it IS the one outcome, and
  // single-outcome bars use price-move coloring.
  const isMulti = group?.isMultiOutcome === true;
  const selectedId = isMulti ? selected : (group?.currentMarketId ?? null);

  // Re-keying the chart on the drawn range restarts the focus-in animation.
  const swapKey = `${shownRange}:${selectedId ?? "all"}`;
  const swapping = showPrev && bars.length > 0;

  function pickRange(next: ActivityRange) {
    if (next === range) return;
    // Hold whatever is on screen right now, not the requested range — a second
    // click mid-load keeps the visible chart rather than an unloaded one.
    setPrevRange(shownRange);
    setRange(next);
  }

  const empty =
    !group || outcomes.length === 0
      ? "loading activity…"
      : bars.length === 0
        ? shownCandles.isPending
          ? "loading volume history…"
          : "no matched volume yet."
        : null;

  return (
    <section
      style={{
        padding: "var(--space-5)",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        boxShadow: "var(--shadow-inset-top)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-4)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: "var(--space-3)",
          flexWrap: "wrap",
        }}
      >
        <SectionTabs />
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: "var(--space-2)",
            flexWrap: "wrap",
          }}
        >
          {isMulti && (
            <OutcomeFilter
              outcomes={outcomes}
              selected={selected}
              onChange={setSelected}
            />
          )}
          <RangeTabs value={range} onChange={pickRange} />
        </div>
      </div>

      {empty != null ? (
        <Empty>{empty}</Empty>
      ) : (
        <div
          style={{
            filter: swapping ? "blur(5px)" : undefined,
            opacity: swapping ? 0.5 : 1,
            transition:
              "filter var(--dur-swap) var(--ease-standard), opacity var(--dur-swap) var(--ease-standard)",
          }}
        >
          <div
            key={swapKey}
            style={{
              animation:
                "sybil-fade-swap var(--dur-swap) var(--ease-standard)",
            }}
          >
            <VolumeBars
              bars={bars}
              outcomes={outcomes}
              selectedId={selectedId}
              spanMs={shownCfg.windowMs}
              tickFractions={shownCfg.tickFractions}
            />
            {selectedId == null && isMulti && <OutcomeKey outcomes={outcomes} />}
          </div>
        </div>
      )}
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Bar derivation                                                      */
/* ------------------------------------------------------------------ */

/** Human bucket width — "5m", "1h", "6h", "1d". */
function bucketLabel(ms: number): string {
  if (ms < 3_600_000) return `${Math.round(ms / 60_000)}m`;
  if (ms < 86_400_000) return `${Math.round(ms / 3_600_000)}h`;
  return `${Math.round(ms / 86_400_000)}d`;
}

/**
 * Bars on a contiguous bucket grid. Candles only exist for buckets a market
 * cleared in, so the grid is generated (windowed ranges count back from the
 * block clock; ALL starts at the earliest bucket seen) and misses read as zero
 * volume — a quiet hour is a real data point, not a gap.
 *
 * When the grid is longer than `MAX_BARS`, consecutive buckets fold together
 * by an integer factor: ALL keeps covering the whole chain, just at a coarser
 * bucket. Volume sums; price move spans the group's first open → last close.
 */
function buildBars(
  byMarket: Map<number, PriceCandle[]>,
  outcomes: EventOutcome[],
  cfg: RangeCfg,
  nowMs: number,
): ActivityBar[] {
  const indexed = new Map<number, Map<number, PriceCandle>>();
  let earliest = Number.POSITIVE_INFINITY;
  let latest = 0;
  for (const o of outcomes) {
    const idx = new Map<number, PriceCandle>();
    for (const c of byMarket.get(o.marketId) ?? []) {
      idx.set(c.bucket_start_ms, c);
      if (c.bucket_start_ms < earliest) earliest = c.bucket_start_ms;
      if (c.bucket_start_ms > latest) latest = c.bucket_start_ms;
    }
    indexed.set(o.marketId, idx);
  }
  if (latest === 0) return [];

  const res = cfg.resolutionMs;
  // Grid head: the newer of block clock / newest bucket, bucket-aligned —
  // candles can momentarily lead the store's latest block on a fresh load.
  const head = Math.floor(Math.max(nowMs, latest) / res) * res;
  const start =
    cfg.windowMs != null
      ? head - (Math.floor(cfg.windowMs / res) - 1) * res
      : Math.floor(earliest / res) * res;
  const bucketCount = Math.max(1, Math.floor((head - start) / res) + 1);
  const factor = Math.max(1, Math.ceil(bucketCount / MAX_BARS));
  const groupMs = res * factor;
  const groupCount = Math.ceil(bucketCount / factor);
  // Each bar spans `groupMs`, i.e. this many 10s batches — surfaced alongside
  // the bucket width so the tooltip reads "1h bucket · 360 batches".
  const barBatches = Math.max(1, Math.round(groupMs / BLOCK_INTERVAL_MS));
  const title = `${bucketLabel(groupMs)} bucket · ${barBatches} ${barBatches === 1 ? "batch" : "batches"}`;

  const bars: ActivityBar[] = [];
  for (let g = 0; g < groupCount; g++) {
    // Right-align the folding so the newest group always ends at `head`.
    const groupEndIdx = bucketCount - 1 - g * factor;
    const groupStartIdx = Math.max(0, groupEndIdx - factor + 1);
    let total = 0n;

    const segments = outcomes.map<BarSegment>((o) => {
      const idx = indexed.get(o.marketId);
      let vol = 0n;
      let firstOpen: bigint | null = null;
      let lastClose: bigint | null = null;
      for (let i = groupStartIdx; i <= groupEndIdx; i++) {
        const c = idx?.get(start + i * res);
        if (!c) continue;
        vol += parseNanos(c.volume_nanos);
        if (firstOpen == null) firstOpen = parseNanos(c.open_yes_price_nanos);
        lastClose = parseNanos(c.close_yes_price_nanos);
      }
      total += vol;
      return {
        marketId: o.marketId,
        volNanos: vol,
        yesNanos: lastClose,
        ppChange:
          firstOpen != null && lastClose != null
            ? Number(lastClose - firstOpen) / 1e7
            : null,
      };
    });

    bars.push({
      key: `c${start + groupEndIdx * res}`,
      title,
      endMs: start + groupEndIdx * res + res,
      totalVolNanos: total,
      segments,
    });
  }
  // Built newest-first above (so folding right-aligns); read left-to-right.
  return bars.reverse();
}

/* ------------------------------------------------------------------ */
/* Chart                                                               */
/* ------------------------------------------------------------------ */

const CHART_H = 132;
const Y_GUTTER = 46;
const MIN_BAR_H = 3;

/** ±N.N pp with an explicit sign (true minus glyph) — as BatchHero. */
function formatPp(pp: number): string {
  if (Math.abs(pp) < 0.05) return "0.0 pp";
  const sign = pp > 0 ? "+" : "−";
  return `${sign}${Math.abs(pp).toFixed(1)} pp`;
}

/** Green when YES rose, red when it fell, neutral otherwise — as BatchHero. */
function moveColor(ppChange: number | null): string {
  if (ppChange == null || Math.abs(ppChange) < 0.05)
    return "color-mix(in srgb, var(--fg-4) 45%, transparent)";
  return ppChange > 0 ? "var(--yes)" : "var(--no)";
}

/** Elapsed span as an axis label — "45m ago", "12h ago", "3d ago", "now". */
function formatSpanAgo(ms: number): string {
  if (ms < 60_000) return "now";
  const minutes = ms / 60_000;
  if (minutes < 60) return `${Math.round(minutes)}m ago`;
  const hours = ms / 3_600_000;
  if (hours < 48) {
    const rounded = hours < 10 ? Math.round(hours * 10) / 10 : Math.round(hours);
    return `${rounded}h ago`;
  }
  return `${Math.round(ms / 86_400_000)}d ago`;
}

function VolumeBars({
  bars,
  outcomes,
  selectedId,
  /** Fixed window length; null = ALL, where the span is whatever we drew. */
  spanMs,
  tickFractions,
}: {
  bars: ActivityBar[];
  outcomes: EventOutcome[];
  selectedId: number | null;
  spanMs: number | null;
  tickFractions: number[];
}) {
  const [hover, setHover] = useState<{ key: string; rect: DOMRect } | null>(
    null,
  );

  const colorOf = new Map(
    outcomes.map((o, i) => [o.marketId, colorForOutcome(o, i)]),
  );

  // Scale to the busiest bar in the window — of the filtered outcome when one
  // is selected, of the stacked total otherwise.
  const volOf = (bar: ActivityBar): bigint =>
    selectedId == null
      ? bar.totalVolNanos
      : (bar.segments.find((s) => s.marketId === selectedId)?.volNanos ?? 0n);
  let max = 0n;
  for (const bar of bars) {
    const v = volOf(bar);
    if (v > max) max = v;
  }

  // Bars thin out as the window widens; keep a hairline gap so they stay
  // individually pickable instead of merging into a filled area.
  const gap = bars.length <= 48 ? 3 : bars.length <= 100 ? 2 : 1;

  const first = bars[0];
  const last = bars[bars.length - 1];
  const axisSpanMs =
    spanMs ?? (first && last ? Math.max(0, last.endMs - first.endMs) : 0);

  return (
    <div>
      <div style={{ display: "flex" }}>
        {/* Y axis — peak and midpoint, so a bar's height reads as a $ number. */}
        <div
          style={{
            width: Y_GUTTER,
            height: CHART_H,
            position: "relative",
            flexShrink: 0,
            fontFamily: "var(--font-mono)",
            fontSize: 9,
            color: "var(--fg-4)",
            fontVariantNumeric: "tabular-nums",
          }}
        >
          {max > 0n && (
            <>
              <span style={{ position: "absolute", top: -4, right: 8 }}>
                {formatCompactDollars(max)}
              </span>
              <span
                style={{ position: "absolute", top: CHART_H / 2 - 5, right: 8 }}
              >
                {formatCompactDollars(max / 2n)}
              </span>
            </>
          )}
          <span style={{ position: "absolute", bottom: -5, right: 8 }}>$0</span>
        </div>

        <div style={{ flex: 1, minWidth: 0, position: "relative", height: CHART_H }}>
          {/* Gridlines at peak + midpoint, behind the bars. */}
          {max > 0n &&
            [0, 0.5].map((t) => (
              <div
                key={t}
                aria-hidden
                style={{
                  position: "absolute",
                  left: 0,
                  right: 0,
                  top: `${t * 100}%`,
                  borderTop: "1px dashed color-mix(in srgb, var(--fg-4) 18%, transparent)",
                }}
              />
            ))}
          <div
            aria-hidden
            style={{
              position: "absolute",
              left: 0,
              right: 0,
              bottom: 0,
              borderTop: "1px solid var(--border-1)",
            }}
          />

          <div
            style={{
              position: "absolute",
              inset: 0,
              display: "flex",
              alignItems: "flex-end",
              gap,
            }}
          >
            {bars.map((bar) => {
              const isHover = hover?.key === bar.key;
              const dimmed = hover != null && !isHover;
              const barVol = volOf(bar);
              const ratio = max === 0n ? 0 : Number((barVol * 1000n) / max) / 1000;
              const barH = Math.max(MIN_BAR_H, ratio * CHART_H);

              // Topmost-first stack: with one outcome selected a single segment
              // colored by price move; otherwise the reverse of outcome order so
              // the favourite sits on the baseline.
              const stack: { color: string; h: number }[] = [];
              if (barVol === 0n) {
                stack.push({
                  color: "color-mix(in srgb, var(--fg-4) 14%, transparent)",
                  h: barH,
                });
              } else if (selectedId != null) {
                const seg = bar.segments.find((s) => s.marketId === selectedId);
                stack.push({ color: moveColor(seg?.ppChange ?? null), h: barH });
              } else {
                for (const seg of bar.segments) {
                  if (seg.volNanos <= 0n) continue;
                  stack.unshift({
                    color: colorOf.get(seg.marketId) ?? "var(--fg-4)",
                    h: (Number((seg.volNanos * 1000n) / barVol) / 1000) * barH,
                  });
                }
              }

              return (
                <div
                  key={bar.key}
                  onMouseEnter={(e) => {
                    // Anchor the tooltip to the top of the plot area — the top of
                    // the tallest bar (ratio 1 → CHART_H) — so the readout always
                    // sits at that constant height and never overflows a bar,
                    // whichever bucket you hover.
                    const col = e.currentTarget.getBoundingClientRect();
                    setHover({
                      key: bar.key,
                      rect: new DOMRect(
                        col.left,
                        col.bottom - CHART_H,
                        col.width,
                        CHART_H,
                      ),
                    });
                  }}
                  onMouseLeave={() =>
                    setHover((cur) => (cur?.key === bar.key ? null : cur))
                  }
                  style={{
                    flex: "1 1 0",
                    minWidth: 0,
                    height: "100%",
                    display: "flex",
                    flexDirection: "column",
                    justifyContent: "flex-end",
                    position: "relative",
                    cursor: "default",
                    background: isHover
                      ? "color-mix(in srgb, var(--fg-4) 8%, transparent)"
                      : "transparent",
                    borderRadius: 2,
                    transition: "background 80ms linear",
                  }}
                >
                  {stack.map((seg, i) => (
                    <div
                      key={i}
                      style={{
                        width: "100%",
                        height: seg.h,
                        background: seg.color,
                        // The hovered bar reads at full strength while its
                        // neighbours recede — pointing at a bar isolates it.
                        opacity: isHover ? 1 : dimmed ? 0.3 : 0.85,
                        borderRadius: i === 0 ? "1px 1px 0 0" : 0,
                        transition: "opacity 80ms linear",
                      }}
                    />
                  ))}
                  {isHover && hover && (
                    <ActivityBarTooltip
                      bar={bar}
                      anchor={hover.rect}
                      outcomes={outcomes}
                      colorOf={colorOf}
                      selectedId={selectedId}
                    />
                  )}
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {/* Time axis — interim ticks so a bar's position reads as a moment. */}
      <div
        style={{
          marginLeft: Y_GUTTER,
          position: "relative",
          height: 18,
          fontFamily: "var(--font-mono)",
          fontSize: 9,
          color: "var(--fg-4)",
          textTransform: "uppercase",
          letterSpacing: "0.04em",
        }}
      >
        {tickFractions.map((f) => {
          const atStart = f === 0;
          const atEnd = f === 1;
          const label = atEnd ? "now" : formatSpanAgo(axisSpanMs * (1 - f));
          return (
            <div
              key={f}
              style={{
                position: "absolute",
                top: 0,
                ...(atStart
                  ? { left: 0 }
                  : atEnd
                    ? { right: 0 }
                    : { left: `${f * 100}%`, transform: "translateX(-50%)" }),
                display: "flex",
                flexDirection: "column",
                alignItems: atStart ? "flex-start" : atEnd ? "flex-end" : "center",
                gap: 3,
              }}
            >
              <span
                aria-hidden
                style={{
                  width: 1,
                  height: 3,
                  background: "color-mix(in srgb, var(--fg-4) 35%, transparent)",
                }}
              />
              <span style={{ whiteSpace: "nowrap" }}>{label}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

/** Tooltip rows shown per outcome before collapsing into "+N more". */
const TOOLTIP_OUTCOME_ROWS = 6;

function ActivityBarTooltip({
  bar,
  anchor,
  outcomes,
  colorOf,
  selectedId,
}: {
  bar: ActivityBar;
  anchor: DOMRect;
  outcomes: EventOutcome[];
  colorOf: Map<number, string>;
  selectedId: number | null;
}) {
  // Ago is measured against the newest bar's end, not the wall clock, so the
  // hovered bucket reads relative to the same block clock the chart is drawn on.
  const nowMs = useStore(selectLatestBlock)?.timestamp_ms ?? 0;
  const labelOf = new Map(outcomes.map((o) => [o.marketId, o.shortLabel]));
  const agoMs = nowMs > 0 ? Math.max(0, nowMs - bar.endMs) : null;
  const endedLabel =
    agoMs == null ? "—" : agoMs < 60_000 ? "just now" : `${formatAge(agoMs)} ago`;

  const selected =
    selectedId == null
      ? null
      : (bar.segments.find((s) => s.marketId === selectedId) ?? null);
  const active =
    selectedId == null
      ? bar.segments
          .filter((s) => s.volNanos > 0n)
          .sort((a, b) => (a.volNanos < b.volNanos ? 1 : -1))
      : [];
  const shown = active.slice(0, TOOLTIP_OUTCOME_ROWS);
  const overflow = active.length - shown.length;

  const ppColor =
    selected?.ppChange == null || Math.abs(selected.ppChange) < 0.05
      ? "var(--fg-3)"
      : selected.ppChange > 0
        ? "var(--yes)"
        : "var(--no)";

  const kv = (
    label: React.ReactNode,
    value: React.ReactNode,
    valueColor = "var(--fg-1)",
  ) => (
    <div style={{ display: "flex", justifyContent: "space-between", gap: 12 }}>
      <span style={{ color: "var(--fg-3)" }}>{label}</span>
      <span style={{ color: valueColor }}>{value}</span>
    </div>
  );

  const estHeight = 58 + (selected ? 34 : shown.length * 16 + (overflow > 0 ? 14 : 0));

  return (
    <FloatingTooltip
      anchor={anchor}
      width={206}
      align="center"
      estHeight={estHeight}
      // The readout floats over neighbouring columns, so it must swallow the
      // pointer rather than let it through: otherwise moving onto the tooltip
      // hovers the bar beneath it and the readout jumps away mid-read. React
      // computes enter/leave across the portal via the React tree, and this
      // tooltip renders inside the hovered column — so capturing the pointer
      // holds that column's hover instead of switching bars.
      style={{ pointerEvents: "auto", cursor: "default" }}
    >
      <div
        style={{
          whiteSpace: "nowrap",
          background: "var(--surface-3)",
          border: "1px solid var(--border-2)",
          borderRadius: 6,
          boxShadow: "var(--shadow-popover)",
          padding: "7px 9px",
          display: "flex",
          flexDirection: "column",
          gap: 3,
          fontFamily: "var(--font-mono)",
          fontSize: 10,
          fontVariantNumeric: "tabular-nums",
          animation: "sybil-fade-in var(--dur-fast) var(--ease-standard)",
        }}
      >
        <span style={{ color: "var(--fg-3)" }}>{bar.title}</span>
        {kv("ended", endedLabel)}
        {kv(
          "matched vol",
          formatCompactDollars(selected ? selected.volNanos : bar.totalVolNanos),
        )}
        {selected != null && (
          <>
            {kv(
              "price",
              selected.yesNanos == null
                ? "no clear"
                : formatCentsPrecise(selected.yesNanos),
            )}
            {kv(
              "price move",
              selected.yesNanos == null
                ? "no clear"
                : selected.ppChange == null
                  ? "—"
                  : formatPp(selected.ppChange),
              ppColor,
            )}
          </>
        )}
        {shown.length > 0 && (
          <div
            aria-hidden
            style={{
              height: 1,
              background: "var(--border-1)",
              margin: "2px 0 1px",
            }}
          />
        )}
        {shown.map((seg) => (
          <div
            key={seg.marketId}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              justifyContent: "space-between",
            }}
          >
            <span
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 5,
                minWidth: 0,
              }}
            >
              <span
                aria-hidden
                style={{
                  width: 6,
                  height: 6,
                  borderRadius: "50%",
                  background: colorOf.get(seg.marketId) ?? "var(--fg-4)",
                  flexShrink: 0,
                }}
              />
              <span
                style={{
                  color: "var(--fg-2)",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  maxWidth: 108,
                }}
              >
                {labelOf.get(seg.marketId) ?? seg.marketId}
              </span>
            </span>
            <span style={{ color: "var(--fg-1)" }}>
              {formatCompactDollars(seg.volNanos)}
            </span>
          </div>
        ))}
        {overflow > 0 && (
          <span style={{ color: "var(--fg-4)" }}>+{overflow} more</span>
        )}
      </div>
    </FloatingTooltip>
  );
}

/* ------------------------------------------------------------------ */
/* Controls                                                            */
/* ------------------------------------------------------------------ */

/**
 * Activity / Comments switcher — ViewSwitcher's segmented track, but the
 * Comments tab has no backend yet: it renders faded, non-interactive, and
 * floats the markets page's cursor-tracking "soon" tooltip (CategoryTabs).
 * When comments land, replace the faded button with a real tab + state.
 */
function SectionTabs() {
  const { hovered, handlers, tooltip } = useSoonTooltip();

  const base: React.CSSProperties = {
    padding: "4px 10px",
    border: 0,
    borderRadius: 3,
    fontFamily: "var(--font-mono)",
    fontSize: 11,
    textTransform: "uppercase",
    letterSpacing: "var(--track-wide)",
    transition: "background 120ms, color 120ms, opacity 120ms",
  };

  return (
    <>
      <div
        style={{
          display: "inline-flex",
          background: "var(--bg-2)",
          border: "1px solid var(--border-1)",
          borderRadius: 4,
          padding: 2,
          gap: 2,
        }}
      >
        <button
          type="button"
          style={{
            ...base,
            background: "var(--surface-2)",
            color: "var(--fg-1)",
            cursor: "pointer",
          }}
        >
          activity
        </button>
        <button
          type="button"
          aria-disabled
          tabIndex={-1}
          {...handlers}
          style={{
            ...base,
            background: "transparent",
            color: hovered ? "var(--fg-3)" : "var(--fg-4)",
            opacity: hovered ? 0.85 : 0.5,
            cursor: "default",
          }}
        >
          comments
        </button>
      </div>
      {tooltip}
    </>
  );
}

/** Range selector — ChartRangeBar's track with the activity ranges. */
function RangeTabs({
  value,
  onChange,
}: {
  value: ActivityRange;
  onChange: (r: ActivityRange) => void;
}) {
  return (
    <div
      style={{
        display: "inline-flex",
        gap: 2,
        padding: 2,
        background: "var(--bg-2)",
        border: "1px solid var(--border-1)",
        borderRadius: 4,
      }}
    >
      {RANGES.map((r) => {
        const active = value === r;
        return (
          <button
            key={r}
            type="button"
            onClick={() => onChange(r)}
            style={{
              padding: "4px 9px",
              borderRadius: 3,
              border: 0,
              cursor: "pointer",
              background: active ? "var(--surface-2)" : "transparent",
              color: active ? "var(--fg-1)" : "var(--fg-3)",
              fontFamily: "var(--font-mono)",
              fontSize: 10,
              letterSpacing: "0.04em",
            }}
          >
            {r}
          </button>
        );
      })}
    </div>
  );
}

/** Static color key under the stacked chart — dots match `colorForOutcome`. */
function OutcomeKey({ outcomes }: { outcomes: EventOutcome[] }) {
  return (
    <div
      style={{
        display: "flex",
        flexWrap: "wrap",
        gap: "4px 14px",
        marginTop: "var(--space-3)",
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        color: "var(--fg-3)",
      }}
    >
      {outcomes.map((o, i) => (
        <span
          key={o.marketId}
          title={o.label}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 5,
            minWidth: 0,
          }}
        >
          <span
            aria-hidden
            style={{
              width: 7,
              height: 7,
              borderRadius: "50%",
              background: colorForOutcome(o, i),
              flexShrink: 0,
            }}
          />
          <span
            style={{
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              maxWidth: 140,
            }}
          >
            {o.shortLabel}
          </span>
        </span>
      ))}
    </div>
  );
}

/**
 * Outcome filter — compact "All outcomes + per-outcome dot" dropdown.
 * Mirrors the private OutcomeFilter in `event-holdings.tsx` (duplicated
 * rather than lifted while that file is mid-flight on a parallel branch —
 * keep visual changes in sync).
 */
function OutcomeFilter({
  outcomes,
  selected,
  onChange,
}: {
  outcomes: EventOutcome[];
  selected: number | null;
  onChange: (id: number | null) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    function close(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", onKey);
    };
  }, []);

  const selectedIndex = outcomes.findIndex((o) => o.marketId === selected);
  const selectedOutcome = selectedIndex >= 0 ? outcomes[selectedIndex] : null;
  const selectedColor =
    selectedOutcome != null
      ? colorForOutcome(selectedOutcome, selectedIndex)
      : null;

  function pick(id: number | null) {
    setOpen(false);
    onChange(id);
  }

  return (
    <div ref={ref} style={{ position: "relative" }}>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-haspopup="listbox"
        aria-expanded={open}
        title="Filter by outcome"
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 6,
          maxWidth: 200,
          padding: "4px 8px",
          borderRadius: 4,
          background: "var(--bg-2)",
          border: "1px solid var(--border-1)",
          cursor: "pointer",
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          letterSpacing: "var(--track-wide)",
          color: "var(--fg-2)",
        }}
      >
        {selectedColor != null && (
          <span
            aria-hidden
            style={{
              width: 7,
              height: 7,
              borderRadius: "50%",
              background: selectedColor,
              flexShrink: 0,
            }}
          />
        )}
        <span
          style={{
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {selectedOutcome != null ? selectedOutcome.shortLabel : "All outcomes"}
        </span>
        <svg
          aria-hidden
          width="10"
          height="10"
          viewBox="0 0 12 12"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          style={{
            flexShrink: 0,
            transform: open ? "rotate(180deg)" : "none",
            transition: "transform 120ms",
          }}
        >
          <path d="m3 4.5 3 3 3-3" />
        </svg>
      </button>

      {open && (
        <div
          role="listbox"
          style={{
            position: "absolute",
            top: "calc(100% + 4px)",
            right: 0,
            zIndex: 30,
            minWidth: 200,
            background: "var(--surface-2)",
            border: "1px solid var(--border-2)",
            borderRadius: 6,
            padding: 4,
            boxShadow: "var(--shadow-popover, 0 8px 24px rgba(0,0,0,0.4))",
            display: "flex",
            flexDirection: "column",
            gap: 2,
            maxHeight: 280,
            overflowY: "auto",
          }}
        >
          <OutcomeOption
            label="All outcomes"
            selected={selected == null}
            onClick={() => pick(null)}
          />
          {outcomes.map((o, i) => (
            <OutcomeOption
              key={o.marketId}
              label={o.shortLabel}
              title={o.label}
              color={colorForOutcome(o, i)}
              selected={selected === o.marketId}
              onClick={() => pick(o.marketId)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function OutcomeOption({
  label,
  title,
  color,
  selected,
  onClick,
}: {
  label: string;
  title?: string;
  color?: string;
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="option"
      aria-selected={selected}
      onClick={onClick}
      title={title ?? label}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "7px 10px",
        borderRadius: 4,
        background: selected ? "var(--bg-2)" : "transparent",
        border: 0,
        cursor: "pointer",
        textAlign: "left",
        width: "100%",
      }}
      onMouseEnter={(e) => {
        if (!selected) e.currentTarget.style.background = "var(--bg-2)";
      }}
      onMouseLeave={(e) => {
        if (!selected) e.currentTarget.style.background = "transparent";
      }}
    >
      <span
        aria-hidden
        style={{
          width: 8,
          height: 8,
          borderRadius: "50%",
          background: color ?? "var(--fg-4)",
          flexShrink: 0,
        }}
      />
      <span
        style={{
          flex: 1,
          minWidth: 0,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          fontFamily: "var(--font-sans)",
          fontSize: 13,
          color: "var(--fg-1)",
        }}
      >
        {label}
      </span>
    </button>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        padding: "48px 0",
        color: "var(--fg-4)",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        letterSpacing: "var(--track-wide)",
        textAlign: "center",
      }}
    >
      {children}
    </div>
  );
}
