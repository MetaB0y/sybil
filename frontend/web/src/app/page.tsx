"use client";

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import { formatInt } from "@/lib/format/nanos";
import {
  selectConnection,
  selectLatestHeight,
  selectMarketCount,
  useStore,
} from "@/lib/store";

export default function Home() {
  const health = useQuery({
    queryKey: ["health"],
    queryFn: async () => {
      const { data, error } = await api.GET("/v1/health");
      if (error || !data) throw new Error("health request failed");
      return data;
    },
  });

  const markets = useQuery({
    queryKey: ["markets-summary"],
    queryFn: async () => {
      const { data, error } = await api.GET("/v1/markets/summary");
      if (error || !data) throw new Error("markets request failed");
      return data;
    },
  });

  const connection = useStore(selectConnection);
  const liveHeight = useStore(selectLatestHeight);
  const livePriceMarkets = useStore(selectMarketCount);

  return (
    <main
      style={{
        minHeight: "100vh",
        background: "var(--bg-1)",
        color: "var(--fg-1)",
        fontFamily: "var(--font-sans)",
        padding: "var(--space-7) var(--space-5)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-6)",
        maxWidth: "720px",
        margin: "0 auto",
      }}
    >
      <header style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
        <div className="eyebrow">{"// scaffolding smoke"}</div>
        <h1
          style={{
            fontFamily: "var(--font-display)",
            fontWeight: 700,
            fontSize: "var(--fs-72)",
            lineHeight: "var(--lh-72)",
            letterSpacing: "var(--track-tight)",
            color: "var(--accent)",
            margin: 0,
            textTransform: "uppercase",
          }}
        >
          Sybil
        </h1>
        <p className="text-annotation">the first prediction market built on frequent batch auctions</p>
      </header>

      <section style={panelStyle}>
        <div className="eyebrow">{"// rest · /v1/health"}</div>
        <div style={{ marginTop: "var(--space-2)" }}>
          {health.isPending && <span style={dimMono}>pending…</span>}
          {health.isError && <span style={errMono}>error: {String(health.error)}</span>}
          {health.data && (
            <span className="text-mono" style={{ fontSize: "var(--fs-20)" }}>
              status={health.data.status} · height={health.data.height != null ? formatInt(health.data.height) : "—"}
            </span>
          )}
        </div>
      </section>

      <section style={panelStyle}>
        <div className="eyebrow">{"// rest · /v1/markets/summary"}</div>
        <div style={{ marginTop: "var(--space-2)" }}>
          {markets.isPending && <span style={dimMono}>pending…</span>}
          {markets.isError && <span style={errMono}>error: {String(markets.error)}</span>}
          {markets.data && (
            <span className="text-mono" style={{ fontSize: "var(--fs-20)" }}>
              {markets.data.length} markets
            </span>
          )}
        </div>
      </section>

      <section style={panelStyle}>
        <div className="eyebrow">{"// store · ws → zustand"}</div>
        <div style={{ marginTop: "var(--space-2)" }}>
          <span className="text-mono" style={{ fontSize: "var(--fs-20)" }}>
            state={connection.state}
            {liveHeight != null ? ` · height=${formatInt(liveHeight)}` : ""}
            {livePriceMarkets > 0 ? ` · live_prices=${livePriceMarkets}` : ""}
          </span>
        </div>
      </section>

      <footer style={{ marginTop: "var(--space-4)" }}>
        <div className="text-annotation">milestone B · store-backed · throwaway until markets ship</div>
      </footer>
    </main>
  );
}

const panelStyle: React.CSSProperties = {
  background: "var(--surface-1)",
  border: "1px solid var(--border-1)",
  borderRadius: "var(--radius-lg)",
  padding: "var(--space-4) var(--space-5)",
  boxShadow: "var(--shadow-inset-top)",
};

const dimMono: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  color: "var(--fg-3)",
  fontSize: "var(--fs-14)",
};

const errMono: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  color: "var(--no)",
  fontSize: "var(--fs-14)",
};
