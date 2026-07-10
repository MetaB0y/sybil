"use client";

/**
 * EventActivity — bottom-of-page section with an Activity / Comments switcher.
 *
 * Activity (default): matched-volume bars for ALL of the event's outcomes —
 * the rail's BatchHero mini-bars grown up. Batch granularity ("24B") reads the
 * live block ring (`by_market` volume + clearing price per outcome, REST
 * backfill like the activity page); longer ranges read per-market volume
 * candles. With all outcomes shown the bars stack per outcome, colored to
 * match the chart legend; filtering to one outcome switches to BatchHero's
 * price-move coloring. Range ↔ resolution pairs sit far inside each
 * resolution's server retention (1m→30d, 5m→180d, 1h→forever), so
 * `retention_min_bucket_ms` can never clip these views.
 *
 * Comments: no backend yet — the tab renders faded and floats the same "soon"
 * cursor tooltip as the markets page's empty categories (CategoryTabs).
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { FloatingTooltip } from "@/components/floating-tooltip";
import { colorForOutcome } from "@/components/outcome-legend";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";
import {
  formatAge,
  formatCentsPrecise,
  formatCompactDollars,
  formatInt,
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
import { selectLatestBlock, selectRecentBlocks, useStore } from "@/lib/store";

type BlockResponse = components["schemas"]["BlockResponse"];

type ActivityRange = "24B" | "1H" | "6H" | "24H" | "ALL";

const RANGES: ActivityRange[] = ["24B", "1H", "6H", "24H", "ALL"];

/** Bars in the batch-granularity view — matches the rail hero's window. */
const BATCH_BAR_COUNT = 24;

/** Hard cap on rendered bars (ALL grows with the chain's age). */
const MAX_BARS = 500;

/** Candle resolution per range — the finest that keeps the bar count sane. */
const CANDLE_CFG: Record<
  Exclude<ActivityRange, "24B">,
  { resolution: string; resolutionMs: number; windowMs: number | null }
> = {
  "1H": { resolution: "1m", resolutionMs: 60_000, windowMs: 3_600_000 },
  "6H": { resolution: "5m", resolutionMs: 300_000, windowMs: 21_600_000 },
  "24H": { resolution: "1h", resolutionMs: 3_600_000, windowMs: 86_400_000 },
  ALL: { resolution: "1h", resolutionMs: 3_600_000, windowMs: null },
};

/** One outcome's slice of a bar. */
type BarSegment = {
  marketId: number;
  volNanos: bigint;
  /** Outcome's YES price at the end of the bar, null when it didn't clear. */
  yesNanos: bigint | null;
  /** YES move across the bar in percentage points, null when unknown. */
  ppChange: number | null;
};

type ActivityBar = {
  key: string;
  /** First tooltip line — "batch #N" or the candle bucket length. */
  title: string;
  /** How the end-time line is labelled — batches "settled", buckets "ended". */
  endedVerb: string;
  /** Bar end timestamp (ms) for the "ended N ago" tooltip line. */
  endMs: number;
  totalVolNanos: bigint;
  /** Outcome order (favourite-first) — matches `colorForOutcome` indices. */
  segments: BarSegment[];
};

export function EventActivity({ marketId }: { marketId: number }) {
  const { group } = useEventGroup(marketId);
  const [range, setRange] = useState<ActivityRange>("24B");
  const [selected, setSelected] = useState<number | null>(null);

  const outcomes = useMemo(() => group?.outcomes ?? [], [group]);
  const marketIds = useMemo(
    () => outcomes.map((o) => o.marketId),
    [outcomes],
  );

  const recentBlocks = useStore(selectRecentBlocks);
  const latestBlock = useStore(selectLatestBlock);

  // One-shot REST backfill of the block ring, same rationale as use-batches:
  // WS replay is all-or-nothing, REST clamps to whatever the server holds and
  // merges cleanly with the live tail via applyBlocks' dedupe.
  const backfilled = useRef(false);
  const [backfilling, setBackfilling] = useState(true);
  useEffect(() => {
    if (backfilled.current) return;
    backfilled.current = true;
    let cancelled = false;
    (async () => {
      try {
        const { data, error } = await api.GET("/v1/blocks", {
          params: { query: { limit: BATCH_BAR_COUNT } },
        });
        if (cancelled || error || !data || data.length === 0) return;
        useStore.getState().applyBlocks(data);
      } finally {
        if (!cancelled) setBackfilling(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const candleCfg = range === "24B" ? null : CANDLE_CFG[range];
  const candles = useEventCandles(
    marketIds,
    candleCfg?.resolution ?? "1h",
    candleCfg != null && marketIds.length > 0,
  );

  // "Now" for ago-labels and bucket padding — the newest committed batch, so
  // the section shares the page's block clock (never Date.now() in render).
  const nowMs = latestBlock?.timestamp_ms ?? 0;

  const bars = useMemo<ActivityBar[]>(() => {
    if (outcomes.length === 0) return [];
    return candleCfg == null
      ? buildBatchBars(recentBlocks, outcomes)
      : buildCandleBars(candles.byMarket, outcomes, candleCfg, nowMs);
  }, [candleCfg, candles.byMarket, nowMs, outcomes, recentBlocks]);

  // A single binary market has nothing to filter — it IS the one outcome, and
  // single-outcome bars use price-move coloring.
  const isMulti = group?.isMultiOutcome === true;
  const selectedId = isMulti ? selected : (group?.currentMarketId ?? null);

  const empty =
    !group || outcomes.length === 0
      ? "loading activity…"
      : candleCfg == null
        ? bars.length === 0
          ? backfilling
            ? "loading batches…"
            : "waiting for batches…"
          : null
        : bars.length === 0
          ? candles.isPending
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
        gap: "var(--space-3)",
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
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: "var(--space-3)",
            minWidth: 0,
            flexWrap: "wrap",
          }}
        >
          <div className="eyebrow">activity</div>
          {isMulti && (
            <OutcomeFilter
              outcomes={outcomes}
              selected={selected}
              onChange={setSelected}
            />
          )}
        </div>
        <SectionTabs />
      </div>

      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: "var(--space-3)",
          flexWrap: "wrap",
        }}
      >
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 9,
            color: "var(--fg-4)",
            textTransform: "uppercase",
            letterSpacing: "0.04em",
          }}
        >
          {candleCfg == null
            ? `last ${BATCH_BAR_COUNT} batches`
            : range === "ALL"
              ? `all time · ${candleCfg.resolution} buckets`
              : `last ${range.toLowerCase()} · ${candleCfg.resolution} buckets`}
          {selectedId != null
            ? " · matched vol · price move"
            : " · matched vol by outcome"}
        </span>
        <RangeTabs value={range} onChange={setRange} />
      </div>

      {empty != null ? (
        <Empty>{empty}</Empty>
      ) : (
        <VolumeBars
          bars={bars}
          outcomes={outcomes}
          selectedId={selectedId}
          padTo={candleCfg == null ? BATCH_BAR_COUNT : 0}
          nowMs={nowMs}
        />
      )}

      {selectedId == null && isMulti && empty == null && (
        <OutcomeKey outcomes={outcomes} />
      )}
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Bar derivation                                                      */
/* ------------------------------------------------------------------ */

/**
 * Batch bars from the block ring — BatchHero's derivation widened to every
 * outcome. Price move compares against the previous batch that actually
 * cleared that outcome (batches skip a market with no crossing orders).
 */
function buildBatchBars(
  blocks: BlockResponse[],
  outcomes: EventOutcome[],
): ActivityBar[] {
  const window = [...blocks.slice(0, BATCH_BAR_COUNT)].reverse();
  const prevYes = new Map<number, bigint>();
  return window.map((b) => {
    let total = 0n;
    const segments = outcomes.map((o) => {
      const key = String(o.marketId);
      const rawYes = b.clearing_prices_nanos?.[key]?.[0];
      const yesNanos = rawYes == null ? null : parseNanos(rawYes);
      const volNanos = parseNanos(b.by_market?.[key]?.volume_nanos ?? 0);
      total += volNanos;
      let ppChange: number | null = null;
      if (yesNanos != null) {
        const prev = prevYes.get(o.marketId);
        if (prev != null) ppChange = Number(yesNanos - prev) / 1e7;
        prevYes.set(o.marketId, yesNanos);
      }
      return { marketId: o.marketId, volNanos, yesNanos, ppChange };
    });
    return {
      key: `b${b.height}`,
      title: `batch #${formatInt(b.height)}`,
      endedVerb: "settled",
      endMs: b.timestamp_ms ?? 0,
      totalVolNanos: total,
      segments,
    };
  });
}

/**
 * Candle bars on a contiguous bucket grid. Candles only exist for buckets a
 * market cleared in, so the grid is generated (window ranges from `nowMs`
 * back; ALL from the earliest bucket seen) and misses read as zero volume —
 * a quiet hour is a real data point, not a gap. Price move is the bucket's
 * own open→close.
 */
function buildCandleBars(
  byMarket: Map<number, PriceCandle[]>,
  outcomes: EventOutcome[],
  cfg: { resolution: string; resolutionMs: number; windowMs: number | null },
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
  // Grid head: the newest of block-clock / newest bucket, bucket-aligned —
  // candles can momentarily lead the store's latest block on a fresh load.
  const head = Math.floor(Math.max(nowMs, latest) / res) * res;
  const start =
    cfg.windowMs != null
      ? head - (Math.floor(cfg.windowMs / res) - 1) * res
      : earliest;
  const count = Math.min(MAX_BARS, Math.floor((head - start) / res) + 1);

  const bars: ActivityBar[] = [];
  for (let i = count - 1; i >= 0; i--) {
    const bucketStart = head - i * res;
    let total = 0n;
    const segments = outcomes.map((o) => {
      const c = indexed.get(o.marketId)?.get(bucketStart);
      const volNanos = c ? parseNanos(c.volume_nanos) : 0n;
      total += volNanos;
      const yesNanos = c ? parseNanos(c.close_yes_price_nanos) : null;
      const ppChange = c
        ? Number(
            parseNanos(c.close_yes_price_nanos) -
              parseNanos(c.open_yes_price_nanos),
          ) / 1e7
        : null;
      return { marketId: o.marketId, volNanos, yesNanos, ppChange };
    });
    bars.push({
      key: `c${bucketStart}`,
      title: `${cfg.resolution} bucket`,
      endedVerb: "ended",
      endMs: bucketStart + res,
      totalVolNanos: total,
      segments,
    });
  }
  return bars;
}

/* ------------------------------------------------------------------ */
/* Chart                                                               */
/* ------------------------------------------------------------------ */

const CHART_H = 120;
const BAR_MAX_H = CHART_H - 8;

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

function VolumeBars({
  bars,
  outcomes,
  selectedId,
  /** Left-pad with placeholder stubs up to this many bars (batch mode). */
  padTo,
  nowMs,
}: {
  bars: ActivityBar[];
  outcomes: EventOutcome[];
  selectedId: number | null;
  padTo: number;
  nowMs: number;
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

  const padCount = Math.max(0, padTo - bars.length);
  const oldest = bars[0];

  return (
    <div>
      <div
        style={{
          display: "flex",
          alignItems: "flex-end",
          gap: 2,
          height: CHART_H,
          position: "relative",
          borderBottom: "1px solid var(--border-1)",
        }}
      >
        {Array.from({ length: padCount }).map((_, i) => (
          <div
            key={`empty-${i}`}
            style={{
              flex: "1 1 0",
              alignSelf: "flex-end",
              height: 3,
              background: "color-mix(in srgb, var(--fg-4) 12%, transparent)",
              borderRadius: 1,
            }}
          />
        ))}
        {bars.map((bar) => {
          const isHover = hover?.key === bar.key;
          const barVol = volOf(bar);
          const ratio =
            max === 0n ? 0 : Number((barVol * 1000n) / max) / 1000;
          const barH = Math.max(3, ratio * BAR_MAX_H);

          // Topmost-first stack: with one outcome selected a single segment
          // colored by price move; otherwise the reverse of outcome order so
          // the favourite sits on the baseline.
          const stack: { color: string; h: number }[] = [];
          if (selectedId != null) {
            const seg = bar.segments.find((s) => s.marketId === selectedId);
            stack.push({ color: moveColor(seg?.ppChange ?? null), h: barH });
          } else if (barVol > 0n) {
            for (const seg of bar.segments) {
              if (seg.volNanos <= 0n) continue;
              stack.unshift({
                color: colorOf.get(seg.marketId) ?? "var(--fg-4)",
                h: Number((seg.volNanos * 1000n) / barVol) / 1000 * barH,
              });
            }
          } else {
            stack.push({ color: moveColor(null), h: barH });
          }

          return (
            <div
              key={bar.key}
              onMouseEnter={(e) =>
                setHover({
                  key: bar.key,
                  rect: e.currentTarget.getBoundingClientRect(),
                })
              }
              onMouseLeave={() =>
                setHover((cur) => (cur?.key === bar.key ? null : cur))
              }
              style={{
                flex: "1 1 0",
                height: "100%",
                display: "flex",
                flexDirection: "column",
                justifyContent: "flex-end",
                position: "relative",
                cursor: "default",
              }}
            >
              {stack.map((seg, i) => (
                <div
                  key={i}
                  style={{
                    width: "100%",
                    height: seg.h,
                    background: seg.color,
                    opacity: isHover ? 1 : 0.8,
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
                  endedAgoMs={nowMs > 0 ? Math.max(0, nowMs - bar.endMs) : null}
                />
              )}
            </div>
          );
        })}
      </div>
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          marginTop: 4,
          fontFamily: "var(--font-mono)",
          fontSize: 9,
          color: "var(--fg-4)",
          textTransform: "uppercase",
          letterSpacing: "0.04em",
        }}
      >
        <span>
          {oldest && nowMs > 0
            ? `${formatAge(Math.max(0, nowMs - oldest.endMs))} ago`
            : ""}
        </span>
        <span>{max > 0n ? `peak ${formatCompactDollars(max)}` : ""}</span>
        <span>now</span>
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
  endedAgoMs,
}: {
  bar: ActivityBar;
  anchor: DOMRect;
  outcomes: EventOutcome[];
  colorOf: Map<number, string>;
  selectedId: number | null;
  endedAgoMs: number | null;
}) {
  const labelOf = new Map(outcomes.map((o) => [o.marketId, o.shortLabel]));
  const endedLabel =
    endedAgoMs == null
      ? "—"
      : endedAgoMs < 5000
        ? "just now"
        : `${formatAge(endedAgoMs)} ago`;

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

  return (
    <FloatingTooltip anchor={anchor} width={210} align="center" estHeight={140}>
      <div
        style={{
          whiteSpace: "nowrap",
          background: "var(--surface-3)",
          border: "1px solid var(--border-1)",
          borderRadius: 6,
          boxShadow: "var(--shadow-popover)",
          padding: "6px 8px",
          display: "flex",
          flexDirection: "column",
          gap: 3,
          fontFamily: "var(--font-mono)",
          fontSize: 10,
          fontVariantNumeric: "tabular-nums",
        }}
      >
        <span style={{ color: "var(--fg-3)" }}>{bar.title}</span>
        {kv(bar.endedVerb, endedLabel)}
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
                  maxWidth: 110,
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
  const [hovered, setHovered] = useState(false);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  const trackCursor = (e: React.MouseEvent) =>
    setPos({ x: e.clientX, y: e.clientY });

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
          onMouseEnter={(e) => {
            setHovered(true);
            trackCursor(e);
          }}
          onMouseMove={trackCursor}
          onMouseLeave={() => {
            setHovered(false);
            setPos(null);
          }}
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
      {hovered && pos && typeof document !== "undefined"
        ? createPortal(<SoonTooltip x={pos.x} y={pos.y} />, document.body)
        : null}
    </>
  );
}

/** Floating "soon" hint anchored just above the cursor — as CategoryTabs. */
function SoonTooltip({ x, y }: { x: number; y: number }) {
  return (
    <div
      role="tooltip"
      aria-hidden
      style={{
        position: "fixed",
        left: x,
        top: y - 14,
        transform: "translate(-50%, -100%)",
        pointerEvents: "none",
        zIndex: 100,
        padding: "3px 7px",
        background: "var(--surface-2)",
        border: "1px solid var(--border-2)",
        borderRadius: "var(--radius-sm)",
        boxShadow: "0 6px 18px rgba(0,0,0,0.35)",
        fontFamily: "var(--font-mono)",
        fontSize: "9px",
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
        color: "var(--accent)",
        whiteSpace: "nowrap",
        animation: "sybil-tooltip-in var(--dur-fast) var(--ease-standard)",
      }}
    >
      soon
    </div>
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
            title={r === "24B" ? `last ${BATCH_BAR_COUNT} batches` : undefined}
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
            left: 0,
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
        padding: "24px 0",
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
