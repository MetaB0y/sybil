"use client";

/**
 * The "next trade in" pill at the top of the Degen rail. Matches
 * `NextBatchBanner` in `fed-right-rail-modes.jsx:42`.
 *
 * Live data:
 *  - countdown / progress: REAL (driven by the 2s block cadence)
 *  - batch number: REAL (latest committed height + 1)
 *  - "N traders joined" counter: MOCK — sourced from useOpenBatch's
 *    mockTradersInOpenBatch (OPEN_QUESTIONS #7)
 */

import { MockValue } from "@/components/mock-value";
import { useOpenBatch } from "@/lib/market-detail/use-open-batch";
import { useBatchCountdown } from "./use-batch-countdown";

export function NextBatchBanner({ marketId }: { marketId: number }) {
  const { progress01, secondsLeft, latestHeight } = useBatchCountdown();
  const snap = useOpenBatch(marketId);
  const batchNumber = latestHeight == null ? null : latestHeight + 1;

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
        0:{secondsLeft.toString().padStart(2, "0")}
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
          {batchNumber != null && (
            <span
              style={{
                marginLeft: 8,
                color: "var(--fg-3)",
                fontVariantNumeric: "tabular-nums",
              }}
            >
              batch #{batchNumber.toLocaleString()}
            </span>
          )}
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
          {secondsLeft} {secondsLeft === 1 ? "second" : "seconds"}
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
          <MockValue hint="traders joined this batch — no prod-safe pending-orders endpoint (OPEN_QUESTIONS #7)">
            <span style={{ color: "var(--fg-1)", fontWeight: 600 }}>
              {snap.tradersInBatch}
            </span>
          </MockValue>
          <span>traders joined</span>
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
