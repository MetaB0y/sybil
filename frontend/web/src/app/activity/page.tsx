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
          padding: "20px 24px 0",
          display: "flex",
          alignItems: "baseline",
          gap: 14,
        }}
      >
        <h1
          style={{
            fontFamily: "var(--font-sans)",
            fontSize: 22,
            fontWeight: 600,
            letterSpacing: "-0.01em",
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
