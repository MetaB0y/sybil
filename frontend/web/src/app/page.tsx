"use client";

import { Suspense, useCallback, useMemo } from "react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { BinaryCard } from "@/components/binary-card";
import { ClearingTicker } from "@/components/clearing-ticker";
import { MultiCard } from "@/components/multi-card";
import {
  MarketsFilterBar,
  parseSortKey,
  type SortKey,
} from "@/components/markets-filter-bar";
import { useMarketsList, type Market } from "@/lib/markets/use-markets";
import { selectPricesByMarketId, useStore } from "@/lib/store";

const MULTI_THRESHOLD = 5;

type CardItem =
  | {
      kind: "multi";
      name: string;
      markets: Market[];
      volumeNanos: bigint;
      sortKey: string;
      expiryMs: number;
      createdMs: number;
    }
  | {
      kind: "binary";
      market: Market;
      volumeNanos: bigint;
      sortKey: string;
      expiryMs: number;
      createdMs: number;
    };

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
  const { query, sort, setSort } = useFilterParams();

  const items = useMemo(() => {
    if (!bundle) return null;
    const all: CardItem[] = [];
    for (const g of bundle.groups) {
      if (g.markets.length > MULTI_THRESHOLD) {
        all.push({
          kind: "multi",
          name: g.name,
          markets: g.markets,
          volumeNanos: sumVolume(g.markets),
          sortKey: g.name.toLowerCase(),
          expiryMs: minExpiry(g.markets),
          createdMs: maxCreated(g.markets),
        });
      } else {
        for (const m of g.markets) {
          all.push({
            kind: "binary",
            market: m,
            volumeNanos: m.volume_nanos ? BigInt(m.volume_nanos) : 0n,
            sortKey: m.name.toLowerCase(),
            expiryMs: m.expiry_timestamp_ms ?? Number.POSITIVE_INFINITY,
            createdMs: m.created_at_ms ?? 0,
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
        expiryMs: m.expiry_timestamp_ms ?? Number.POSITIVE_INFINITY,
        createdMs: m.created_at_ms ?? 0,
      });
    }
    return all;
  }, [bundle]);

  const filtered = useMemo(() => {
    if (!items) return null;
    const q = query.trim().toLowerCase();
    let out = items;
    if (q) {
      out = out.filter((it) => it.sortKey.includes(q));
    }
    out = [...out];
    if (sort === "closing") {
      out.sort((a, b) => a.expiryMs - b.expiryMs);
    } else if (sort === "new") {
      out.sort((a, b) => b.createdMs - a.createdMs);
    } else {
      // volume desc; tie-break by size desc. "topmovers" falls here for now
      // (it's a disabled chip — never actually selected from the UI).
      out.sort((a, b) => {
        if (a.volumeNanos !== b.volumeNanos) {
          return a.volumeNanos < b.volumeNanos ? 1 : -1;
        }
        return sizeOf(b) - sizeOf(a);
      });
    }
    return out;
  }, [items, query, sort]);

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
              : filtered && filtered.length !== items?.length
                ? `${filtered.length} of ${bundle.total} events · uniform clearing every 2s`
                : `${bundle.total} events · ${bundle.groups.length} groups · uniform clearing every 2s`}
          </p>
        </header>

        <MarketsFilterBar sort={sort} onSortChange={setSort} />

        {isPending && <Placeholder>loading markets…</Placeholder>}
        {error && <Placeholder error>error: {String(error)}</Placeholder>}

        {filtered && filtered.length === 0 && (
          <Placeholder>no events match these filters.</Placeholder>
        )}

        {filtered && filtered.length > 0 && (
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
              gap: "var(--space-4)",
            }}
          >
            {filtered.map((it) =>
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

function minExpiry(markets: Market[]): number {
  let min = Number.POSITIVE_INFINITY;
  for (const m of markets) {
    if (m.expiry_timestamp_ms != null && m.expiry_timestamp_ms < min) {
      min = m.expiry_timestamp_ms;
    }
  }
  return min;
}

function maxCreated(markets: Market[]): number {
  let max = 0;
  for (const m of markets) {
    if (m.created_at_ms != null && m.created_at_ms > max) {
      max = m.created_at_ms;
    }
  }
  return max;
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
