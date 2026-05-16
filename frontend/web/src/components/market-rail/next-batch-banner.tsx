"use client";

/**
 * The "next trade in" pill at the top of the Degen rail. Matches
 * `NextBatchBanner` in `fed-right-rail-modes.jsx:42`.
 *
 * Live data:
 *  - countdown / progress: REAL (driven by the 2s block cadence)
 *  - "N traders in this batch": REAL — polled open-batch unique placers
 *    (see use-open-batch-live.ts)
 */

import { formatBatchSeconds } from "@/lib/format/nanos";
import { useOpenBatchLive } from "@/lib/market-detail/use-open-batch-live";
import { useBatchCountdown } from "./use-batch-countdown";

export function NextBatchBanner({ marketId }: { marketId: number }) {
  const { progress01, secondsLeftPrecise } = useBatchCountdown();
  const placers = useOpenBatchLive(marketId)?.uniquePlacers ?? null;
  const countdown = formatBatchSeconds(secondsLeftPrecise);

  return (
    <div
      style={{
        position: "relative",
        overflow: "hidden",
        background:
          "linear-gradient(135deg, color-mix(in srgb, var(--accent) 12%, transparent), color-mix(in srgb, var(--accent) 2%, transparent))",
        border: "1px solid color-mix(in srgb, var(--accent) 35%, transparent)",
        borderRadius: 8,
        padding: "14px 16px",
        display: "flex",
        alignItems: "center",
        gap: 14,
      }}
    >
      <div
        style={{
          width: 48,
          height: 48,
          borderRadius: "50%",
          background: "color-mix(in srgb, var(--accent) 10%, transparent)",
          border: "1.5px solid var(--accent)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          fontFamily: "var(--font-mono)",
          fontSize: 14,
          fontWeight: 600,
          color: "var(--accent)",
          fontVariantNumeric: "tabular-nums",
          flexShrink: 0,
        }}
      >
        {countdown}
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 9.5,
            color: "var(--accent)",
            textTransform: "uppercase",
            letterSpacing: "0.06em",
            marginBottom: 3,
          }}
        >
          ● next trade in
        </div>
        <div
          style={{
            fontFamily: "var(--font-sans)",
            fontSize: 17,
            fontWeight: 600,
            color: "var(--fg-1)",
            letterSpacing: "-0.01em",
            lineHeight: 1.2,
            fontVariantNumeric: "tabular-nums",
          }}
        >
          {countdown}s
        </div>
        <div
          style={{
            marginTop: 4,
            display: "flex",
            alignItems: "center",
            gap: 5,
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
      <div
        style={{
          position: "absolute",
          left: 0,
          right: 0,
          bottom: 0,
          height: 2,
          background: "color-mix(in srgb, var(--accent) 16%, transparent)",
        }}
      >
        <div
          style={{
            width: `${progress01 * 100}%`,
            height: "100%",
            background: "var(--accent)",
          }}
        />
      </div>
    </div>
  );
}
