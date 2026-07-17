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
import { useMarketsIndex, type IndexMarket } from "@/lib/markets/use-markets";
import { useEventTradersMap } from "@/lib/markets/use-event-traders";
import { selectPricesByMarketId, useStore } from "@/lib/store";
import {
  selectIndexCards,
  summarizeIndexCards,
} from "@/lib/markets/select-index-cards";
import { buildIndexCards } from "@/lib/markets/build-index-cards";

/** Cards (events) shown per page on the markets index. */
const PAGE_SIZE = 15;

export default function MarketsPageClient({
  initialMarkets,
}: {
  initialMarkets: IndexMarket[] | undefined;
}) {
  return (
    <Suspense fallback={null}>
      <MarketsPageInner initialMarkets={initialMarkets} />
    </Suspense>
  );
}

function MarketsPageInner({
  initialMarkets,
}: {
  initialMarkets: IndexMarket[] | undefined;
}) {
  const { bundle, isPending, error, refetch } = useMarketsIndex(initialMarkets);
  const prices = useStore(selectPricesByMarketId);

  // The clearing ticker is an active-board readout — exclude closed markets,
  // which the bundle now retains for detail/multi-card use.
  const openById = useMemo(() => {
    if (!bundle) return null;
    const m = new Map<number, IndexMarket>();
    for (const [id, mk] of bundle.byId) {
      if (mk.closed !== true) m.set(id, mk);
    }
    return m;
  }, [bundle]);

  const { query, sort, setSort, category, showClosed, setHideClosed } =
    useFilterParams();

  const items = useMemo(
    () => (bundle ? buildIndexCards(bundle) : null),
    [bundle],
  );

  // Event ids for MultiCard items — the "traders" sort ranks events by their
  // union trader count, fetched per event. Gated to that sort so the fan-out
  // of requests only fires when the user actually picks it.
  const multiEventIds = useMemo(
    () =>
      items
        ? items.flatMap((it) => (it.kind === "multi" ? [it.eventId] : []))
        : [],
    [items],
  );
  const eventTradersMap = useEventTradersMap(multiEventIds, sort === "traders");

  const filtered = useMemo(() => {
    if (!items) return null;
    return selectIndexCards(items, {
      query,
      sort,
      category,
      showClosed,
      eventTraders: eventTradersMap,
    });
  }, [items, query, sort, category, showClosed, eventTradersMap]);

  const [page, setPage] = useState(1);

  // Reset to the first page whenever the active filter set changes. Done
  // during render (not in an effect) per the React "adjust state on prop
  // change" pattern — avoids an extra commit.
  const filterKey = `${query} ${sort} ${category ?? ""} ${showClosed}`;
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

  // One card is one market in product language. A multi-outcome market contains
  // several independently traded outcome rows in the API.
  const shown = summarizeIndexCards(filtered ?? []);

  return (
    <>
      {openById && <ClearingTicker marketsById={openById} />}
      <main
        className="sybil-page-pad"
        style={{
          width: "100%",
          paddingTop: "var(--space-6)",
          paddingBottom: "var(--space-9)",
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
            className="markets-page-title"
            style={{
              fontFamily: "var(--font-display)",
              fontWeight: 600,
              fontSize: "var(--fs-56)",
              lineHeight: "var(--lh-56)",
              letterSpacing: "var(--track-tight)",
              margin: 0,
              color: "var(--fg-1)",
            }}
          >
            All markets
          </h1>
          <p
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: "var(--fs-13)",
              lineHeight: "var(--lh-18)",
              color: "var(--fg-3)",
              fontVariantNumeric: "tabular-nums",
              margin: 0,
            }}
          >
            {bundle == null
              ? "loading…"
              : `${shown.markets} markets · ${shown.outcomes} outcomes`}
          </p>
        </header>

        <MarketsFilterBar
          sort={sort}
          onSortChange={setSort}
          hideClosed={!showClosed}
          onHideClosedChange={setHideClosed}
        />

        {isPending && <Placeholder>loading markets…</Placeholder>}
        {error && !bundle && (
          <MarketsLoadError onRetry={() => void refetch()} />
        )}

        {filtered && filtered.length === 0 && (
          <Placeholder>
            {query !== "" || category != null || sort !== "volume" || showClosed
              ? "no events match these filters."
              : "no open markets right now — check back soon."}
          </Placeholder>
        )}

        {paged && paged.length > 0 && (
          <>
            <div className="markets-grid" data-testid="markets-grid">
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
                ),
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
  const showClosed = searchParams.get("closed") === "show";

  const update = useCallback(
    (next: { q?: string; sort?: SortKey; showClosed?: boolean }) => {
      const params = new URLSearchParams(searchParams.toString());
      if (next.q !== undefined) {
        if (next.q) params.set("q", next.q);
        else params.delete("q");
      }
      if (next.sort !== undefined) {
        if (next.sort !== "volume") params.set("sort", next.sort);
        else params.delete("sort");
      }
      if (next.showClosed !== undefined) {
        // Default is hide-closed; only write the param when showing them.
        if (next.showClosed) params.set("closed", "show");
        else params.delete("closed");
      }
      const qs = params.toString();
      router.replace(qs ? `${pathname}?${qs}` : pathname, { scroll: false });
    },
    [pathname, router, searchParams],
  );

  return {
    query,
    sort,
    category,
    showClosed,
    setSort: (s: SortKey) => update({ sort: s }),
    setHideClosed: (hide: boolean) => update({ showClosed: !hide }),
  };
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
        minHeight: 40,
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

function MarketsLoadError({ onRetry }: { onRetry: () => void }) {
  return (
    <div
      role="alert"
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: 12,
        padding: "var(--space-7) var(--space-5)",
        border: "1px solid color-mix(in srgb, var(--no) 35%, var(--border-1))",
        borderRadius: "var(--radius-lg)",
        background: "color-mix(in srgb, var(--no) 5%, var(--surface-1))",
        textAlign: "center",
      }}
    >
      <strong
        style={{
          color: "var(--fg-1)",
          fontFamily: "var(--font-sans)",
          fontSize: 15,
        }}
      >
        Markets are temporarily unavailable
      </strong>
      <span className="text-annotation">
        The live feed did not respond. Your account and orders are unaffected.
      </span>
      <button
        type="button"
        onClick={onRetry}
        style={{
          minHeight: 40,
          padding: "0 16px",
          border: "1px solid var(--border-2)",
          borderRadius: "var(--radius-md)",
          background: "var(--surface-2)",
          color: "var(--fg-1)",
          fontFamily: "var(--font-sans)",
          fontSize: 13,
          fontWeight: 600,
          cursor: "pointer",
        }}
      >
        Try again
      </button>
    </div>
  );
}
