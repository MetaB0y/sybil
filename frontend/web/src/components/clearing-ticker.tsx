"use client";

import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import {
  formatAge,
  formatCompactDollars,
  formatInt,
  parseNanos,
} from "@/lib/format/nanos";
import type { Market } from "@/lib/markets/use-markets";
import { selectLatestBlock, selectRecentBlocks, useStore } from "@/lib/store";

type Props = {
  /** Lookup table for resolving market_id → name. */
  marketsById: Map<number, Market>;
};

/** Most recent clears to keep on the strip. */
const MAX_TICKER_ITEMS = 40;
/** Below this many items the content won't overflow, so don't animate. */
const MARQUEE_MIN_ITEMS = 6;

/** One market clearing in one batch — a single "trade" entry on the ticker. */
type ClearEvent = {
  key: string;
  id: number;
  name: string;
  /** Clearing YES price this batch (nanos). */
  yes: bigint;
  /** Matched volume this market contributed this batch (nanos, $). */
  volNanos: bigint;
  /** YES price change vs this market's previous clear, in pp. Always a real
   *  (non-flat) move — flat and first-seen clears are filtered out of the feed. */
  ppChange: number;
  /** Block timestamp (epoch ms) → "seconds ago". */
  ts: number;
};

/**
 * ClearingTicker — a continuously scrolling strip of recent clears across the
 * last committed blocks. Each entry is one market whose clearing price moved
 * in a batch (flat / first-seen clears are skipped):
 * `name  $vol  ±pp  age` (title · volume · price change · age). The buffer
 * accumulates across blocks (rather
 * than being replaced each batch) so the marquee has stable content to scroll;
 * new clears enter at the head and old ones fall off after MAX_TICKER_ITEMS.
 *
 * Side (buy/sell) is intentionally absent: block-level fills carry no
 * market_id or side, so a global feed can only speak in per-batch clears.
 */
export function ClearingTicker({ marketsById }: Props) {
  const latest = useStore(selectLatestBlock);
  const recent = useStore(selectRecentBlocks);
  const [paused, setPaused] = useState(false);

  // Live "now" so the age column ticks every second between blocks.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);

  const events = useMemo(() => {
    // Walk oldest → newest so each market's previous clear price is known when
    // we reach its next clear (price change is vs the prior *clearing* batch).
    const prevYes = new Map<number, bigint>();
    const all: ClearEvent[] = [];
    for (const b of [...recent].reverse()) {
      const cp = b.clearing_prices_nanos;
      if (!cp) continue;
      for (const [key, arr] of Object.entries(cp)) {
        const id = Number(key);
        if (!Number.isFinite(id)) continue;
        const yesStr = arr[0];
        if (yesStr == null) continue;
        const volStr = b.by_market?.[key]?.volume_nanos;
        const volNanos = volStr == null ? 0n : parseNanos(volStr);
        // Only batches where this market actually traded — a "purchase".
        if (volNanos <= 0n) continue;
        const yes = parseNanos(yesStr);
        const prior = prevYes.get(id);
        const ppChange = prior == null ? null : Number(yes - prior) / 1e7;
        prevYes.set(id, yes);
        // Only surface clears that actually moved the price: skip a market's
        // first clear in the window (no prior to diff) and flat ticks (which
        // would read "0.0pp"). prevYes is updated above regardless, so the next
        // clear's delta stays correct.
        if (ppChange == null || Math.abs(ppChange) < 0.05) continue;
        const m = marketsById.get(id);
        all.push({
          key: `${b.height}-${id}`,
          id,
          name: m?.name ?? `#${id}`,
          yes,
          volNanos,
          ppChange,
          ts: b.timestamp_ms,
        });
      }
    }
    // Newest first, capped.
    all.reverse();
    return all.slice(0, MAX_TICKER_ITEMS);
  }, [recent, marketsById]);

  const animate = events.length >= MARQUEE_MIN_ITEMS;
  // ~one cell-width per ~6s keeps the scroll slow and readable as the list grows.
  const durationSec = Math.max(36, events.length * 6.4);

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
      {/* Anchored badge — pulsing dot + latest committed height */}
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
            animation: "sybil-pulse 1.6s ease-in-out infinite",
          }}
        />
        <span>Recent trades</span>
        <span style={{ color: "var(--accent)", opacity: 0.6 }}>·</span>
        <span className="tabular">
          #{latest?.height != null ? formatInt(latest.height) : "—"}
        </span>
      </div>

      {events.length === 0 ? (
        <span
          className="text-mono"
          style={{
            display: "inline-flex",
            alignItems: "center",
            padding: "0 var(--space-4)",
            color: "var(--fg-2)",
            fontSize: "var(--fs-12)",
          }}
        >
          awaiting fills…
        </span>
      ) : (
        <div
          onMouseEnter={() => setPaused(true)}
          onMouseLeave={() => setPaused(false)}
          style={{
            flex: 1,
            minWidth: 0,
            overflow: "hidden",
            WebkitMaskImage:
              "linear-gradient(to right, transparent 0, black 24px, black calc(100% - 32px), transparent 100%)",
            maskImage:
              "linear-gradient(to right, transparent 0, black 24px, black calc(100% - 32px), transparent 100%)",
          }}
        >
          <div
            style={{
              display: "inline-flex",
              flexWrap: "nowrap",
              willChange: animate ? "transform" : undefined,
              // Longhand (not the `animation` shorthand) so toggling
              // animationPlayState on hover doesn't conflict with the shorthand.
              animationName: animate ? "sybil-marquee" : undefined,
              animationDuration: animate ? `${durationSec}s` : undefined,
              animationTimingFunction: animate ? "linear" : undefined,
              animationIterationCount: animate ? "infinite" : undefined,
              animationPlayState: paused ? "paused" : "running",
            }}
          >
            {events.map((e) => (
              <TickerCell key={e.key} event={e} now={now} />
            ))}
            {/* Second copy makes the -50% loop seamless. */}
            {animate &&
              events.map((e) => (
                <TickerCell key={`dup-${e.key}`} event={e} now={now} ariaHidden />
              ))}
          </div>
        </div>
      )}
    </div>
  );
}

function TickerCell({
  event,
  now,
  ariaHidden,
}: {
  event: ClearEvent;
  now: number;
  ariaHidden?: boolean;
}) {
  const { id, name, volNanos, ppChange, ts } = event;
  return (
    <Link
      href={`/m/${id}`}
      aria-hidden={ariaHidden}
      tabIndex={ariaHidden ? -1 : undefined}
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
      {/* Full market question, untruncated. For grouped (NegRisk) outcomes the
          name is "{event}: {outcome}"; for binaries it's the bare question.
          The marquee scrolls, so a wide cell is fine. */}
      <span style={{ color: "var(--fg-2)", whiteSpace: "nowrap" }}>{name}</span>
      <span className="tabular" style={{ color: "var(--fg-4)" }}>
        {formatCompactDollars(volNanos)} vol
      </span>
      <span
        className="tabular"
        style={{ color: ppColor(ppChange), fontWeight: 600 }}
      >
        {formatPp(ppChange)}
      </span>
      <span className="tabular" style={{ color: "var(--fg-4)" }}>
        {formatAge(Math.max(0, now - ts))} ago
      </span>
    </Link>
  );
}

/** ±N.N pp with explicit sign; ~flat collapses to "0.0pp". */
function formatPp(pp: number): string {
  if (Math.abs(pp) < 0.05) return "0.0pp";
  const sign = pp > 0 ? "+" : "−";
  return `${sign}${Math.abs(pp).toFixed(1)}pp`;
}

function ppColor(pp: number | null): string {
  // Flat/unknown reads in a legible neutral (not the faint fg-4) so a "0.0pp"
  // is actually visible; only real moves take the green/red accents.
  if (pp == null || Math.abs(pp) < 0.05) return "var(--fg-3)";
  return pp > 0 ? "var(--yes)" : "var(--no)";
}
