"use client";

import Link from "next/link";
import { useMemo } from "react";
import { formatInt, formatProbability } from "@/lib/format/nanos";
import type { Market } from "@/lib/markets/use-markets";
import { selectLatestBlock, useStore } from "@/lib/store";
import { parseNanos } from "@/lib/format/nanos";

type Props = {
  /** Lookup table for resolving market_id → name. */
  marketsById: Map<number, Market>;
};

/**
 * ClearingTicker — 36px strip showing the markets that cleared in the last
 * committed block. Re-renders every block. If clearing_prices_nanos is
 * empty for that block (no fills landed), shows a neutral idle state.
 *
 * Visual is a horizontal row of chips: `#ID name 87.3%`. Overflows with
 * native horizontal scroll (no marquee animation — content changes every
 * 2s, a marquee would visually fight the data update).
 */
export function ClearingTicker({ marketsById }: Props) {
  const latest = useStore(selectLatestBlock);

  const entries = useMemo(() => {
    if (!latest?.clearing_prices_nanos) return [];
    const out: Array<{ id: number; name: string; yes: bigint }> = [];
    for (const [key, arr] of Object.entries(latest.clearing_prices_nanos)) {
      const id = Number(key);
      if (!Number.isFinite(id)) continue;
      const yesStr = arr[0];
      if (yesStr == null) continue;
      const m = marketsById.get(id);
      out.push({
        id,
        name: m?.name ?? `#${id}`,
        yes: parseNanos(yesStr),
      });
    }
    // Most extreme moves first (anything far from 50% is "interesting").
    out.sort((a, b) => {
      const da = absDistanceFromHalf(a.yes);
      const db = absDistanceFromHalf(b.yes);
      return da > db ? -1 : da < db ? 1 : 0;
    });
    return out;
  }, [latest, marketsById]);

  return (
    <div
      style={{
        position: "sticky",
        top: "var(--nav-height)",
        zIndex: 40,
        display: "flex",
        alignItems: "center",
        gap: "var(--space-3)",
        height: 36,
        padding: "0 var(--space-5)",
        background: "rgba(10,14,18,0.72)",
        backdropFilter: "var(--blur-nav)",
        WebkitBackdropFilter: "var(--blur-nav)",
        borderBottom: "1px solid var(--border-1)",
        overflow: "hidden",
      }}
    >
      <span
        className="text-mono"
        style={{
          flexShrink: 0,
          color: "var(--fg-3)",
          fontSize: "10px",
          letterSpacing: "var(--track-wide)",
          textTransform: "uppercase",
        }}
      >
        cleared
      </span>
      <span
        className="text-mono tabular"
        style={{
          flexShrink: 0,
          color: "var(--fg-1)",
          fontSize: "var(--fs-12)",
        }}
      >
        #{latest?.height != null ? formatInt(latest.height) : "—"}
      </span>
      <span
        style={{
          flexShrink: 0,
          width: 1,
          height: 16,
          background: "var(--border-2)",
        }}
      />

      {entries.length === 0 ? (
        <span className="text-mono" style={{ color: "var(--fg-4)", fontSize: "var(--fs-12)" }}>
          no fills this block
        </span>
      ) : (
        <div
          style={{
            display: "flex",
            gap: "var(--space-3)",
            overflowX: "auto",
            scrollbarWidth: "none",
            // Hide webkit scrollbar
            WebkitMaskImage:
              "linear-gradient(to right, transparent 0, black 16px, black calc(100% - 32px), transparent 100%)",
            maskImage:
              "linear-gradient(to right, transparent 0, black 16px, black calc(100% - 32px), transparent 100%)",
          }}
        >
          {entries.map((e) => (
            <TickerChip key={e.id} id={e.id} name={e.name} yes={e.yes} />
          ))}
        </div>
      )}
    </div>
  );
}

function TickerChip({ id, name, yes }: { id: number; name: string; yes: bigint }) {
  const short = trimChipName(name);
  return (
    <Link
      href={`/m/${id}`}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: "var(--space-2)",
        flexShrink: 0,
        height: 22,
        padding: "0 var(--space-2)",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-sm)",
        fontFamily: "var(--font-mono)",
        fontSize: "11px",
        color: "var(--fg-2)",
        textDecoration: "none",
        whiteSpace: "nowrap",
        transition: "border-color var(--dur-fast) var(--ease-standard)",
      }}
    >
      <span style={{ color: "var(--fg-3)" }}>#{id}</span>
      <span
        style={{
          maxWidth: 220,
          overflow: "hidden",
          textOverflow: "ellipsis",
        }}
      >
        {short}
      </span>
      <span
        className="tabular"
        style={{
          color: "var(--yes)",
          fontWeight: 600,
        }}
      >
        {formatProbability(yes)}
      </span>
    </Link>
  );
}

function trimChipName(name: string): string {
  // For "Group Name: Outcome" → "Outcome"; otherwise return as-is.
  const idx = name.indexOf(":");
  if (idx > 0 && idx < name.length - 2) return name.slice(idx + 1).trim();
  return name;
}

function absDistanceFromHalf(nanos: bigint): bigint {
  const HALF = 500_000_000n;
  return nanos > HALF ? nanos - HALF : HALF - nanos;
}
