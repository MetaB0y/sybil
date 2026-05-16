"use client";

/**
 * Prototype dev route for the specific-market page's three new data panels:
 *   1. Market stats (lifetime)
 *   2. Open-batch snapshot (in-flight)
 *   3. Recent batches window (1 / 5 / 10 / 100 selectable)
 *
 * Raw hook output, no styling — same pattern as /activity-dev. Folded into
 * /m/[id] with handoff styling once the data layer is settled.
 */

import { useState, use } from "react";
import Link from "next/link";
import { notFound } from "next/navigation";
import { MockValue } from "@/components/mock-value";
import {
  formatCompactDollars,
  formatDollars,
  formatInt,
  formatProbability,
} from "@/lib/format/nanos";
import {
  selectConnection,
  selectHydration,
  selectLatestBlock,
  useStore,
} from "@/lib/store";
import { useMarketStats } from "@/lib/market-detail/use-market-stats";
import { useOpenBatch } from "@/lib/market-detail/use-open-batch";
import {
  WINDOW_SIZES,
  useBatchWindowStats,
} from "@/lib/market-detail/use-batch-windows";
import type { WindowSize } from "@/lib/market-detail/types";

type RouteParams = { id: string };

export default function MarketDevPage({
  params,
}: {
  params: Promise<RouteParams>;
}) {
  const { id } = use(params);
  const marketId = Number(id);
  if (!Number.isFinite(marketId) || marketId < 0) {
    notFound();
  }

  return (
    <main
      style={{
        minHeight: "100vh",
        background: "var(--bg-1)",
        color: "var(--fg-1)",
        padding: "20px 24px 40px",
        display: "flex",
        flexDirection: "column",
        gap: 20,
      }}
    >
      <PageHeader marketId={marketId} />
      <MarketStatsPanel marketId={marketId} />
      <OpenBatchPanel marketId={marketId} />
      <RecentBatchesPanel marketId={marketId} />
      <DebugPanel />
    </main>
  );
}

// ── Header ───────────────────────────────────────────────────────────────

function PageHeader({ marketId }: { marketId: number }) {
  return (
    <div
      style={{
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
          margin: 0,
        }}
      >
        Market #{marketId}
      </h1>
      <span className="text-annotation" style={{ fontSize: 11, color: "var(--fg-3)" }}>
        data audit · raw hook output
      </span>
      <span style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 10 }}>
        <Link
          href={`/m/${marketId}`}
          style={{ fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--accent)" }}
        >
          → styled /m/{marketId}
        </Link>
        <span
          style={{
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
      </span>
    </div>
  );
}

// ── Panel 1: lifetime stats ──────────────────────────────────────────────

function MarketStatsPanel({ marketId }: { marketId: number }) {
  const { stats, isPending, isError } = useMarketStats(marketId);

  return (
    <Panel title="market stats (lifetime)">
      {isPending && <Loading>loading market metadata…</Loading>}
      {isError && <Loading error>error loading market</Loading>}
      {stats && (
        <Grid>
          <Stat label="total volume" value={formatCompactDollars(stats.totalVolumeNanos)} />
          <Stat
            label="24h volume"
            value={
              <MockValue hint="24h volume — no backend rollup (OPEN_QUESTIONS #3)">
                {formatCompactDollars(stats.volume24hNanos)}
              </MockValue>
            }
          />
          <Stat
            label="traders (lifetime)"
            value={
              <MockValue hint="trader count — MarketResponse lacks trader_count (OPEN_QUESTIONS #2)">
                {formatInt(stats.traders)}
              </MockValue>
            }
          />
          <Stat
            label="liquidity"
            value={
              <MockValue hint="liquidity — no resting book snapshot (OPEN_QUESTIONS #1)">
                {formatCompactDollars(stats.liquidityNanos)}
              </MockValue>
            }
          />
          <Stat
            label="batches existed for"
            value={
              stats.batchesExistedFor == null ? (
                "—"
              ) : (
                <MockValue hint="NOT NOW — approximate at 2s FBA cadence; backend lacks created_at_height (OPEN_QUESTIONS #9)">
                  ~{formatInt(stats.batchesExistedFor)}
                </MockValue>
              )
            }
          />
        </Grid>
      )}
    </Panel>
  );
}

// ── Panel 2: open-batch snapshot ─────────────────────────────────────────

function OpenBatchPanel({ marketId }: { marketId: number }) {
  const snap = useOpenBatch(marketId);

  return (
    <Panel
      title={`open batch (in-flight · height ${
        snap.latestHeight == null ? "?" : snap.latestHeight + 1
      })`}
      subtitle="every field is mocked — see OPEN_QUESTIONS #6 (imbalance) and #7 (everything else)"
    >
      <Grid>
        <Stat
          label="traders in batch"
          value={
            <MockValue hint="no prod-safe pending-orders endpoint (OPEN_QUESTIONS #7)">
              {formatInt(snap.tradersInBatch)}
            </MockValue>
          }
        />
        <Stat
          label="indicative YES"
          value={
            <MockValue hint="mid-batch clearing price not exposed (OPEN_QUESTIONS #7)">
              {formatProbability(snap.indicativeYesPriceNanos)}
            </MockValue>
          }
        />
        <Stat
          label="indicative volume"
          value={
            <MockValue hint="mid-batch volume not exposed (OPEN_QUESTIONS #7)">
              {formatCompactDollars(snap.indicativeVolumeNanos)}
            </MockValue>
          }
        />
        <Stat
          label="imbalance"
          value={
            <MockValue hint="NOT NOW — FillResponse has no side (OPEN_QUESTIONS #6)">
              {formatImbalance(snap.imbalanceBps)}
            </MockValue>
          }
        />
      </Grid>
    </Panel>
  );
}

function formatImbalance(bps: number): string {
  const pct = bps / 100;
  const sign = pct > 0 ? "+" : pct < 0 ? "−" : "";
  return `${sign}${Math.abs(pct).toFixed(2)}%`;
}

// ── Panel 3: recent N batches ────────────────────────────────────────────

function RecentBatchesPanel({ marketId }: { marketId: number }) {
  const [windowSize, setWindowSize] = useState<WindowSize>(10);
  const stats = useBatchWindowStats(marketId, windowSize);
  const partial = stats.actualBlockCount < windowSize;

  return (
    <Panel
      title="recent batches"
      subtitle={
        partial
          ? `requested ${windowSize}, store buffer holds ${stats.actualBlockCount} (${
              stats.firstHeight ?? "?"
            } … ${stats.lastHeight ?? "?"}) — be honest about the buffer window`
          : `last ${windowSize} batches (${stats.firstHeight ?? "?"} … ${stats.lastHeight ?? "?"})`
      }
      header={
        <WindowSelector value={windowSize} onChange={setWindowSize} />
      }
    >
      <Grid>
        <Stat
          label="unique traders placed"
          value={
            <MockValue hint="placed-trader counts not on the wire (OPEN_QUESTIONS #8)">
              {formatInt(stats.uniqueTradersPlaced)}
            </MockValue>
          }
          sub={
            <span style={{ color: "var(--fg-3)" }}>
              per-market split of mock placers
            </span>
          }
        />
        <Stat
          label="unique traders matched"
          value={
            <MockValue hint="per-market scoping is mocked (OPEN_QUESTIONS #5)">
              {formatInt(stats.uniqueTradersMatched)}
            </MockValue>
          }
          sub={
            <span style={{ color: "var(--fg-3)" }}>
              chain-wide real: {formatInt(stats.uniqueTradersMatchedChainWide)}
            </span>
          }
        />
        <Stat
          label="volume placed"
          value={
            <MockValue hint="no placed-volume notional on the wire (OPEN_QUESTIONS #8)">
              {formatCompactDollars(stats.volumePlacedNanos)}
            </MockValue>
          }
          sub={
            <span style={{ color: "var(--fg-3)" }}>
              ≈ matched × 1.3-1.7
            </span>
          }
        />
        <Stat
          label="volume matched"
          value={
            <MockValue hint="per-market scoping is mocked (OPEN_QUESTIONS #5)">
              {formatCompactDollars(stats.volumeMatchedNanos)}
            </MockValue>
          }
          sub={
            <span style={{ color: "var(--fg-3)" }}>
              chain-wide real: {formatDollars(stats.volumeMatchedChainWideNanos, { decimals: 0 })}
            </span>
          }
        />
      </Grid>
    </Panel>
  );
}

function WindowSelector({
  value,
  onChange,
}: {
  value: WindowSize;
  onChange: (n: WindowSize) => void;
}) {
  return (
    <div style={{ display: "flex", gap: 4, fontFamily: "var(--font-mono)", fontSize: 11 }}>
      {WINDOW_SIZES.map((n) => {
        const active = n === value;
        return (
          <button
            type="button"
            key={n}
            onClick={() => onChange(n)}
            style={{
              padding: "3px 10px",
              borderRadius: 4,
              border: `1px solid ${active ? "var(--accent)" : "var(--border-1)"}`,
              background: active ? "var(--accent-soft, transparent)" : "transparent",
              color: active ? "var(--accent)" : "var(--fg-2)",
              cursor: "pointer",
              fontFamily: "inherit",
              fontSize: "inherit",
            }}
          >
            {n}
          </button>
        );
      })}
    </div>
  );
}

// ── Debug strip ─────────────────────────────────────────────────────────

function DebugPanel() {
  const hydration = useStore(selectHydration);
  const connection = useStore(selectConnection);
  const latestBlock = useStore(selectLatestBlock);

  return (
    <section
      style={{
        marginTop: 16,
        padding: 14,
        background: "var(--surface-1)",
        border: "1px dashed var(--border-2)",
        borderRadius: 6,
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        color: "var(--fg-2)",
        display: "grid",
        gridTemplateColumns: "repeat(4, minmax(0, 1fr))",
        gap: 12,
      }}
    >
      <DebugItem label="hydration" value={hydration} />
      <DebugItem label="ws" value={connection.state} />
      <DebugItem
        label="latest height"
        value={latestBlock?.height ?? "—"}
      />
      <DebugItem
        label="last block ts"
        value={
          latestBlock?.timestamp_ms
            ? new Date(latestBlock.timestamp_ms).toISOString().slice(11, 19) + "Z"
            : "—"
        }
      />
    </section>
  );
}

function DebugItem({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
      <span style={{ color: "var(--fg-3)", fontSize: 10, textTransform: "uppercase", letterSpacing: "0.04em" }}>
        {label}
      </span>
      <span>{value}</span>
    </div>
  );
}

// ── Layout primitives ───────────────────────────────────────────────────

function Panel({
  title,
  subtitle,
  header,
  children,
}: {
  title: string;
  subtitle?: string;
  header?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section
      style={{
        padding: "var(--space-4) var(--space-5)",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        display: "flex",
        flexDirection: "column",
        gap: 12,
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", gap: 12 }}>
        <div
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            color: "var(--fg-3)",
            textTransform: "uppercase",
            letterSpacing: "0.04em",
          }}
        >
          {`// ${title}`}
        </div>
        {subtitle && (
          <div
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 10,
              color: "var(--fg-4)",
            }}
          >
            {subtitle}
          </div>
        )}
        {header && <div style={{ marginLeft: "auto" }}>{header}</div>}
      </div>
      {children}
    </section>
  );
}

function Grid({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))",
        gap: 16,
      }}
    >
      {children}
    </div>
  );
}

function Stat({
  label,
  value,
  sub,
}: {
  label: string;
  value: React.ReactNode;
  sub?: React.ReactNode;
}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
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
      <span
        className="tabular"
        style={{ fontFamily: "var(--font-mono)", fontSize: 18, color: "var(--fg-1)" }}
      >
        {value}
      </span>
      {sub && (
        <span style={{ fontFamily: "var(--font-mono)", fontSize: 10 }}>{sub}</span>
      )}
    </div>
  );
}

function Loading({ children, error }: { children: React.ReactNode; error?: boolean }) {
  return (
    <div
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        color: error ? "var(--no)" : "var(--fg-3)",
        padding: 12,
      }}
    >
      {children}
    </div>
  );
}
