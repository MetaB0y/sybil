"use client";

/**
 * Collapsible "last batches · stats" panel. Wraps the same hook
 * `useBatchWindowStats` we built for `/m-dev/[id]`, in a Disclosure with the
 * 1 / 5 / 10 / 100 window selector. Matches `LastNStats` at
 * `fed-fba-panel.jsx:99`: a bordered 2×2 stat grid topped by the window
 * selector, footed by a match-rate progress bar.
 *
 * Mocked values shown (all wrapped <MockValue>):
 *  - traders placed (OPEN_QUESTIONS #8)
 *  - traders matched, per-market scoping (#5)
 *  - volume placed (#8)
 *  - volume matched, per-market scoping (#5)
 */

import { useState } from "react";
import { MockValue } from "@/components/mock-value";
import { formatCompactDollars, formatInt } from "@/lib/format/nanos";
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

  const placed = stats.uniqueTradersPlaced;
  const matched = stats.uniqueTradersMatched;
  const matchRate = placed > 0 ? Math.round((matched / placed) * 100) : 0;

  return (
    <div
      style={{
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: 8,
        overflow: "hidden",
      }}
    >
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        style={{
          width: "100%",
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          gap: 12,
          padding: "12px 16px",
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

      {open && (
        <div style={{ padding: "4px 16px 16px" }}>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              gap: 12,
              marginBottom: 8,
            }}
          >
            <span
              style={{
                fontFamily: "var(--font-mono)",
                fontSize: 10,
                color: "var(--fg-3)",
                textTransform: "uppercase",
                letterSpacing: "0.04em",
              }}
            >
              last batches
            </span>
            <WindowSelector value={windowSize} onChange={setWindowSize} />
          </div>

          {partial && (
            <div
              style={{
                fontFamily: "var(--font-mono)",
                fontSize: 10,
                color: "var(--fg-4)",
                marginBottom: 8,
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
              gap: 1,
              background: "var(--border-1)",
              border: "1px solid var(--border-1)",
              borderRadius: 4,
            }}
          >
            <Cell
              label="traders placed"
              value={
                <MockValue hint="placed-trader counts not on the wire (OPEN_QUESTIONS #8)">
                  {formatInt(placed)}
                </MockValue>
              }
            />
            <Cell
              label="traders matched"
              value={
                <MockValue hint="per-market scoping is mocked (OPEN_QUESTIONS #5)">
                  {formatInt(matched)}
                </MockValue>
              }
            />
            <Cell
              label="volume placed"
              value={
                <MockValue hint="no placed-volume notional on the wire (OPEN_QUESTIONS #8)">
                  {formatCompactDollars(stats.volumePlacedNanos)}
                </MockValue>
              }
            />
            <Cell
              label="volume matched"
              value={
                <MockValue hint="per-market scoping is mocked (OPEN_QUESTIONS #5)">
                  {formatCompactDollars(stats.volumeMatchedNanos)}
                </MockValue>
              }
            />
          </div>

          <div
            style={{
              marginTop: 10,
              display: "flex",
              alignItems: "center",
              gap: 8,
              fontFamily: "var(--font-mono)",
              fontSize: 10,
              color: "var(--fg-3)",
            }}
          >
            <span>match rate</span>
            <div
              style={{
                flex: 1,
                height: 3,
                background: "var(--bg-2)",
                borderRadius: 2,
                overflow: "hidden",
              }}
            >
              <div
                style={{
                  width: `${matchRate}%`,
                  height: "100%",
                  background: "var(--accent)",
                }}
              />
            </div>
            <span style={{ color: "var(--fg-1)" }}>{matchRate}%</span>
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
}: {
  label: string;
  value: React.ReactNode;
}) {
  return (
    <div
      style={{
        background: "var(--surface-1)",
        padding: "10px 12px",
        display: "flex",
        flexDirection: "column",
        gap: 4,
      }}
    >
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 9.5,
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
    </div>
  );
}
