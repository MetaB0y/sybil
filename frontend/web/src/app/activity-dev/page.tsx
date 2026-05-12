"use client";

/**
 * Prototype route for the Activity page. Renders the styled visual
 * components (HeroAllTime, PulseStrip, BatchesTable) on top, with a debug
 * dashboard underneath showing raw hook outputs + connection state for
 * sanity-checking. The whole file is deleted in the commit that lifts the
 * styled page to /activity.
 */

import { formatCompactDollars, formatInt } from "@/lib/format/nanos";
import {
  selectConnection,
  selectLatestHeight,
  selectHydration,
  useStore,
} from "@/lib/store";
import { useActivityOverview } from "@/lib/activity/use-activity-overview";
import { useBatches } from "@/lib/activity/use-batches";
import { HeroAllTime } from "@/components/activity/hero-all-time";
import { PulseStrip } from "@/components/activity/pulse-strip";
import { BatchesTable } from "@/components/activity/batches-table";
import { BatchDetail } from "@/components/activity/batch-detail";

export default function ActivityDevPage() {
  const hydration = useStore(selectHydration);
  const connection = useStore(selectConnection);
  const latestHeight = useStore(selectLatestHeight);

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
      {/* Page header (matches the handoff zone above the hero) */}
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
          everything happening on Sybil · uniform clearing every 2 s
        </span>
        <span
          style={{
            marginLeft: "auto",
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            color: "var(--warn)",
            background: "var(--warn-soft)",
            padding: "2px 7px",
            borderRadius: 9999,
            letterSpacing: "0.04em",
            textTransform: "uppercase",
          }}
        >
          dev preview
        </span>
      </div>

      <HeroAllTime allTime={overview.allTime} />
      <PulseStrip />
      <BatchesTable
        rows={rows}
        isBackfilling={isBackfilling}
        renderDetail={(r) => <BatchDetail row={r} />}
      />

      {/* Debug dashboard — kept on the dev route as a sanity check; not shipped to /activity. */}
      <section
        style={{
          margin: "24px 24px 40px",
          padding: 16,
          background: "var(--surface-1)",
          border: "1px dashed var(--border-2)",
          borderRadius: 6,
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          color: "var(--fg-2)",
        }}
      >
        <div
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            color: "var(--fg-3)",
            textTransform: "uppercase",
            letterSpacing: "0.04em",
            marginBottom: 10,
          }}
        >
          {"// debug · removed when lifted to /activity"}
        </div>
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 24 }}>
          <DebugBlock title="connection">
            <Kv k="hydration" v={hydration} />
            <Kv k="ws.state" v={connection.state} />
            <Kv
              k="latest.height"
              v={latestHeight != null ? formatInt(latestHeight) : "—"}
            />
            <Kv k="recent buffer" v={`${rows.length} rows`} />
          </DebugBlock>
          <DebugBlock
            title={`recent window · ${describeWindow(
              overview.last24h.firstTimestampMs,
              overview.last24h.lastTimestampMs,
              overview.last24h.blockCount
            )}`}
          >
            <Kv
              k="matchedVolume (real)"
              v={formatCompactDollars(overview.last24h.matchedVolumeNanos)}
            />
            <Kv k="traders (real)" v={formatInt(overview.last24h.traders)} />
            <Kv
              k="placed/matched/unmatched (real)"
              v={`${overview.last24h.ordersPlaced} / ${overview.last24h.ordersMatched} / ${overview.last24h.ordersUnmatched}`}
            />
            <Kv
              k="prior window"
              v={
                overview.prior24h.blockCount > 0
                  ? `${overview.prior24h.blockCount} blocks`
                  : "empty (buffer doesn't extend that far)"
              }
            />
          </DebugBlock>
        </div>
      </section>
    </main>
  );
}

function DebugBlock({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <div
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 10,
          color: "var(--fg-3)",
          textTransform: "uppercase",
          letterSpacing: "0.04em",
          marginBottom: 6,
        }}
      >
        {title}
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        {children}
      </div>
    </div>
  );
}

function Kv({ k, v }: { k: string; v: string }) {
  return (
    <div style={{ display: "flex", gap: 10 }}>
      <span style={{ color: "var(--fg-3)", width: 220, flexShrink: 0 }}>
        {k}
      </span>
      <span style={{ color: "var(--fg-1)" }}>{v}</span>
    </div>
  );
}

function describeWindow(
  firstMs: number | null,
  lastMs: number | null,
  count: number
): string {
  if (count === 0 || firstMs == null || lastMs == null) {
    return "empty";
  }
  const spanS = Math.max(0, Math.round((lastMs - firstMs) / 1000));
  let span: string;
  if (spanS < 60) span = `${spanS}s`;
  else if (spanS < 3600) {
    const m = Math.floor(spanS / 60);
    const s = spanS % 60;
    span = s > 0 ? `${m}m ${s}s` : `${m}m`;
  } else {
    const h = Math.floor(spanS / 3600);
    const m = Math.floor((spanS % 3600) / 60);
    span = m > 0 ? `${h}h ${m}m` : `${h}h`;
  }
  return `last ${span} · ${count} blocks`;
}
