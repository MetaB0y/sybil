"use client";

import { useMemo, useState } from "react";
import { BinaryCard } from "@/components/binary-card";
import { MultiCard } from "@/components/multi-card";
import { MarketsFilterBar, type SortKey } from "@/components/markets-filter-bar";
import { useMarketsList, type Market } from "@/lib/markets/use-markets";
import { selectPricesByMarketId, useStore } from "@/lib/store";

const MULTI_THRESHOLD = 5;

/** Display unit. Either one event group rendered as MultiCard, or one
 *  individual market rendered as BinaryCard. */
type CardItem =
  | { kind: "multi"; name: string; markets: Market[]; volumeNanos: bigint; sortKey: string }
  | { kind: "binary"; market: Market; volumeNanos: bigint; sortKey: string };

export default function MarketsPage() {
  const { bundle, isPending, error } = useMarketsList();
  const prices = useStore(selectPricesByMarketId);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<SortKey>("volume");

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
        });
      } else {
        for (const m of g.markets) {
          all.push({
            kind: "binary",
            market: m,
            volumeNanos: m.volume_nanos ? BigInt(m.volume_nanos) : 0n,
            sortKey: m.name.toLowerCase(),
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
    if (sort === "name") {
      out.sort((a, b) => a.sortKey.localeCompare(b.sortKey));
    } else if (sort === "count") {
      out.sort((a, b) => sizeOf(b) - sizeOf(a));
    } else {
      // volume desc; tie-break by size desc
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
    <main
      style={{
        maxWidth: "var(--max-content)",
        margin: "0 auto",
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
          {bundle != null
            ? `${bundle.total} events · ${bundle.groups.length} groups · uniform clearing every 2s`
            : "loading…"}
        </p>
      </header>

      <MarketsFilterBar
        query={query}
        onQueryChange={setQuery}
        sort={sort}
        onSortChange={setSort}
        resultsCount={filtered?.length ?? 0}
      />

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
  );
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
