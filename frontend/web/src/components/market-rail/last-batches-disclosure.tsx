"use client";

/**
 * Collapsible "last batches · stats" panel. Wraps the same hook
 * `useBatchWindowStats` we built for `/m-dev/[id]`, but in a Disclosure with
 * the 1 / 5 / 10 / 100 selector inline. Matches the Disclosure block at
 * `fed-variations.jsx:163`.
 *
 * Mocked values shown (all wrapped <MockValue>):
 *  - unique placers (OPEN_QUESTIONS #8)
 *  - unique matched, per-market scoping (#5)
 *  - volume placed (#8)
 *  - volume matched, per-market scoping (#5)
 *
 * Chain-wide totals (without per-market scoping) are real and shown as
 * the small "chain-wide" sub-stat line.
 */

import { useState } from "react";
import { MockValue } from "@/components/mock-value";
import {
  formatCompactDollars,
  formatDollars,
  formatInt,
} from "@/lib/format/nanos";
import type { WindowSize } from "@/lib/market-detail/types";
import {
  WINDOW_SIZES,
  useBatchWindowStats,
} from "@/lib/market-detail/use-batch-windows";

export function LastBatchesDisclosure({ marketId }: { marketId: number }) {
  const [open, setOpen] = useState(false);
  const [windowSize, setWindowSize] = useState<WindowSize>(10);
  const stats = useBatchWindowStats(marketId, windowSize);
  const partial = stats.actualBlockCount < windowSize;

  return (
    <div
      style={{
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: 8,
        overflow: "hidden",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "12px 16px",
        }}
      >
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          aria-expanded={open}
          style={{
            flex: 1,
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            gap: 12,
            padding: 0,
            background: "transparent",
            border: 0,
            cursor: "pointer",
            color: "var(--fg-2)",
            fontFamily: "var(--font-mono)",
            fontSize: 10.5,
            textTransform: "uppercase",
            letterSpacing: "0.04em",
            textAlign: "left",
          }}
        >
          <span>last batches · stats</span>
          <span style={{ color: "var(--fg-4)" }}>{open ? "–" : "+"}</span>
        </button>
        <WindowSelector value={windowSize} onChange={setWindowSize} />
      </div>
      {open && (
        <div style={{ padding: "4px 16px 16px" }}>
          {partial && (
            <div
              style={{
                fontFamily: "var(--font-mono)",
                fontSize: 10,
                color: "var(--fg-4)",
                marginBottom: 10,
              }}
            >
              showing {stats.actualBlockCount} of {windowSize} (buffer holds{" "}
              {stats.firstHeight ?? "?"}–{stats.lastHeight ?? "?"})
            </div>
          )}
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "1fr 1fr",
              rowGap: 10,
              columnGap: 16,
            }}
          >
            <Cell
              label="unique placed"
              value={
                <MockValue hint="placed-trader counts not on the wire (OPEN_QUESTIONS #8)">
                  {formatInt(stats.uniqueTradersPlaced)}
                </MockValue>
              }
            />
            <Cell
              label="unique matched"
              value={
                <MockValue hint="per-market scoping is mocked (OPEN_QUESTIONS #5)">
                  {formatInt(stats.uniqueTradersMatched)}
                </MockValue>
              }
              sub={`chain ${formatInt(stats.uniqueTradersMatchedChainWide)}`}
            />
            <Cell
              label="vol placed"
              value={
                <MockValue hint="no placed-volume notional on the wire (OPEN_QUESTIONS #8)">
                  {formatCompactDollars(stats.volumePlacedNanos)}
                </MockValue>
              }
            />
            <Cell
              label="vol matched"
              value={
                <MockValue hint="per-market scoping is mocked (OPEN_QUESTIONS #5)">
                  {formatCompactDollars(stats.volumeMatchedNanos)}
                </MockValue>
              }
              sub={`chain ${formatDollars(stats.volumeMatchedChainWideNanos, { decimals: 0 })}`}
            />
          </div>
        </div>
      )}
    </div>
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
    <span
      style={{
        display: "inline-flex",
        gap: 1,
        padding: 1,
        background: "var(--bg-2)",
        border: "1px solid var(--border-1)",
        borderRadius: 3,
      }}
    >
      {WINDOW_SIZES.map((n) => {
        const active = n === value;
        return (
          <button
            key={n}
            type="button"
            onClick={() => onChange(n)}
            style={{
              padding: "2px 8px",
              borderRadius: 2,
              border: 0,
              cursor: "pointer",
              background: active ? "var(--surface-2)" : "transparent",
              color: active ? "var(--fg-1)" : "var(--fg-3)",
              fontFamily: "var(--font-mono)",
              fontSize: 10,
            }}
          >
            {n}
          </button>
        );
      })}
    </span>
  );
}

function Cell({
  label,
  value,
  sub,
}: {
  label: string;
  value: React.ReactNode;
  sub?: string;
}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
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
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 16,
          color: "var(--fg-1)",
        }}
      >
        {value}
      </span>
      {sub && (
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            color: "var(--fg-4)",
          }}
        >
          {sub}
        </span>
      )}
    </div>
  );
}
