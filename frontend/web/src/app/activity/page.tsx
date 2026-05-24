"use client";

/**
 * Activity page — the live dashboard for everything happening on Sybil.
 * See `frontend/ACTIVITY_PLAN.md` for the architecture; mocked fields are
 * tracked in `frontend/OPEN_QUESTIONS.md` items #3-#6.
 */

import { useActivityOverview } from "@/lib/activity/use-activity-overview";
import { useBatches } from "@/lib/activity/use-batches";
import { HeroAllTime } from "@/components/activity/hero-all-time";
import { PulseStrip } from "@/components/activity/pulse-strip";
import { BatchesTable } from "@/components/activity/batches-table";
import { BatchDetail } from "@/components/activity/batch-detail";
import { ActivityBatchChip } from "@/components/activity/batch-chip";

export default function ActivityPage() {
  const overview = useActivityOverview();
  const { rows, isBackfilling } = useBatches(60);

  return (
    <main
      style={{
        minHeight: "100vh",
        background: "var(--bg-1)",
        color: "var(--fg-1)",
        fontFamily: "var(--font-sans)",
        display: "flex",
        flexDirection: "column",
      }}
    >
      <div
        style={{
          // +36px = markets ClearingTicker height, so the title aligns
          // with /'s "All markets" across pages
          padding: "calc(var(--space-6) + 36px) var(--space-5) 0",
          display: "flex",
          alignItems: "baseline",
          gap: 14,
        }}
      >
        <h1
          style={{
            fontFamily: "var(--font-display)",
            fontWeight: 600,
            fontSize: "var(--fs-56)",
            lineHeight: "var(--lh-56)",
            letterSpacing: "var(--track-tight)",
            color: "var(--fg-1)",
            margin: 0,
          }}
        >
          Activity
        </h1>
        <span className="text-annotation" style={{ fontSize: 11 }}>
          everything happening on Sybil · uniform clearing every 10 s
        </span>
        <span style={{ marginLeft: "auto" }}>
          <ActivityBatchChip />
        </span>
      </div>

      <HeroAllTime allTime={overview.allTime} />
      <PulseStrip last24h={overview.last24h} />
      <BatchesTable
        rows={rows}
        isBackfilling={isBackfilling}
        renderDetail={(r) => <BatchDetail row={r} />}
      />
    </main>
  );
}
