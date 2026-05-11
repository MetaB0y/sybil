"use client";

import Link from "next/link";
import { notFound } from "next/navigation";
import { use } from "react";
import { BatchTheater } from "@/components/batch-theater";
import { PriceChart } from "@/components/price-chart";
import {
  formatCompactDollars,
  formatDate,
  formatProbability,
} from "@/lib/format/nanos";
import { useMarket } from "@/lib/markets/use-market";
import { usePriceHistory } from "@/lib/markets/use-price-history";
import { selectPricesByMarketId, useStore } from "@/lib/store";

type RouteParams = { id: string };

export default function MarketDetailPage({
  params,
}: {
  params: Promise<RouteParams>;
}) {
  const { id } = use(params);
  const marketId = Number(id);

  if (!Number.isFinite(marketId) || marketId < 0) {
    notFound();
  }

  const marketQ = useMarket(marketId);
  const historyQ = usePriceHistory(marketId);
  const prices = useStore(selectPricesByMarketId);
  const price = prices[marketId];

  const market = marketQ.data;
  const history = historyQ.data ?? [];

  return (
    <main
      style={{
        maxWidth: "var(--max-content)",
        margin: "0 auto",
        padding: "var(--space-6) var(--space-5) var(--space-9)",
        display: "grid",
        gridTemplateColumns: "minmax(0, 1fr) 360px",
        gap: "var(--space-6)",
      }}
    >
      <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-5)" }}>
        <Breadcrumb market={market?.name ?? `#${marketId}`} />

        {marketQ.isPending && <Placeholder>loading market…</Placeholder>}
        {marketQ.isError && (
          <Placeholder error>error: {String(marketQ.error)}</Placeholder>
        )}

        {market && (
          <>
            <Header market={market} />
            <CurrentPriceBlock yes={price?.yes} no={price?.no} />
            <ChartSection
              marketId={marketId}
              history={history}
              isPending={historyQ.isPending}
            />
            <DescriptionBlock market={market} />
          </>
        )}
      </div>

      <BatchTheater marketId={marketId} marketName={market?.name ?? `#${marketId}`} />
    </main>
  );
}

function Breadcrumb({ market }: { market: string }) {
  return (
    <div
      className="text-mono"
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-2)",
        fontSize: "var(--fs-12)",
        color: "var(--fg-3)",
      }}
    >
      <Link
        href="/"
        style={{
          color: "var(--fg-3)",
          textDecoration: "none",
        }}
      >
        all markets
      </Link>
      <span>→</span>
      <span style={{ color: "var(--fg-2)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
        {market}
      </span>
    </div>
  );
}

function Header({
  market,
}: {
  market: { name: string; status: string; volume_nanos?: string; expiry_timestamp_ms?: number | null; market_id: number };
}) {
  const statusLabel = (market.status || "active").toUpperCase();
  return (
    <header style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
      <div
        style={{
          display: "flex",
          gap: "var(--space-3)",
          alignItems: "baseline",
        }}
      >
        <span className="text-meta">#{market.market_id}</span>
        <span
          className="text-mono"
          style={{
            fontSize: "10px",
            letterSpacing: "var(--track-wide)",
            color: market.status === "active" ? "var(--fg-3)" : "var(--warn)",
            textTransform: "uppercase",
          }}
        >
          {statusLabel}
        </span>
      </div>
      <h1
        style={{
          fontFamily: "var(--font-display)",
          fontWeight: 600,
          fontSize: "var(--fs-32)",
          lineHeight: "var(--lh-32)",
          letterSpacing: "var(--track-tight)",
          margin: 0,
          color: "var(--fg-1)",
        }}
      >
        {market.name}
      </h1>
      <div
        className="text-mono"
        style={{
          display: "flex",
          gap: "var(--space-4)",
          fontSize: "var(--fs-12)",
          color: "var(--fg-3)",
        }}
      >
        <span>
          vol{" "}
          <span className="tabular" style={{ color: "var(--fg-2)" }}>
            {market.volume_nanos ? formatCompactDollars(market.volume_nanos) : "—"}
          </span>
        </span>
        <span>
          resolves{" "}
          <span className="tabular" style={{ color: "var(--fg-2)" }}>
            {formatDate(market.expiry_timestamp_ms)}
          </span>
        </span>
      </div>
    </header>
  );
}

function CurrentPriceBlock({ yes, no }: { yes: bigint | undefined; no: bigint | undefined }) {
  return (
    <section
      style={{
        display: "grid",
        gridTemplateColumns: "1fr 1fr",
        gap: "var(--space-4)",
      }}
    >
      <PriceCard tone="yes" label="yes" value={yes} />
      <PriceCard tone="no" label="no" value={no} />
    </section>
  );
}

function PriceCard({
  tone,
  label,
  value,
}: {
  tone: "yes" | "no";
  label: string;
  value: bigint | undefined;
}) {
  const color = tone === "yes" ? "var(--yes)" : "var(--no)";
  const faint = tone === "yes" ? "var(--yes-faint)" : "var(--no-faint)";
  return (
    <div
      style={{
        padding: "var(--space-4) var(--space-5)",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        boxShadow: "var(--shadow-inset-top)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-2)",
      }}
    >
      <div
        className="text-mono"
        style={{
          fontSize: "10px",
          letterSpacing: "var(--track-wide)",
          textTransform: "uppercase",
          color,
        }}
      >
        {label}
      </div>
      <div
        className="text-mono tabular"
        style={{
          fontSize: "var(--fs-40)",
          lineHeight: "var(--lh-40)",
          color: value != null ? "var(--fg-1)" : "var(--fg-4)",
        }}
      >
        {value != null ? formatProbability(value) : "—"}
      </div>
      {value != null && (
        <div style={{ marginTop: "var(--space-2)", height: 4, background: faint, borderRadius: 2, overflow: "hidden" }}>
          <span
            style={{
              display: "block",
              height: "100%",
              width: `${probabilityPercent(value)}%`,
              background: color,
            }}
          />
        </div>
      )}
    </div>
  );
}

function ChartSection({
  marketId,
  history,
  isPending,
}: {
  marketId: number;
  history: import("@/lib/markets/use-price-history").PricePoint[];
  isPending: boolean;
}) {
  return (
    <section
      style={{
        padding: "var(--space-4) var(--space-5)",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        boxShadow: "var(--shadow-inset-top)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-3)",
      }}
    >
      <div className="eyebrow">{"// yes probability"}</div>
      {isPending && history.length === 0 ? (
        <div
          className="text-mono"
          style={{
            height: 280,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--fg-3)",
          }}
        >
          loading…
        </div>
      ) : history.length === 0 ? (
        <div
          className="text-mono"
          style={{
            height: 280,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--fg-4)",
          }}
        >
          no clearing history yet — chart will populate as batches clear.
        </div>
      ) : (
        <PriceChart marketId={marketId} history={history} />
      )}
    </section>
  );
}

function DescriptionBlock({
  market,
}: {
  market: { description?: string | null; resolution_criteria?: string | null; external_url?: string | null };
}) {
  if (!market.description && !market.resolution_criteria && !market.external_url) {
    return null;
  }
  return (
    <section
      style={{
        padding: "var(--space-5)",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        boxShadow: "var(--shadow-inset-top)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-3)",
      }}
    >
      {market.description && (
        <div>
          <div className="eyebrow" style={{ marginBottom: "var(--space-2)" }}>
            {"// description"}
          </div>
          <p
            style={{
              fontSize: "var(--fs-14)",
              lineHeight: "var(--lh-20)",
              color: "var(--fg-2)",
              margin: 0,
              whiteSpace: "pre-wrap",
            }}
          >
            {market.description}
          </p>
        </div>
      )}
      {market.resolution_criteria && (
        <div>
          <div className="eyebrow" style={{ marginBottom: "var(--space-2)" }}>
            {"// resolution"}
          </div>
          <p
            style={{
              fontSize: "var(--fs-13)",
              lineHeight: "var(--lh-18)",
              color: "var(--fg-3)",
              margin: 0,
              whiteSpace: "pre-wrap",
            }}
          >
            {market.resolution_criteria}
          </p>
        </div>
      )}
      {market.external_url && (
        <a
          href={market.external_url}
          target="_blank"
          rel="noreferrer noopener"
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: "var(--space-2)",
            color: "var(--accent)",
            textDecoration: "none",
            fontFamily: "var(--font-mono)",
            fontSize: "var(--fs-12)",
            letterSpacing: "var(--track-wide)",
            textTransform: "uppercase",
          }}
        >
          source ↗
        </a>
      )}
    </section>
  );
}

function probabilityPercent(nanos: bigint): number {
  return Number(nanos) / 1e7;
}

function Placeholder({ children, error }: { children: React.ReactNode; error?: boolean }) {
  return (
    <div
      className="text-mono"
      style={{
        color: error ? "var(--no)" : "var(--fg-3)",
        padding: "var(--space-6) 0",
        textAlign: "center",
      }}
    >
      {children}
    </div>
  );
}
