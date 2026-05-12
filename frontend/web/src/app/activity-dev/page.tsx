"use client";

/**
 * Prototype route for the Activity page. Raw dumps from the activity hooks
 * with no styling — used to verify numbers against a hand calc on real
 * blocks before any visual components land. Deleted in the same commit that
 * lifts the styled page to /activity.
 */

import { useState } from "react";
import {
  formatCompactDollars,
  formatDollars,
  formatInt,
  formatPctDelta,
  formatProbability,
} from "@/lib/format/nanos";
import {
  selectConnection,
  selectLatestHeight,
  selectHydration,
  useStore,
} from "@/lib/store";
import { useActivityOverview } from "@/lib/activity/use-activity-overview";
import { useBatches } from "@/lib/activity/use-batches";
import { useBatchDetail } from "@/lib/activity/use-batch-detail";
import {
  pctDeltaBigint,
  pctDeltaNumber,
} from "@/lib/activity/derive-overview";

export default function ActivityDevPage() {
  const hydration = useStore(selectHydration);
  const connection = useStore(selectConnection);
  const latestHeight = useStore(selectLatestHeight);

  const overview = useActivityOverview();
  const { rows, isBackfilling } = useBatches(60);

  const [expandedHeight, setExpandedHeight] = useState<number | null>(null);
  const detail = useBatchDetail(expandedHeight);

  return (
    <main
      style={{
        minHeight: "100vh",
        background: "var(--bg-1)",
        color: "var(--fg-1)",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        padding: 24,
        display: "flex",
        flexDirection: "column",
        gap: 24,
      }}
    >
      <header>
        <div style={eyebrow}>{"// activity · prototype"}</div>
        <h1 style={{ fontSize: 18, margin: "4px 0", fontWeight: 600 }}>
          activity-dev
        </h1>
        <div style={{ color: "var(--fg-3)", fontSize: 11 }}>
          raw values from hooks · for backend sanity-check before visuals
        </div>
      </header>

      <Panel title="// connection">
        <KV k="hydration" v={hydration} />
        <KV k="ws.state" v={connection.state} />
        <KV
          k="latest.height"
          v={latestHeight != null ? formatInt(latestHeight) : "—"}
        />
      </Panel>

      <Panel title="// all-time (mostly mocked — see OPEN_QUESTIONS #3)">
        <KV
          k="matchedVolume"
          v={overview.allTime.matchedVolume}
          mocked={overview.allTime.mocked.matchedVolume}
        />
        <KV
          k="totalBatches"
          v={formatInt(overview.allTime.totalBatches)}
          mocked={false}
        />
        <KV
          k="liveMarkets"
          v={String(overview.allTime.liveMarkets)}
          mocked={false}
        />
        <KV
          k="traders"
          v={formatInt(overview.allTime.traders)}
          mocked={overview.allTime.mocked.traders}
        />
        <KV
          k="ordersPlaced"
          v={formatInt(overview.allTime.ordersPlaced)}
          mocked={overview.allTime.mocked.orders}
        />
        <KV
          k="ordersMatched"
          v={formatInt(overview.allTime.ordersMatched)}
          mocked={overview.allTime.mocked.orders}
        />
        <KV
          k="ordersUnmatched"
          v={formatInt(overview.allTime.ordersUnmatched)}
          mocked={overview.allTime.mocked.orders}
        />
        <KV
          k="uptime"
          v={overview.allTime.uptime}
          mocked={overview.allTime.mocked.uptime}
        />
      </Panel>

      <Panel
        title={`// recent activity · ${describeWindow(
          overview.last24h.firstTimestampMs,
          overview.last24h.lastTimestampMs,
          overview.last24h.blockCount
        )}`}
      >
        <KV
          k="window"
          v={`store buffer holds ${overview.last24h.blockCount} blocks; honest 24h needs backend rollup (OPEN_QUESTIONS #3)`}
        />
        <KV
          k="prior window"
          v={
            overview.prior24h.blockCount > 0
              ? `${overview.prior24h.blockCount} blocks`
              : "empty — buffer doesn't extend back that far"
          }
        />
        <KV
          k="matchedVolume"
          v={formatCompactDollars(overview.last24h.matchedVolumeNanos)}
        />
        <KV
          k="vs prior 24h"
          v={fmtPctOrDash(
            pctDeltaBigint(
              overview.last24h.matchedVolumeNanos,
              overview.prior24h.matchedVolumeNanos
            )
          )}
        />
        <KV k="traders" v={formatInt(overview.last24h.traders)} />
        <KV
          k="vs prior 24h"
          v={fmtPctOrDash(
            pctDeltaNumber(overview.last24h.traders, overview.prior24h.traders)
          )}
        />
        <KV
          k="ordersPlaced"
          v={formatInt(overview.last24h.ordersPlaced)}
        />
        <KV
          k="ordersMatched"
          v={formatInt(overview.last24h.ordersMatched)}
        />
        <KV
          k="ordersUnmatched"
          v={formatInt(overview.last24h.ordersUnmatched)}
        />
      </Panel>

      <Panel
        title={`// batches table · ${rows.length} rows${
          isBackfilling ? " · backfilling" : ""
        }`}
      >
        <div
          style={{
            display: "grid",
            gridTemplateColumns:
              "32px 80px 130px 60px 110px 110px 60px 1fr",
            gap: 12,
            fontSize: 10,
            color: "var(--fg-3)",
            borderBottom: "1px solid var(--border-1)",
            paddingBottom: 4,
            marginBottom: 4,
          }}
        >
          <div></div>
          <div>height</div>
          <div>cleared</div>
          <div>mkts</div>
          <div>vol</div>
          <div>welfare</div>
          <div>trdrs</div>
          <div>placed/matched/unmatched</div>
        </div>
        {rows.map((r) => (
          <div
            key={r.height}
            onClick={() =>
              setExpandedHeight(expandedHeight === r.height ? null : r.height)
            }
            style={{
              display: "grid",
              gridTemplateColumns:
                "32px 80px 130px 60px 110px 110px 60px 1fr",
              gap: 12,
              cursor: "pointer",
              padding: "3px 0",
              borderBottom: "1px solid var(--border-1)",
              background:
                expandedHeight === r.height ? "var(--surface-2)" : "transparent",
            }}
          >
            <div style={{ color: "var(--fg-4)" }}>
              {expandedHeight === r.height ? "▼" : "▸"}
            </div>
            <div>#{formatInt(r.height)}</div>
            <div style={{ color: "var(--fg-3)" }}>
              {new Date(r.timestampMs).toISOString().slice(11, 19)}
            </div>
            <div>{r.marketsTouched}</div>
            <div>{formatCompactDollars(r.matchedVolumeNanos)}</div>
            <div style={{ color: "var(--yes)" }}>
              {formatCompactDollars(r.welfareNanos)}
            </div>
            <div>{r.uniqueTraders}</div>
            <div style={{ color: "var(--fg-3)" }}>
              {r.ordersPlaced} / {r.ordersMatched} /{" "}
              <span style={{ color: "var(--no)" }}>{r.ordersUnmatched}</span>
            </div>
          </div>
        ))}
        {rows.length === 0 && (
          <div style={{ color: "var(--fg-3)", padding: 8 }}>
            no blocks in store yet — waiting for hydration
          </div>
        )}
      </Panel>

      {expandedHeight != null && (
        <Panel
          title={`// batch detail · #${formatInt(expandedHeight)} · ${
            detail.rows.length
          } market rows`}
        >
          {detail.isPending && <div>loading…</div>}
          {!detail.isPending && detail.block && (
            <>
              <KV
                k="ts"
                v={new Date(detail.block.timestamp_ms).toISOString()}
              />
              <KV
                k="parent_hash"
                v={detail.block.parent_hash.slice(0, 18) + "…"}
              />
              <KV
                k="state_root"
                v={detail.block.state_root.slice(0, 18) + "…"}
              />
              <KV
                k="total_volume"
                v={formatDollars(detail.block.total_volume_nanos)}
              />
              <KV
                k="total_welfare"
                v={formatDollars(detail.block.total_welfare_nanos)}
              />
              <KV
                k="prev?"
                v={detail.prev ? `#${formatInt(detail.prev.height)}` : "—"}
              />
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns:
                    "60px 1fr 80px 60px 80px 80px 80px 60px",
                  gap: 10,
                  fontSize: 10,
                  color: "var(--fg-3)",
                  borderTop: "1px solid var(--border-1)",
                  marginTop: 8,
                  paddingTop: 6,
                }}
              >
                <div>id</div>
                <div>title</div>
                <div>clear</div>
                <div>Δ</div>
                <div>vol [m]</div>
                <div>welf [m]</div>
                <div>p/m [m]</div>
                <div>imb [m]</div>
              </div>
              {detail.rows.map((r) => (
                <div
                  key={r.marketId}
                  style={{
                    display: "grid",
                    gridTemplateColumns:
                      "60px 1fr 80px 60px 80px 80px 80px 60px",
                    gap: 10,
                    padding: "2px 0",
                  }}
                >
                  <div style={{ color: "var(--fg-3)" }}>#{r.marketId}</div>
                  <div style={truncate}>{r.title}</div>
                  <div>{formatProbability(r.clearPriceNanos)}</div>
                  <div
                    style={{
                      color:
                        r.deltaNanos == null
                          ? "var(--fg-4)"
                          : r.deltaNanos > 0n
                          ? "var(--yes)"
                          : r.deltaNanos < 0n
                          ? "var(--no)"
                          : "var(--fg-3)",
                    }}
                  >
                    {r.deltaNanos == null
                      ? "—"
                      : formatPctDelta(Number(r.deltaNanos) / 1e7)}
                  </div>
                  <div>{formatCompactDollars(r.matchedVolumeNanos)}</div>
                  <div>{formatCompactDollars(r.welfareNanos)}</div>
                  <div style={{ color: "var(--fg-3)" }}>
                    {r.ordersPlaced}/{r.ordersMatched}
                  </div>
                  <div>{(r.imbalanceBps / 100).toFixed(1)}%</div>
                </div>
              ))}
            </>
          )}
        </Panel>
      )}

      <footer style={{ color: "var(--fg-4)", fontSize: 10 }}>
        verify against: curl{" "}
        {process.env.NEXT_PUBLIC_API_BASE}/v1/blocks/{"{height}"}
      </footer>
    </main>
  );
}

function Panel({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section
      style={{
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: 4,
        padding: 12,
      }}
    >
      <div style={eyebrow}>{title}</div>
      <div style={{ marginTop: 6, display: "flex", flexDirection: "column", gap: 2 }}>
        {children}
      </div>
    </section>
  );
}

function KV({
  k,
  v,
  mocked,
}: {
  k: string;
  v: string;
  mocked?: boolean;
}) {
  return (
    <div style={{ display: "flex", gap: 10 }}>
      <span style={{ color: "var(--fg-3)", width: 160, flexShrink: 0 }}>
        {k}
      </span>
      <span
        style={{
          color: "var(--fg-1)",
          borderBottom: mocked
            ? "1px dotted color-mix(in srgb, var(--warn) 60%, transparent)"
            : "none",
        }}
        title={mocked ? "mocked — backend field pending" : undefined}
      >
        {v}
      </span>
    </div>
  );
}

function fmtPctOrDash(pct: number | null): string {
  if (pct == null) return "—";
  return formatPctDelta(pct);
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

const eyebrow: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 10,
  color: "var(--fg-3)",
  textTransform: "uppercase",
  letterSpacing: "0.04em",
};

const truncate: React.CSSProperties = {
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};
