"use client";

import { Suspense, useCallback, useMemo, useState } from "react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { BinaryCard } from "@/components/binary-card";
import { ClearingTicker } from "@/components/clearing-ticker";
import { MultiCard } from "@/components/multi-card";
import {
  MarketsFilterBar,
  parseSortKey,
  type SortKey,
} from "@/components/markets-filter-bar";
import { pickDisplayCategory } from "@/lib/categorize";
import { useMarketsList, type Market } from "@/lib/markets/use-markets";
import { useEventTradersMap } from "@/lib/markets/use-event-traders";
import { selectPricesByMarketId, useStore } from "@/lib/store";
import { BLOCK_INTERVAL_MS } from "@/lib/constants";

type CardItem =
  | {
      kind: "multi";
      name: string;
      eventId: string;
      markets: Market[];
      volumeNanos: bigint;
      sortKey: string;
      createdMs: number;
      primaryCategory: string | null;
    }
  | {
      kind: "binary";
      market: Market;
      volumeNanos: bigint;
      sortKey: string;
      createdMs: number;
      primaryCategory: string | null;
    };

/** Cards (events) shown per page on the markets index. */
const PAGE_SIZE = 15;

export default function MarketsPage() {
  return (
    <Suspense fallback={null}>
      <MarketsPageInner />
    </Suspense>
  );
}

function MarketsPageInner() {
  const { bundle, isPending, error } = useMarketsList();
  const prices = useStore(selectPricesByMarketId);
  const { query, sort, setSort, category } = useFilterParams();

  const items = useMemo(() => {
    if (!bundle) return null;
    const all: CardItem[] = [];
    for (const g of bundle.groups) {
      if (g.markets.length >= 2) {
        // Group-level category: use any market's categories (they share an
        // event so the buckets are the same; pick the first market's).
        const first = g.markets[0]!;
        const primary = pickDisplayCategory(first.categories, first.category).primary;
        all.push({
          kind: "multi",
          name: g.name,
          eventId: g.eventId,
          markets: g.markets,
          volumeNanos: sumVolume(g.markets),
          sortKey: g.name.toLowerCase(),
          createdMs: eventNewnessMs(g.markets),
          primaryCategory: primary,
        });
      } else {
        for (const m of g.markets) {
          all.push({
            kind: "binary",
            market: m,
            volumeNanos: m.volume_nanos ? BigInt(m.volume_nanos) : 0n,
            sortKey: m.name.toLowerCase(),
            createdMs: marketNewnessMs(m),
            primaryCategory: pickDisplayCategory(m.categories, m.category).primary,
          });
        }
      }
    }
    for (const m of bundle.ungrouped) {
      all.push({
        kind: "binary",
        market: m,
        volumeNanos: m.volume_nanos ? BigInt(m.volume_nanos) : 0n,
        sortKey: m.name.toLowerCase(),
        createdMs: m.created_at_ms ?? 0,
        primaryCategory: pickDisplayCategory(m.categories, m.category).primary,
      });
    }
    return all;
  }, [bundle]);

  // Event ids for MultiCard items — the "traders" sort ranks events by their
  // union trader count, fetched per event. Gated to that sort so the fan-out
  // of requests only fires when the user actually picks it.
  const multiEventIds = useMemo(
    () =>
      items
        ? items.flatMap((it) => (it.kind === "multi" ? [it.eventId] : []))
        : [],
    [items]
  );
  const eventTradersMap = useEventTradersMap(
    multiEventIds,
    sort === "traders"
  );

  const filtered = useMemo(() => {
    if (!items) return null;
    const q = query.trim().toLowerCase();
    let out = items;
    if (q) {
      out = out.filter((it) => it.sortKey.includes(q));
    }
    if (category) {
      out = out.filter((it) => it.primaryCategory === category);
    }
    out = [...out];
    if (sort === "new") {
      out.sort((a, b) => b.createdMs - a.createdMs);
    } else if (sort === "traders") {
      // Event total traders desc; tie-break by volume desc. While the
      // per-event queries resolve, missing events read as 0 and the list
      // settles as data lands.
      out.sort((a, b) => {
        const ta = traderCountOf(a, eventTradersMap);
        const tb = traderCountOf(b, eventTradersMap);
        if (ta !== tb) return tb - ta;
        if (a.volumeNanos === b.volumeNanos) return 0;
        return a.volumeNanos < b.volumeNanos ? 1 : -1;
      });
    } else {
      // volume desc; tie-break by size desc.
      out.sort((a, b) => {
        if (a.volumeNanos !== b.volumeNanos) {
          return a.volumeNanos < b.volumeNanos ? 1 : -1;
        }
        return sizeOf(b) - sizeOf(a);
      });
    }
    return out;
  }, [items, query, sort, category, eventTradersMap]);

  const [page, setPage] = useState(1);

  // Reset to the first page whenever the active filter set changes. Done
  // during render (not in an effect) per the React "adjust state on prop
  // change" pattern — avoids an extra commit.
  const filterKey = `${query} ${sort} ${category ?? ""}`;
  const [prevFilterKey, setPrevFilterKey] = useState(filterKey);
  if (filterKey !== prevFilterKey) {
    setPrevFilterKey(filterKey);
    setPage(1);
  }

  const totalPages = filtered
    ? Math.max(1, Math.ceil(filtered.length / PAGE_SIZE))
    : 1;
  // Clamp: a filter change can shrink the result set below the current page.
  const currentPage = Math.min(page, totalPages);
  const paged = filtered
    ? filtered.slice((currentPage - 1) * PAGE_SIZE, currentPage * PAGE_SIZE)
    : null;

  const goToPage = useCallback((next: number) => {
    setPage(next);
    window.scrollTo({ top: 0, behavior: "smooth" });
  }, []);

  // Header counts — same "{markets} markets · {events} events" shape whether or
  // not a filter is active. Derived from `filtered` (≡ all cards when nothing
  // is filtered), so picking a category just narrows both numbers instead of
  // switching to a different "N of M cards" wording. Summing markets per card
  // equals bundle.total when unfiltered (every market lives in exactly one card).
  const shownEvents = filtered?.length ?? 0;
  const shownMarkets =
    filtered?.reduce(
      (n, it) => n + (it.kind === "multi" ? it.markets.length : 1),
      0,
    ) ?? 0;

  return (
    <>
      {bundle && <ClearingTicker marketsById={bundle.byId} />}
      <main
        style={{
          width: "100%",
          padding: "var(--space-6) var(--space-5) var(--space-9)",
          display: "flex",
          flexDirection: "column",
          gap: "var(--space-5)",
        }}
      >
        <header
          style={{
            display: "flex",
            flexDirection: "column",
            gap: "var(--space-2)",
          }}
        >
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
            All markets
          </h1>
          <p className="text-annotation">
            {bundle == null
              ? "loading…"
              : `${shownMarkets} markets · ${shownEvents} events · uniform clearing every ${BLOCK_INTERVAL_MS / 1000}s`}
          </p>
        </header>

        <MarketsFilterBar sort={sort} onSortChange={setSort} />

        {isPending && <Placeholder>loading markets…</Placeholder>}
        {error && <Placeholder error>error: {String(error)}</Placeholder>}

        {filtered && filtered.length === 0 && (
          <Placeholder>no events match these filters.</Placeholder>
        )}

        {paged && paged.length > 0 && (
          <>
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
                gap: "var(--space-4)",
              }}
            >
              {paged.map((it) =>
                it.kind === "multi" ? (
                  <MultiCard
                    key={`g-${it.name}`}
                    groupName={it.name}
                    markets={it.markets}
                    prices={prices}
                  />
                ) : (
                  <BinaryCard
                    key={`m-${it.market.market_id}`}
                    market={it.market}
                    price={prices[it.market.market_id]}
                  />
                )
              )}
            </div>
            {totalPages > 1 && (
              <Pagination
                page={currentPage}
                totalPages={totalPages}
                onChange={goToPage}
              />
            )}
          </>
        )}
      </main>
    </>
  );
}

/**
 * URL-backed `?q=` + `?sort=` state. Reads via useSearchParams, writes via
 * router.replace so back/forward doesn't get cluttered by every keystroke.
 * Empty/default values are dropped from the URL to keep it tidy.
 */
function useFilterParams() {
  const searchParams = useSearchParams();
  const router = useRouter();
  const pathname = usePathname();

  const query = searchParams.get("q") ?? "";
  const sort = parseSortKey(searchParams.get("sort"));
  const category = searchParams.get("category");

  const update = useCallback(
    (next: { q?: string; sort?: SortKey }) => {
      const params = new URLSearchParams(searchParams.toString());
      if (next.q !== undefined) {
        if (next.q) params.set("q", next.q);
        else params.delete("q");
      }
      if (next.sort !== undefined) {
        if (next.sort !== "volume") params.set("sort", next.sort);
        else params.delete("sort");
      }
      const qs = params.toString();
      router.replace(qs ? `${pathname}?${qs}` : pathname, { scroll: false });
    },
    [pathname, router, searchParams]
  );

  return {
    query,
    sort,
    category,
    setSort: (s: SortKey) => update({ sort: s }),
  };
}

function sizeOf(item: CardItem): number {
  return item.kind === "multi" ? item.markets.length : 1;
}

function sumVolume(markets: Market[]): bigint {
  let total = 0n;
  for (const m of markets) {
    if (m.volume_nanos != null) total += BigInt(m.volume_nanos);
  }
  return total;
}

/** Trader count for sorting: per-market for binary, event union for multi. */
function traderCountOf(
  item: CardItem,
  eventTraders: Map<string, number>
): number {
  if (item.kind === "binary") return item.market.trader_count ?? 0;
  return eventTraders.get(item.eventId) ?? 0;
}

/**
 * "New" sort key: the most recent of the Polymarket event-start and
 * market-start dates, so a brand-new event AND a newly-added outcome inside an
 * existing event both surface. `created_at_ms` (the mirror's admit time, which
 * clusters at sync) is only a last-resort fallback.
 */
function marketNewnessMs(m: Market): number {
  return Math.max(
    m.event_start_date_ms ?? 0,
    m.market_start_date_ms ?? 0,
    m.created_at_ms ?? 0
  );
}

function eventNewnessMs(markets: Market[]): number {
  let max = 0;
  for (const m of markets) max = Math.max(max, marketNewnessMs(m));
  return max;
}

function Pagination({
  page,
  totalPages,
  onChange,
}: {
  page: number;
  totalPages: number;
  onChange: (next: number) => void;
}) {
  return (
    <div
      className="text-mono"
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        gap: "var(--space-4)",
        padding: "var(--space-4) 0",
        fontSize: "11px",
        textTransform: "uppercase",
        letterSpacing: "var(--track-wide)",
      }}
    >
      <PageButton disabled={page <= 1} onClick={() => onChange(page - 1)}>
        ← prev
      </PageButton>
      <span style={{ color: "var(--fg-3)" }}>
        page {page} / {totalPages}
      </span>
      <PageButton
        disabled={page >= totalPages}
        onClick={() => onChange(page + 1)}
      >
        next →
      </PageButton>
    </div>
  );
}

function PageButton({
  children,
  disabled,
  onClick,
}: {
  children: React.ReactNode;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className="text-mono"
      style={{
        padding: "var(--space-2) var(--space-3)",
        fontSize: "11px",
        textTransform: "uppercase",
        letterSpacing: "var(--track-wide)",
        color: disabled ? "var(--fg-4)" : "var(--fg-1)",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-md)",
        cursor: disabled ? "default" : "pointer",
        opacity: disabled ? 0.5 : 1,
      }}
    >
      {children}
    </button>
  );
}

function Placeholder({
  children,
  error,
}: {
  children: React.ReactNode;
  error?: boolean;
}) {
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
