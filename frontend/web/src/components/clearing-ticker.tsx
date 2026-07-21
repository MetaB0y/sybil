"use client";

import Link from "next/link";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  formatAge,
  formatCompactDollars,
  formatPercentPrecise,
  parseNanos,
} from "@/lib/format/nanos";
import type { IndexMarket } from "@/lib/markets/use-markets";
import { useEventQuestions } from "@/lib/markets/use-event-raw";
import {
  selectRecentBlocks,
  selectRecentHistory,
  type RecentHistoryState,
  useStore,
} from "@/lib/store";
import type { Block } from "@/lib/ws/types";

type Props = {
  /** Lookup table for resolving market_id → name. */
  marketsById: Map<number, IndexMarket>;
};

/** Most recent clears to keep on the strip. Sized against the scroll speed
 *  below: a longer strip takes minutes to traverse end to end, so its tail is
 *  already stale by the time it scrolls into view. */
const MAX_TICKER_ITEMS = 16;
/** Marquee speed in CSS px per second. Deriving the duration from measured
 *  width rather than item count keeps the scroll rate identical whether two
 *  markets clear or sixteen, and whether labels are short native names or long
 *  mirror questions. */
const MARQUEE_PX_PER_SEC = 30;

/** One market clearing in one batch — a single "trade" entry on the ticker. */
export type ClearEvent = {
  key: string;
  height: number;
  id: number;
  name: string;
  /** Mirror identifiers used to resolve the full per-outcome question. */
  condId: string | null;
  eventId: string | null;
  /** Clearing YES price this batch (nanos). */
  yes: bigint;
  /** Matched volume this market contributed this batch (nanos, $). */
  volNanos: bigint;
  /** YES price change vs this market's previous traded clear, in pp. Null for
   *  the first traded clear in the loaded window; flat changes are real data. */
  ppChange: number | null;
  /** Block timestamp (epoch ms) → "seconds ago". */
  ts: number;
};

/**
 * ClearingTicker — a continuously scrolling strip of recent clears across the
 * last committed blocks. Each entry is one market that traded in one batch:
 * `name  price  $vol  ±pp  age` (title · YES price · volume · change · age).
 * First and flat traded clears remain visible because price movement is not
 * the membership predicate. The buffer accumulates across blocks rather than
 * being replaced each batch, so the marquee has stable content to scroll.
 *
 * Side (buy/sell) is intentionally absent: block-level fills carry no
 * market_id or side, so a global feed can only speak in per-batch clears.
 */
export function ClearingTicker({ marketsById }: Props) {
  const recent = useStore(selectRecentBlocks);
  const recentHistory = useStore(selectRecentHistory);
  const [paused, setPaused] = useState(false);

  // Live "now" so the age column ticks every second between blocks.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);

  const events = useMemo(
    () => deriveRecentTrades(recent, marketsById),
    [recent, marketsById],
  );

  // Whether the content overflows is a question about width, not item count: a
  // handful of long mirror questions overflows a phone while sixteen short
  // native names may not fill a desktop. Measure both sides and compare.
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const copyRef = useRef<HTMLDivElement | null>(null);
  const [copyWidth, setCopyWidth] = useState(0);
  const [viewportWidth, setViewportWidth] = useState(0);

  const animate = copyWidth > 0 && copyWidth > viewportWidth + 1;
  // Constant pixel speed. Tying the duration to item count (the old
  // `events.length * 6.4`) meant every new clear re-timed a running animation,
  // which remapped its progress and jumped the strip sideways.
  const durationSec = copyWidth / MARQUEE_PX_PER_SEC;

  // A scrolling strip shows a frozen snapshot: splicing cells in mid-loop
  // shifts every following cell sideways, which is the other half of the same
  // stutter. Fresh clears are taken up at a loop boundary instead, where the
  // track has wrapped back to its start. While nothing scrolls there is no
  // boundary to wait for, so the live list renders directly.
  const [frozen, setFrozen] = useState<ClearEvent[] | null>(null);
  const displayed = animate && frozen ? frozen : events;
  const pendingRef = useRef(events);
  useEffect(() => {
    pendingRef.current = events;
  }, [events]);

  // Resolve questions for what is on screen, not for the live window: a frozen
  // snapshot outlives the blocks it came from, and keying this to `events`
  // would drop its ids and flip cells back to catalog names mid-scroll.
  const eventIds = useMemo(() => {
    const ids = new Set<string>();
    for (const event of displayed) {
      if (event.condId && event.eventId) ids.add(event.eventId);
    }
    return [...ids];
  }, [displayed]);
  const questionByCondition = useEventQuestions(eventIds);
  const labelFor = (event: ClearEvent): string =>
    (event.condId ? questionByCondition.get(event.condId) : undefined) ??
    event.name;

  useEffect(() => {
    const viewport = viewportRef.current;
    const copy = copyRef.current;
    if (!viewport || !copy || typeof ResizeObserver === "undefined") return;
    // Labels arrive asynchronously (the mirror question replaces the catalog
    // name), so cell widths settle after the first paint — observe rather than
    // measure once.
    const measure = () => {
      setCopyWidth(copy.getBoundingClientRect().width);
      setViewportWidth(viewport.clientWidth);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(viewport);
    observer.observe(copy);
    return () => observer.disconnect();
  }, [displayed]);

  return (
    <div
      className="clearing-ticker"
      aria-busy={displayed.length === 0 && recentHistory === "loading"}
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
          className="clearing-ticker-live-dot sybil-motion-pulse"
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
      </div>

      {displayed.length === 0 ? (
        <span
          className="text-mono"
          role={recentHistory === "error" ? "alert" : "status"}
          style={{
            display: "inline-flex",
            alignItems: "center",
            padding: "0 var(--space-4)",
            color: "var(--fg-2)",
            fontSize: "var(--fs-12)",
          }}
        >
          {recentTradesEmptyCopy(recentHistory, recent)}
        </span>
      ) : (
        <div
          ref={viewportRef}
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
            className="clearing-ticker-track"
            // Start and every wrap are the two moments the track sits at its
            // origin, so they are the only points where swapping cells is
            // invisible. Ignore animations bubbling up from a cell.
            onAnimationStart={(event) => {
              if (event.target !== event.currentTarget) return;
              setFrozen(pendingRef.current);
            }}
            onAnimationIteration={(event) => {
              if (event.target !== event.currentTarget) return;
              setFrozen(pendingRef.current);
            }}
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
            <div ref={copyRef} style={{ display: "inline-flex", flexWrap: "nowrap" }}>
              {displayed.map((e) => (
                <TickerCell
                  key={e.key}
                  event={e}
                  label={labelFor(e)}
                  now={now}
                />
              ))}
            </div>
            {/* Second copy makes the -50% loop seamless: the track is exactly
                two identical copies wide, so half a track is one full copy. */}
            {animate && (
              <div style={{ display: "inline-flex", flexWrap: "nowrap" }}>
                {displayed.map((e) => (
                  <TickerCell
                    key={`dup-${e.key}`}
                    event={e}
                    label={labelFor(e)}
                    now={now}
                    ariaHidden
                  />
                ))}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function TickerCell({
  event,
  label,
  now,
  ariaHidden,
}: {
  event: ClearEvent;
  label: string;
  now: number;
  ariaHidden?: boolean;
}) {
  const { id, volNanos, ppChange, ts } = event;
  return (
    <Link
      className="clearing-ticker-link"
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
      {/* Full mirror question when raw metadata is available; native markets
          fall back to their catalog name. The marquee can carry a wide cell. */}
      <span style={{ color: "var(--fg-2)", whiteSpace: "nowrap" }}>
        {label}
      </span>
      <span className="tabular" style={{ color: "var(--fg-4)" }}>
        YES {formatPercentPrecise(event.yes)}
      </span>
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
        {formatAge(Math.max(60_000, now - ts))} ago
      </span>
    </Link>
  );
}

/** ±N.N pp with explicit sign; ~flat collapses to "0.0pp". */
function formatPp(pp: number | null): string {
  if (pp == null) return "—";
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

/**
 * Pure trade-strip derivation. Walk oldest-to-newest to compute each market's
 * delta against its previous traded clear, then order newest block first and
 * market id ascending within a block for deterministic rendering.
 */
export function deriveRecentTrades(
  recent: Block[],
  marketsById: Map<number, IndexMarket>,
  maxItems = MAX_TICKER_ITEMS,
): ClearEvent[] {
  const prevYes = new Map<number, bigint>();
  const all: ClearEvent[] = [];
  const chronological = [...recent].sort((a, b) => a.height - b.height);

  for (const block of chronological) {
    const prices = block.clearing_prices_nanos;
    if (!prices) continue;
    for (const [key, vector] of Object.entries(prices)) {
      const id = Number(key);
      if (!Number.isSafeInteger(id)) continue;
      const yesRaw = vector[0];
      const volumeRaw = block.by_market?.[key]?.volume_nanos;
      if (yesRaw == null || volumeRaw == null) continue;
      const volumeNanos = parseNanos(volumeRaw);
      if (volumeNanos <= 0n) continue;

      const yes = parseNanos(yesRaw);
      const prior = prevYes.get(id);
      prevYes.set(id, yes);
      const market = marketsById.get(id);
      all.push({
        key: `${block.height}-${id}`,
        height: block.height,
        id,
        name: market?.name ?? `#${id}`,
        condId: market?.polymarket_condition_id ?? null,
        eventId: market?.event_id ?? null,
        yes,
        volNanos: volumeNanos,
        ppChange: prior == null ? null : Number(yes - prior) / 1e7,
        ts: block.timestamp_ms,
      });
    }
  }

  return all
    .sort((a, b) => b.height - a.height || a.id - b.id)
    .slice(0, Math.max(0, maxItems));
}

/** Truthful empty-state language for the independently loaded history window. */
export function recentTradesEmptyCopy(
  state: RecentHistoryState,
  recent: Block[],
): string {
  if (state === "idle" || state === "loading") return "loading recent trades…";
  if (state === "error") return "recent trades unavailable";
  if (recent.some((block) => block.fill_count > 0)) {
    return "recent trade details unavailable";
  }
  return "no trades in recent blocks";
}
