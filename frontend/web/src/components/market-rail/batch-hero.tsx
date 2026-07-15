"use client";

/**
 * Pro-mode hero card — the showpiece of the rail. Big circular countdown
 * gauge + batch # + "N traders joined" + last-price / indicative-volume /
 * liquidity stats + last-24-batches mini bars. Matches the inline hero block in `V2BatchTheater` ProRail
 * (`fed-variations.jsx:128`).
 *
 * All values are real:
 *  - circular countdown progress + batch number
 *  - traders in this batch — polled open-batch unique placers
 *  - last price — most recent batch where this market actually traded
 *  - indicative volume — polled open-batch shadow-solve
 *  - liquidity — average near-price resting depth per batch
 *  - past-batch bar heights (matched volume)
 */

import { useState } from "react";
import { FloatingTooltip } from "@/components/floating-tooltip";
import { Glossary } from "@/components/glossary";
import {
  formatAge,
  formatBatchSeconds,
  formatCentsPrecise,
  formatCompactDollars,
  formatInt,
  parseNanos,
} from "@/lib/format/nanos";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";
import { avgLiquidityNanos } from "@/lib/markets/liquidity";
import {
  useOpenBatchLive,
  type OpenBatchLive,
  type OpenBatchReadState,
} from "@/lib/market-detail/use-open-batch-live";
import { selectConnection, selectRecentBlocks, useStore } from "@/lib/store";
import { useBatchCountdown } from "./use-batch-countdown";

const HERO_BATCH_COUNT = 24;

export function BatchHero({ outcome }: { outcome: EventOutcome }) {
  const { progress01, secondsLeftPrecise, latestHeight } = useBatchCountdown();
  const openBatch = useOpenBatchLive(outcome.marketId);
  const live = openBatch.data;
  const recent = useStore(selectRecentBlocks);
  const connection = useStore(selectConnection);

  const batchNumber = latestHeight == null ? null : latestHeight + 1;
  const placers = live?.uniquePlacers ?? null;
  const lastPriceNanos =
    lastTradedYesNanos(outcome.marketId, recent) ?? outcome.yesPriceNanos;
  const liquidityNanos = avgLiquidityNanos(outcome.liquidityNanos);

  // Honest connection pill: only claim a "live batch" when the block stream is
  // actually connected. If it's reconnecting/failed the countdown freezes at
  // 0.0s, so a hardcoded green "live" pill would read as a hang — surface the
  // real state instead.
  const wsState = connection.state;
  const isStreamLive = wsState === "live" || wsState === "replaying";
  const pill = isStreamLive
    ? { label: "live batch", color: "var(--accent)" }
    : wsState === "reconnecting" || wsState === "connecting"
      ? { label: "reconnecting…", color: "var(--warn)" }
      : { label: "stream offline", color: "var(--warn)" };

  return (
    <div
      data-testid="batch-hero"
      style={{
        background:
          "linear-gradient(180deg, color-mix(in srgb, var(--accent) 10%, transparent), color-mix(in srgb, var(--accent) 2%, transparent))",
        border: "1px solid color-mix(in srgb, var(--accent) 30%, transparent)",
        borderRadius: 8,
        padding: "20px 22px",
        position: "relative",
      }}
    >
      <div
        style={{
          position: "absolute",
          top: 14,
          right: 16,
          display: "inline-flex",
          alignItems: "center",
          gap: 5,
          fontFamily: "var(--font-mono)",
          fontSize: 9.5,
          color: pill.color,
          textTransform: "uppercase",
          letterSpacing: "0.06em",
        }}
      >
        <span
          aria-hidden
          style={{
            width: 6,
            height: 6,
            borderRadius: "50%",
            background: pill.color,
            boxShadow: isStreamLive ? `0 0 6px ${pill.color}` : "none",
            animation: isStreamLive
              ? "none"
              : "sybil-pulse 1.6s ease-in-out infinite",
          }}
        />
        {pill.label}
      </div>

      {/* Hero clock: large circular gauge + label block */}
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        <CircularCountdown
          progress01={progress01}
          countdown={formatBatchSeconds(secondsLeftPrecise)}
        />
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 4,
            minWidth: 0,
          }}
        >
          <div
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 10,
              color: "var(--fg-3)",
              textTransform: "uppercase",
              letterSpacing: "0.06em",
            }}
          >
            <Glossary term="Batch">next batch clears in</Glossary>
          </div>
          <div
            style={{
              fontFamily: "var(--font-sans)",
              fontSize: 15,
              fontWeight: 600,
              color: "var(--fg-1)",
              fontVariantNumeric: "tabular-nums",
            }}
          >
            batch #{batchNumber == null ? "—" : batchNumber.toLocaleString()}
          </div>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              fontFamily: "var(--font-mono)",
              fontSize: 11,
              color: "var(--fg-3)",
              fontVariantNumeric: "tabular-nums",
            }}
          >
            <span
              style={{
                width: 5,
                height: 5,
                borderRadius: "50%",
                background: "var(--yes)",
                boxShadow: "0 0 6px var(--yes)",
              }}
            />
            <span style={{ color: "var(--fg-1)", fontWeight: 600 }}>
              {placers ?? "—"}
            </span>
            <span>{placers === 1 ? "trader" : "traders"} in this batch</span>
          </div>
        </div>
      </div>

      <div
        style={{
          height: 1,
          background: "var(--border-1)",
          margin: "18px 0 14px",
        }}
      />

      <OpenBatchReadNotice
        state={openBatch.readState}
        retrying={openBatch.isRetrying}
        onRetry={openBatch.retry}
      />

      {/* Live batch stats */}
      <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        <SubStat
          label={<Glossary term="Last price">last price</Glossary>}
          secondary={`for ${outcome.shortLabel}`}
          value={
            lastPriceNanos == null ? "—" : formatCentsPrecise(lastPriceNanos)
          }
          valueColor="var(--yes)"
        />
        <SubStat
          label={<Glossary term="IEV">indicative volume</Glossary>}
          secondary="would clear this batch"
          value={
            live == null
              ? "—"
              : formatCompactDollars(live.indicativeVolumeNanos)
          }
        />
        <SubStat
          label={<Glossary term="Liquidity">liquidity</Glossary>}
          secondary="resting near the price"
          value={
            liquidityNanos > 0n ? formatCompactDollars(liquidityNanos) : "—"
          }
        />
      </div>

      {/* Last N batches mini-bars */}
      <div style={{ marginTop: 14 }}>
        <BatchHistoryBars
          marketId={outcome.marketId}
          blocks={recent.slice(0, HERO_BATCH_COUNT)}
        />
        <div
          style={{
            marginTop: 6,
            fontFamily: "var(--font-mono)",
            fontSize: 9,
            color: "var(--fg-4)",
            textTransform: "uppercase",
            letterSpacing: "0.04em",
          }}
        >
          last {HERO_BATCH_COUNT} batches · matched vol · price move
        </div>
      </div>
    </div>
  );
}

/** Most recent clearing YES price from a block where this market had matched
 * volume. Recent blocks are newest-first, so the first hit is the last trade. */
function lastTradedYesNanos(
  marketId: number,
  blocks: import("@/lib/api/schema").components["schemas"]["BlockResponse"][],
): bigint | null {
  const key = String(marketId);
  for (const block of blocks) {
    const volume = block.by_market?.[key]?.volume_nanos;
    if (volume == null || parseNanos(volume) <= 0n) continue;
    const price = block.clearing_prices_nanos?.[key]?.[0];
    if (price != null) return parseNanos(price);
  }
  return null;
}

export function deriveOpenBatchPrice(
  live: OpenBatchLive | null,
  readState: OpenBatchReadState,
  committedYesNanos: bigint | null,
  outcomeLabel: string,
): {
  kind: "indicative" | "last-clearing";
  valueNanos: bigint | null;
  secondary: string;
} {
  // Only a successful shadow-solve with positive crossing volume is an
  // indicative price. A one-sided/no-cross batch, initial load, or cold outage
  // uses the committed mark and labels it as such.
  if (
    live?.indicativeYesPriceNanos != null &&
    live.indicativeVolumeNanos > 0n
  ) {
    return {
      kind: "indicative",
      valueNanos: live.indicativeYesPriceNanos,
      secondary:
        readState === "stale"
          ? "saved open-batch response"
          : `for ${outcomeLabel}`,
    };
  }

  const secondary =
    readState === "loading"
      ? "open batch loading"
      : readState === "unavailable"
        ? "open batch unavailable"
        : readState === "stale"
          ? "no cross in saved response"
          : "no open-batch cross";
  return {
    kind: "last-clearing",
    valueNanos: committedYesNanos,
    secondary,
  };
}

export function OpenBatchReadNotice({
  state,
  retrying,
  onRetry,
}: {
  state: OpenBatchReadState;
  retrying: boolean;
  onRetry: () => void;
}) {
  if (state === "loading" || state === "ready") return null;
  const stale = state === "stale";
  return (
    <div
      role={stale ? "status" : "alert"}
      aria-live={stale ? "polite" : undefined}
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: "var(--space-2)",
        margin: "0 0 14px",
        padding: "var(--space-2) var(--space-3)",
        border:
          "1px solid color-mix(in srgb, var(--warn) 45%, var(--border-1))",
        borderRadius: "var(--radius-sm)",
        color: "var(--warn)",
        fontFamily: "var(--font-mono)",
        fontSize: "var(--fs-11)",
      }}
    >
      <span>
        {stale
          ? "open-batch refresh failed · showing saved response"
          : "open-batch data unavailable"}
      </span>
      <button
        type="button"
        disabled={retrying}
        onClick={onRetry}
        style={{
          minWidth: 44,
          minHeight: 44,
          padding: "0 var(--space-2)",
          border: "1px solid var(--border-2)",
          borderRadius: "var(--radius-sm)",
          background: "var(--surface-2)",
          color: "var(--fg-1)",
          font: "inherit",
          cursor: retrying ? "wait" : "pointer",
        }}
      >
        {retrying ? "retrying…" : "retry"}
      </button>
    </div>
  );
}

function CircularCountdown({
  progress01,
  countdown,
}: {
  progress01: number;
  countdown: string;
}) {
  // Large hero gauge — matches the handoff `BatchCountdown size="lg"`.
  const size = 176;
  const stroke = 8;
  const radius = (size - stroke) / 2;
  const circumference = 2 * Math.PI * radius;
  const dashOffset = circumference * (1 - progress01);

  return (
    <div
      style={{
        position: "relative",
        width: size,
        height: size,
        flexShrink: 0,
      }}
    >
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          stroke="color-mix(in srgb, var(--accent) 20%, transparent)"
          strokeWidth={stroke}
          fill="none"
        />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          stroke="var(--accent)"
          strokeWidth={stroke}
          fill="none"
          strokeLinecap="round"
          strokeDasharray={circumference}
          strokeDashoffset={dashOffset}
          transform={`rotate(-90 ${size / 2} ${size / 2})`}
          style={{ transition: "stroke-dashoffset 60ms linear" }}
        />
      </svg>
      <div
        style={{
          position: "absolute",
          inset: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          fontFamily: "var(--font-mono)",
          fontSize: 32,
          fontWeight: 600,
          color: "var(--fg-1)",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {countdown}
      </div>
    </div>
  );
}

function SubStat({
  label,
  secondary,
  value,
  valueColor,
}: {
  label: React.ReactNode;
  secondary?: string;
  value: React.ReactNode;
  valueColor?: string;
}) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "space-between",
        alignItems: "center",
        gap: 12,
      }}
    >
      <div style={{ display: "flex", flexDirection: "column", minWidth: 0 }}>
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            color: "var(--fg-3)",
            textTransform: "uppercase",
            letterSpacing: "0.04em",
          }}
        >
          {label}
        </span>
        {secondary && (
          <span
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 10,
              color: "var(--fg-4)",
              marginTop: 1,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {secondary}
          </span>
        )}
      </div>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 18,
          color: valueColor || "var(--fg-1)",
          fontVariantNumeric: "tabular-nums",
          flexShrink: 0,
        }}
      >
        {value}
      </span>
    </div>
  );
}

/** One bar's worth of derived, market-scoped batch stats. */
type BatchBar = {
  height: number;
  /** Wall-clock the batch settled (ms), for the "settled N ago" tooltip line. */
  timestampMs: number;
  /** This market's matched volume in the batch (nanos). */
  volNanos: bigint;
  /** This market's YES clearing price this batch, or null if it didn't clear. */
  yesNanos: bigint | null;
  /**
   * YES price change vs the previous *clearing* batch, in percentage points.
   * null when this batch didn't clear or there's no prior clear in the window.
   */
  ppChange: number | null;
};

/** ±N.N pp with an explicit sign (true minus glyph for negatives). */
function formatPp(pp: number): string {
  if (Math.abs(pp) < 0.05) return "0.0 pp";
  const sign = pp > 0 ? "+" : "−";
  return `${sign}${Math.abs(pp).toFixed(1)} pp`;
}

/** Green when YES rose this batch, red when it fell, neutral otherwise. */
function barColor(ppChange: number | null): string {
  if (ppChange == null || Math.abs(ppChange) < 0.05)
    return "color-mix(in srgb, var(--fg-4) 45%, transparent)";
  return ppChange > 0 ? "var(--yes)" : "var(--no)";
}

function BatchHistoryBars({
  marketId,
  blocks,
}: {
  marketId: number;
  blocks: import("@/lib/api/schema").components["schemas"]["PublicBlockResponse"][];
}) {
  const [hover, setHover] = useState<{ height: number; rect: DOMRect } | null>(
    null,
  );
  const key = String(marketId);

  // "Now" reference for the settled-ago line — the newest committed batch.
  // `blocks` arrives newest-first (we reverse it below for the bars).
  const nowMs = blocks[0]?.timestamp_ms ?? 0;

  // Sort oldest-first so the bars read left-to-right chronologically, then
  // derive this market's volume + price move per batch in one pass. Price
  // change compares against the previous batch that actually cleared this
  // market (batches may skip a market when it has no crossing orders).
  const bars: BatchBar[] = [];
  let prevYes: bigint | null = null;
  for (const b of [...blocks].reverse()) {
    const rawYes = b.clearing_prices_nanos?.[key]?.[0];
    const yesNanos = rawYes == null ? null : parseNanos(rawYes);
    const volNanos = parseNanos(b.by_market?.[key]?.volume_nanos ?? 0);
    let ppChange: number | null = null;
    if (yesNanos != null) {
      if (prevYes != null) ppChange = Number(yesNanos - prevYes) / 1e7;
      prevYes = yesNanos;
    }
    bars.push({
      height: b.height,
      timestampMs: b.timestamp_ms ?? 0,
      volNanos,
      yesNanos,
      ppChange,
    });
  }

  // Pad on the left with empty placeholders so the row width stays stable
  // while the ring buffer is still filling.
  const padCount = Math.max(0, HERO_BATCH_COUNT - bars.length);

  // Scale bar heights to this market's busiest batch in the window.
  let max = 0n;
  for (const bar of bars) if (bar.volNanos > max) max = bar.volNanos;

  return (
    <div
      style={{
        display: "flex",
        alignItems: "flex-end",
        gap: 2,
        height: 48,
        position: "relative",
      }}
    >
      {Array.from({ length: padCount }).map((_, i) => (
        <div
          key={`empty-${i}`}
          style={{
            flex: "1 1 0",
            alignSelf: "flex-end",
            height: 4,
            background: "color-mix(in srgb, var(--fg-4) 12%, transparent)",
            borderRadius: 1,
          }}
        />
      ))}
      {bars.map((bar) => {
        const ratio =
          max === 0n ? 0 : Number((bar.volNanos * 1000n) / max) / 1000;
        const h = Math.max(4, ratio * 44);
        const isHover = hover?.height === bar.height;
        return (
          <div
            key={bar.height}
            onMouseEnter={(e) =>
              setHover({
                height: bar.height,
                rect: e.currentTarget.getBoundingClientRect(),
              })
            }
            onMouseLeave={() =>
              setHover((cur) => (cur?.height === bar.height ? null : cur))
            }
            style={{
              flex: "1 1 0",
              height: "100%",
              display: "flex",
              alignItems: "flex-end",
              position: "relative",
              cursor: "default",
            }}
          >
            <div
              style={{
                width: "100%",
                height: h,
                background: barColor(bar.ppChange),
                opacity: isHover ? 1 : 0.8,
                borderRadius: "1px 1px 0 0",
                transition: "opacity 80ms linear",
              }}
            />
            {isHover && hover && (
              <BatchBarTooltip
                bar={bar}
                anchor={hover.rect}
                settledAgoMs={
                  nowMs > 0 ? Math.max(0, nowMs - bar.timestampMs) : null
                }
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

function BatchBarTooltip({
  bar,
  anchor,
  settledAgoMs,
}: {
  bar: BatchBar;
  anchor: DOMRect;
  /** ms since the batch settled, or null when no "now" reference is known. */
  settledAgoMs: number | null;
}) {
  const cleared = bar.yesNanos != null;
  const ppColor =
    bar.ppChange == null || Math.abs(bar.ppChange) < 0.05
      ? "var(--fg-3)"
      : bar.ppChange > 0
        ? "var(--yes)"
        : "var(--no)";
  const settledLabel =
    settledAgoMs == null
      ? "—"
      : settledAgoMs < 5000
        ? "just now"
        : `${formatAge(settledAgoMs)} ago`;
  return (
    <FloatingTooltip anchor={anchor} width={170} align="center" estHeight={92}>
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
        <span style={{ color: "var(--fg-3)" }}>
          batch #{formatInt(bar.height)}
        </span>
        <div
          style={{ display: "flex", justifyContent: "space-between", gap: 12 }}
        >
          <span style={{ color: "var(--fg-3)" }}>settled</span>
          <span style={{ color: "var(--fg-1)" }}>{settledLabel}</span>
        </div>
        <div
          style={{ display: "flex", justifyContent: "space-between", gap: 12 }}
        >
          <span style={{ color: "var(--fg-3)" }}>matched vol</span>
          <span style={{ color: "var(--fg-1)" }}>
            {formatCompactDollars(bar.volNanos)}
          </span>
        </div>
        <div
          style={{ display: "flex", justifyContent: "space-between", gap: 12 }}
        >
          <span style={{ color: "var(--fg-3)" }}>price move</span>
          <span style={{ color: ppColor }}>
            {!cleared
              ? "no clear"
              : bar.ppChange == null
                ? "—"
                : formatPp(bar.ppChange)}
          </span>
        </div>
      </div>
    </FloatingTooltip>
  );
}
