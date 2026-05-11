"use client";

import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import { formatInt } from "@/lib/format/nanos";

type WsState =
  | { kind: "idle" }
  | { kind: "open" }
  | { kind: "ok"; version: number; height?: number }
  | { kind: "error"; reason: string };

function useFirstBlockEnvelope() {
  const [state, setState] = useState<WsState>({ kind: "idle" });

  /* eslint-disable react-hooks/set-state-in-effect -- smoke hook; real WS owner in Milestone B uses a store */
  useEffect(() => {
    const base = process.env.NEXT_PUBLIC_WS_BASE;
    if (!base) {
      setState({ kind: "error", reason: "NEXT_PUBLIC_WS_BASE not set" });
      return;
    }
    const ws = new WebSocket(`${base}/v1/blocks/ws`);
    setState({ kind: "open" });
    ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        const height =
          msg?.payload?.data?.height ?? msg?.payload?.up_to_height ?? undefined;
        setState({ kind: "ok", version: msg?.v ?? 0, height });
      } catch (err) {
        setState({
          kind: "error",
          reason: err instanceof Error ? err.message : "parse failed",
        });
      } finally {
        ws.close();
      }
    };
    ws.onerror = () =>
      setState({ kind: "error", reason: "websocket connection failed" });
    return () => {
      if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING) {
        ws.close();
      }
    };
  }, []);
  /* eslint-enable react-hooks/set-state-in-effect */

  return state;
}

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

  const ws = useFirstBlockEnvelope();

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
        <div className="eyebrow">{"// ws · /v1/blocks/ws"}</div>
        <div style={{ marginTop: "var(--space-2)" }}>
          {ws.kind === "idle" && <span style={dimMono}>idle</span>}
          {ws.kind === "open" && <span style={dimMono}>connecting…</span>}
          {ws.kind === "ok" && (
            <span className="text-mono" style={{ fontSize: "var(--fs-20)" }}>
              WS OK · v{ws.version}
              {ws.height != null ? ` · height=${formatInt(ws.height)}` : ""}
            </span>
          )}
          {ws.kind === "error" && <span style={errMono}>error: {ws.reason}</span>}
        </div>
      </section>

      <footer style={{ marginTop: "var(--space-4)" }}>
        <div className="text-annotation">scaffolding step 11 · throwaway page · replaced when real markets ship</div>
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
