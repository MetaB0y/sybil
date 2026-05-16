"use client";

/**
 * Pro-mode hero card — the showpiece of the rail. Big circular countdown
 * gauge + batch # + "N traders joined" + indicative trio + last-24-batches
 * mini bars. Matches the inline hero block in `V2BatchTheater` ProRail
 * (`fed-variations.jsx:128`).
 *
 * Mocked values shown here (all wrapped in <MockValue>):
 *  - indicative price (#7)
 *  - indicative volume (#7)
 *  - imbalance label (#6)
 *  - side coloring of past-batch bars (#6) — bar HEIGHTS are real volume
 *
 * Real values:
 *  - circular countdown progress + batch number
 *  - traders in this batch — polled open-batch unique placers
 *  - past-batch bar heights (matched volume)
 */

import { MockValue } from "@/components/mock-value";
import {
  formatBatchSeconds,
  formatCompactDollars,
  formatProbability,
  parseNanos,
} from "@/lib/format/nanos";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";
import { useOpenBatch } from "@/lib/market-detail/use-open-batch";
import { useOpenBatchPlacers } from "@/lib/market-detail/use-open-batch-placers";
import { selectRecentBlocks, useStore } from "@/lib/store";
import { useBatchCountdown } from "./use-batch-countdown";

const HERO_BATCH_COUNT = 24;

export function BatchHero({ outcome }: { outcome: EventOutcome }) {
  const { progress01, secondsLeftPrecise, latestHeight } = useBatchCountdown();
  const snap = useOpenBatch(outcome.marketId);
  const placers = useOpenBatchPlacers(outcome.marketId);
  const recent = useStore(selectRecentBlocks);

  const batchNumber = latestHeight == null ? null : latestHeight + 1;

  return (
    <div
      style={{
        background:
          "linear-gradient(180deg, color-mix(in srgb, var(--accent) 10%, transparent), color-mix(in srgb, var(--accent) 2%, transparent))",
        border: "1px solid color-mix(in srgb, var(--accent) 30%, transparent)",
        borderRadius: 8,
        padding: "20px 22px",
        position: "relative",
        overflow: "hidden",
      }}
    >
      <div
        style={{
          position: "absolute",
          top: 14,
          right: 16,
          fontFamily: "var(--font-mono)",
          fontSize: 9.5,
          color: "var(--accent)",
          textTransform: "uppercase",
          letterSpacing: "0.06em",
        }}
      >
        ● live batch
      </div>

      {/* Hero clock: large circular gauge + label block */}
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        <CircularCountdown
          progress01={progress01}
          countdown={formatBatchSeconds(secondsLeftPrecise)}
        />
        <div style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 0 }}>
          <div
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 10,
              color: "var(--fg-3)",
              textTransform: "uppercase",
              letterSpacing: "0.06em",
            }}
          >
            next batch clears in
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
            <span
              style={{ color: "var(--fg-1)", fontWeight: 600 }}
              title="Distinct traders with a resting order in the open batch — updates ~1s"
            >
              {placers ?? "—"}
            </span>
            <span>traders in this batch</span>
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

      {/* Indicative trio */}
      <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        <SubStat
          label="indicative price"
          secondary={`for ${truncate(outcome.label, 32)}`}
          value={
            <MockValue hint="mid-batch clearing price not exposed (OPEN_QUESTIONS #7)">
              {formatProbability(snap.indicativeYesPriceNanos)}
            </MockValue>
          }
          valueColor="var(--yes)"
        />
        <SubStat
          label="indicative volume"
          secondary="would clear at indicative"
          value={
            <MockValue hint="mid-batch volume not exposed (OPEN_QUESTIONS #7)">
              {formatCompactDollars(snap.indicativeVolumeNanos)}
            </MockValue>
          }
        />
        <SubStat
          label="imbalance"
          secondary="net unmatched orders"
          value={
            <MockValue hint="NOT NOW — FillResponse has no side; per-batch imbalance is mocked (OPEN_QUESTIONS #6)">
              <span
                style={{
                  color: snap.imbalanceBps >= 0 ? "var(--yes)" : "var(--no)",
                }}
              >
                {snap.imbalanceBps >= 0 ? "↑ buy-side" : "↓ sell-side"}
              </span>
            </MockValue>
          }
        />
      </div>

      {/* Last N batches mini-bars */}
      <div style={{ marginTop: 14 }}>
        <BatchHistoryBars blocks={recent.slice(0, HERO_BATCH_COUNT)} />
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
          last {HERO_BATCH_COUNT} batches · matched volume + side
          <MockValue hint="NOT NOW — bar HEIGHTS = real matched volume per block. Bar COLORS = mocked side because FillResponse has no side (OPEN_QUESTIONS #6).">
            <span style={{ marginLeft: 4 }}>·</span>
          </MockValue>
        </div>
      </div>
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
  const size = 88;
  const stroke = 4;
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
          fontSize: 22,
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
  label: string;
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

function BatchHistoryBars({
  blocks,
}: {
  blocks: import("@/lib/api/schema").components["schemas"]["BlockResponse"][];
}) {
  // Sort oldest-first so the bars read left-to-right chronologically.
  const ordered = [...blocks].reverse();
  // Pad up to HERO_BATCH_COUNT with empty placeholders so the row width stays
  // stable while the ring buffer is still filling.
  const padded = [
    ...Array(Math.max(0, HERO_BATCH_COUNT - ordered.length)).fill(null),
    ...ordered,
  ] as (typeof ordered[number] | null)[];

  // Compute max volume across the window for bar scaling.
  let max = 0n;
  for (const b of ordered) {
    if (!b) continue;
    const v = parseNanos(b.total_volume_nanos);
    if (v > max) max = v;
  }

  return (
    <div
      style={{
        display: "flex",
        alignItems: "flex-end",
        gap: 2,
        height: 48,
      }}
    >
      {padded.map((b, i) => {
        if (!b) {
          return (
            <div
              key={`empty-${i}`}
              style={{
                flex: "1 1 0",
                height: 4,
                background: "color-mix(in srgb, var(--fg-4) 12%, transparent)",
                borderRadius: 1,
              }}
            />
          );
        }
        const vol = parseNanos(b.total_volume_nanos);
        const ratio = max === 0n ? 0 : Number((vol * 1000n) / max) / 1000;
        const h = Math.max(4, ratio * 44);
        // Mocked side coloring — derived deterministically from height.
        const sideMockYes = (b.height % 7) % 2 === 0;
        const color = sideMockYes ? "var(--yes)" : "var(--no)";
        return (
          <div
            key={b.height}
            title={`batch #${b.height} · matched vol ${vol}n`}
            style={{
              flex: "1 1 0",
              height: h,
              background: color,
              opacity: 0.75,
              borderRadius: "1px 1px 0 0",
            }}
          />
        );
      })}
    </div>
  );
}

function truncate(s: string, max: number): string {
  if (s.length <= max) return s;
  return s.slice(0, max - 1).trimEnd() + "…";
}
