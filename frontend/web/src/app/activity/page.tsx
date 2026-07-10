"use client";

/**
 * Activity page — the live dashboard for everything happening on Sybil.
 * See `frontend/ACTIVITY_PLAN.md` for the architecture; mocked fields are
 * tracked in `frontend/OPEN_QUESTIONS.md` items #3-#6.
 */

import { useActivityOverview } from "@/lib/activity/use-activity-overview";
import { HeroAllTime } from "@/components/activity/hero-all-time";
import { PulseStrip } from "@/components/activity/pulse-strip";
import { BatchesTable } from "@/components/activity/batches-table";
import { BatchDetail } from "@/components/activity/batch-detail";
import { ActivityBatchChip } from "@/components/activity/batch-chip";
import { PageHeader } from "@/components/page-header";
import { BLOCK_INTERVAL_MS } from "@/lib/constants";
import { useArenaFeed } from "@/lib/arena/use-arena-feed";

export default function ActivityPage() {
  const overview = useActivityOverview();
  const bots = useArenaFeed({ limit: 1 });
  const botCount =
    bots.data?.db_available === true ? (bots.data.stats?.traders ?? null) : null;

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
        className="sybil-page-pad"
        style={{
          // +36px = markets ClearingTicker height, so the title aligns
          // with /'s "All markets" across pages
          paddingTop: "calc(var(--space-6) + 36px)",
        }}
      >
        <PageHeader
          title="Activity"
          meta={`everything happening on Sybil · uniform clearing every ${BLOCK_INTERVAL_MS / 1000}s`}
          action={<ActivityBatchChip />}
        />
      </div>

      <HeroAllTime allTime={overview.allTime} botCount={botCount} />
      <PulseStrip last24h={overview.last24h} />
      <BatchesTable renderDetail={(r) => <BatchDetail row={r} />} />
    </main>
  );
}
