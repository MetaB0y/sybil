"use client";

/**
 * Live "batch #N closing in T" chip for the Activity page header.
 *
 * Distinct from the global <BatchPill> in the nav: this one names the specific
 * batch # currently clearing. Lifted from the handoff's right-aligned
 * indicator.
 *
 * The clock comes from the shared `useBatchCountdown` — same source as the Pro
 * hero gauge and the "queued for batch" timer — so it opens at 9.9 rather than
 * resting on a static "10.0", and it stays glued to the store's block anchor
 * instead of restarting on remount.
 */

import { selectConnection, useStore } from "@/lib/store";
import { useBatchCountdown } from "@/components/market-rail/use-batch-countdown";
import { formatBatchSeconds, formatInt } from "@/lib/format/nanos";

export function ActivityBatchChip() {
  const connection = useStore(selectConnection);
  const { secondsLeftPrecise, latestHeight } = useBatchCountdown();

  if (latestHeight == null) return null;

  const isLive = connection.state === "live";
  // The current batch is the *next* one to clear, i.e. latestHeight + 1.
  const currentBatchNum = latestHeight + 1;

  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        color: "var(--fg-3)",
      }}
    >
      <span
        aria-hidden
        style={{
          width: 6,
          height: 6,
          borderRadius: "50%",
          background: isLive ? "var(--accent)" : "var(--warn)",
        }}
      />
      batch{" "}
      <span style={{ color: "var(--fg-1)" }}>
        #{formatInt(currentBatchNum)}
      </span>{" "}
      closing in{" "}
      <span
        style={{
          color: "var(--accent)",
          fontVariantNumeric: "tabular-nums",
          minWidth: 24,
          display: "inline-block",
        }}
      >
        {formatBatchSeconds(secondsLeftPrecise)}s
      </span>
    </span>
  );
}
