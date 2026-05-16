"use client";

/**
 * Pro-mode hero card — the showpiece of the rail. Big circular countdown
 * gauge + batch # + "N traders joined" + indicative trio + last-24-batches
 * mini bars. Matches the inline hero block in `V2BatchTheater` ProRail
 * (`fed-variations.jsx:128`).
 *
 * All values are real:
 *  - circular countdown progress + batch number
 *  - traders in this batch — polled open-batch unique placers
 *  - indicative price / volume — polled open-batch shadow-solve (C2)
 *  - past-batch bar heights (matched volume)
 */

import { Glossary } from "@/components/glossary";
import {
  formatBatchSeconds,
  formatCents,
  formatCompactDollars,
  parseNanos,
} from "@/lib/format/nanos";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";
import { useOpenBatchLive } from "@/lib/market-detail/use-open-batch-live";
import { selectRecentBlocks, useStore } from "@/lib/store";
import { useBatchCountdown } from "./use-batch-countdown";

const HERO_BATCH_COUNT = 24;

export function BatchHero({ outcome }: { outcome: EventOutcome }) {
  const { progress01, secondsLeftPrecise, latestHeight } = useBatchCountdown();
  const live = useOpenBatchLive(outcome.marketId);
  const recent = useStore(selectRecentBlocks);

  const batchNumber = latestHeight == null ? null : latestHeight + 1;
  const placers = live?.uniquePlacers ?? null;
  // Indicative YES price falls back to the last clearing price when the
  // shadow-solve has no resting orders for this market (null).
  const indicativeYesNanos = live?.indicativeYesPriceNanos ?? outcome.yesPriceNanos;

  return (
    <div
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
            <span
              style={{ color: "var(--fg-1)", fontWeight: 600 }}
              title="Distinct traders with a resting order in the open batch — updates ~1s"
            >
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

      {/* Indicative trio */}
      <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        <SubStat
          label={
            <Glossary term="Indicative price">indicative price</Glossary>
          }
          secondary={`for ${outcome.shortLabel}`}
          value={
            indicativeYesNanos == null ? "—" : formatCents(indicativeYesNanos)
          }
          valueColor="var(--yes)"
        />
        <SubStat
          label={<Glossary term="IEV">indicative volume</Glossary>}
          secondary="would clear at indicative"
          value={
            live == null ? "—" : formatCompactDollars(live.indicativeVolumeNanos)
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
          last {HERO_BATCH_COUNT} batches · matched volume
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
        return (
          <div
            key={b.height}
            title={`batch #${b.height} · matched vol ${vol}n`}
            style={{
              flex: "1 1 0",
              height: h,
              background: "var(--accent)",
              opacity: 0.75,
              borderRadius: "1px 1px 0 0",
            }}
          />
        );
      })}
    </div>
  );
}
