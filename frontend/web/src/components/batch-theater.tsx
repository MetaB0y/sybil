"use client";

import { useEffect, useRef, useState } from "react";
import { formatInt, formatProbability } from "@/lib/format/nanos";
import {
  selectConnection,
  selectLatestBlock,
  selectPricesByMarketId,
  useStore,
} from "@/lib/store";

const BLOCK_MS = 2000;

type Props = {
  marketId: number;
  marketName: string;
};

/**
 * BatchTheater — the right-rail showpiece. Big block clock with a 2s
 * countdown bar that resets on every new block, current indicative
 * clearing price for this market, and a disabled order-entry stub.
 *
 * Aligned with the design system: linear easing keyed to block.height
 * (not wall-clock spring physics — would jank at 2s cadence).
 */
export function BatchTheater({ marketId, marketName }: Props) {
  const connection = useStore(selectConnection);
  const latest = useStore(selectLatestBlock);
  const prices = useStore(selectPricesByMarketId);
  const price = prices[marketId];

  const [progress, setProgress] = useState(0);
  const rafRef = useRef<number | null>(null);
  const anchorRef = useRef<number | null>(null);

  /* eslint-disable react-hooks/set-state-in-effect, react-hooks/exhaustive-deps -- reset on new block */
  useEffect(() => {
    if (latest == null) return;
    anchorRef.current = performance.now();
    setProgress(0);
  }, [latest?.height]);
  /* eslint-enable */

  useEffect(() => {
    const step = () => {
      if (anchorRef.current != null) {
        const elapsed = performance.now() - anchorRef.current;
        setProgress(Math.min(1, elapsed / BLOCK_MS));
      }
      rafRef.current = requestAnimationFrame(step);
    };
    rafRef.current = requestAnimationFrame(step);
    return () => {
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
    };
  }, []);

  const isLive = connection.state === "live";
  const isReplaying = connection.state === "replaying";
  const stateLabel =
    connection.state === "live"
      ? "LIVE"
      : connection.state.toUpperCase();
  const dotColor = isLive ? "var(--accent)" : isReplaying ? "var(--warn)" : "var(--fg-3)";

  // Block-level FillResponse doesn't carry per-market deltas (that's on the
  // per-account /v1/accounts/{id}/fills endpoint), so we can't directly count
  // fills *for this market* from the broadcast. Instead surface whether this
  // market cleared in the latest block (its key appears in clearing_prices).
  const clearedThisBlock =
    latest?.clearing_prices_nanos != null &&
    String(marketId) in latest.clearing_prices_nanos;
  const blockTimestampMs = latest?.timestamp_ms ?? null;

  return (
    <aside
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-4)",
        position: "sticky",
        top: `calc(var(--nav-height) + 36px + var(--space-4))`,
      }}
    >
      {/* Theater header */}
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: "var(--space-2)",
          padding: "var(--space-4) var(--space-5)",
          background: "var(--surface-1)",
          border: "1px solid var(--border-2)",
          borderRadius: "var(--radius-lg)",
          boxShadow: "var(--shadow-inset-top)",
          overflow: "hidden",
          position: "relative",
        }}
      >
        {/* Status row */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <span
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: "var(--space-2)",
            }}
          >
            <span
              aria-hidden
              style={{
                width: 8,
                height: 8,
                borderRadius: "50%",
                background: dotColor,
                boxShadow: isLive
                  ? "0 0 8px var(--accent-soft)"
                  : "0 0 6px transparent",
                animation: isLive
                  ? "none"
                  : "sybil-pulse 1.6s ease-in-out infinite",
              }}
            />
            <span
              className="text-mono"
              style={{
                fontSize: "10px",
                letterSpacing: "var(--track-wide)",
                textTransform: "uppercase",
                color: isLive ? "var(--accent)" : "var(--warn)",
              }}
            >
              {stateLabel}
            </span>
          </span>
          <span
            className="text-mono tabular"
            style={{
              fontSize: "var(--fs-12)",
              color: "var(--fg-3)",
            }}
          >
            block #{latest?.height != null ? formatInt(latest.height) : "—"}
          </span>
        </div>

        {/* Big indicative YES price */}
        <div
          style={{
            marginTop: "var(--space-2)",
            display: "flex",
            alignItems: "baseline",
            gap: "var(--space-3)",
          }}
        >
          <span
            className="text-mono tabular"
            style={{
              fontSize: "var(--fs-72)",
              lineHeight: "var(--lh-72)",
              fontWeight: 600,
              letterSpacing: "var(--track-mono)",
              color: price ? "var(--fg-1)" : "var(--fg-4)",
            }}
          >
            {price ? formatProbability(price.yes) : "—"}
          </span>
          <span
            className="text-mono"
            style={{
              fontSize: "var(--fs-12)",
              letterSpacing: "var(--track-wide)",
              color: "var(--yes)",
              textTransform: "uppercase",
            }}
          >
            yes
          </span>
        </div>

        <div className="text-annotation" style={{ marginTop: "var(--space-1)" }}>
          uniform clearing · next batch in
          <span className="text-mono tabular" style={{ color: "var(--fg-2)", marginLeft: 4 }}>
            {Math.max(0, BLOCK_MS - Math.round(progress * BLOCK_MS))}ms
          </span>
        </div>

        {/* Next-batch progress bar */}
        <div
          aria-hidden
          style={{
            position: "absolute",
            bottom: 0,
            left: 0,
            height: 2,
            width: `${progress * 100}%`,
            background: "var(--accent)",
            opacity: 0.7,
            transition: "none",
          }}
        />
      </div>

      {/* Batch composition card */}
      <div
        style={{
          padding: "var(--space-4) var(--space-5)",
          background: "var(--surface-1)",
          border: "1px solid var(--border-1)",
          borderRadius: "var(--radius-lg)",
          boxShadow: "var(--shadow-inset-top)",
        }}
      >
        <div
          className="eyebrow"
          style={{ marginBottom: "var(--space-3)" }}
        >
          {"// last batch"}
        </div>
        <KV label="Cleared this block">
          <span
            className="text-mono tabular"
            style={{ color: clearedThisBlock ? "var(--yes)" : "var(--fg-3)" }}
          >
            {clearedThisBlock ? "yes" : "no"}
          </span>
        </KV>
        <KV label="Indicative no">
          <span
            className="text-mono tabular"
            style={{ color: price ? "var(--no)" : "var(--fg-4)" }}
          >
            {price ? formatProbability(price.no) : "—"}
          </span>
        </KV>
        <KV label="Last block at">
          <span className="text-mono tabular">
            {blockTimestampMs != null
              ? new Date(blockTimestampMs).toLocaleTimeString("en-US", {
                  hour12: false,
                })
              : "—"}
          </span>
        </KV>
      </div>

      {/* Order entry placeholder */}
      <div
        style={{
          padding: "var(--space-4) var(--space-5)",
          background: "var(--surface-1)",
          border: "1px solid var(--border-1)",
          borderRadius: "var(--radius-lg)",
          boxShadow: "var(--shadow-inset-top)",
        }}
      >
        <div className="eyebrow" style={{ marginBottom: "var(--space-3) " }}>
          {"// place batch order"}
        </div>
        <div className="text-annotation">
          order entry — wired when wallet/auth lands. for now, watch the next batch clear on its own.
        </div>
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "1fr 1fr",
            gap: "var(--space-3)",
            marginTop: "var(--space-4)",
          }}
        >
          <DisabledOrderButton tone="yes" label="buy yes" sub={marketName} />
          <DisabledOrderButton tone="no" label="buy no" sub={marketName} />
        </div>
      </div>
    </aside>
  );
}

function KV({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "space-between",
        alignItems: "baseline",
        padding: "var(--space-2) 0",
        borderTop: "1px solid var(--border-1)",
      }}
    >
      <span className="text-meta">{label}</span>
      <span style={{ fontSize: "var(--fs-13)", color: "var(--fg-1)" }}>
        {children}
      </span>
    </div>
  );
}

function DisabledOrderButton({
  tone,
  label,
  sub,
}: {
  tone: "yes" | "no";
  label: string;
  sub: string;
}) {
  const color = tone === "yes" ? "var(--yes)" : "var(--no)";
  return (
    <button
      type="button"
      disabled
      title={sub}
      style={{
        height: 40,
        padding: "0 var(--space-3)",
        background: tone === "yes" ? "var(--yes-faint)" : "var(--no-faint)",
        color,
        border: `1px solid color-mix(in srgb, ${color} 24%, transparent)`,
        borderRadius: "var(--radius-md)",
        fontFamily: "var(--font-sans)",
        fontSize: "var(--fs-13)",
        fontWeight: 600,
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
        cursor: "not-allowed",
        opacity: 0.7,
      }}
    >
      {label}
    </button>
  );
}
