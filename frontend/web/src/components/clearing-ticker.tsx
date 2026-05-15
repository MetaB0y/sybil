"use client";

import Link from "next/link";
import { useMemo } from "react";
import { formatCents, formatInt } from "@/lib/format/nanos";
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
        alignItems: "stretch",
        height: 36,
        background: "var(--bg-1)",
        borderTop: "1px solid var(--border-1)",
        borderBottom: "1px solid var(--border-1)",
        overflow: "hidden",
      }}
    >
      {/* Accented badge — handoff's anchored "Last batch · #N" cell */}
      <div
        style={{
          flexShrink: 0,
          display: "inline-flex",
          alignItems: "center",
          gap: "var(--space-2)",
          padding: "0 var(--space-4)",
          background: "var(--accent-soft)",
          color: "var(--accent)",
          borderRight: "1px solid var(--border-1)",
          fontFamily: "var(--font-mono)",
          fontSize: "11px",
          letterSpacing: "var(--track-wide)",
          textTransform: "uppercase",
        }}
      >
        <span
          aria-hidden
          style={{
            width: 6,
            height: 6,
            borderRadius: "50%",
            background: "var(--accent)",
          }}
        />
        <span>Last batch</span>
        <span style={{ color: "var(--accent)", opacity: 0.6 }}>·</span>
        <span className="tabular">
          #{latest?.height != null ? formatInt(latest.height) : "—"}
        </span>
      </div>

      {/* Bordered cell row, scrolls horizontally with mask fade at edge */}
      {entries.length === 0 ? (
        <span
          className="text-mono"
          style={{
            display: "inline-flex",
            alignItems: "center",
            padding: "0 var(--space-4)",
            color: "var(--fg-4)",
            fontSize: "var(--fs-12)",
          }}
        >
          no fills this block
        </span>
      ) : (
        <div
          style={{
            display: "flex",
            overflowX: "auto",
            scrollbarWidth: "none",
            WebkitMaskImage:
              "linear-gradient(to right, black 0, black calc(100% - 32px), transparent 100%)",
            maskImage:
              "linear-gradient(to right, black 0, black calc(100% - 32px), transparent 100%)",
          }}
        >
          {entries.map((e) => (
            <TickerCell key={e.id} id={e.id} name={e.name} yes={e.yes} />
          ))}
        </div>
      )}
    </div>
  );
}

function TickerCell({
  id,
  name,
  yes,
}: {
  id: number;
  name: string;
  yes: bigint;
}) {
  const short = trimChipName(name);
  // Color the cents by which side of 50% the price sits on — the same visual
  // signal the handoff conveys with delta24, without needing a per-item fetch.
  const HALF = 500_000_000n;
  const tone = yes >= HALF ? "var(--yes)" : "var(--no)";
  return (
    <Link
      href={`/m/${id}`}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: "var(--space-2)",
        flexShrink: 0,
        padding: "0 var(--space-4)",
        height: 36,
        borderRight: "1px solid var(--border-1)",
        fontFamily: "var(--font-mono)",
        fontSize: "var(--fs-12)",
        color: "var(--fg-3)",
        textDecoration: "none",
        whiteSpace: "nowrap",
        transition: "background var(--dur-fast) var(--ease-standard)",
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = "var(--surface-1)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = "transparent";
      }}
    >
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
          color: tone,
          fontWeight: 600,
        }}
      >
        {formatCents(yes)}
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
