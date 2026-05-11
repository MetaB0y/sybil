"use client";

import { BinaryCard } from "@/components/binary-card";
import { useMarketsList, type Market } from "@/lib/markets/use-markets";
import { selectPricesByMarketId, useStore, type MarketPrice } from "@/lib/store";

export default function MarketsPage() {
  const { bundle, isPending, error } = useMarketsList();
  const prices = useStore(selectPricesByMarketId);

  return (
    <main
      style={{
        maxWidth: "var(--max-content)",
        margin: "0 auto",
        padding: "var(--space-6) var(--space-5) var(--space-9)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-7)",
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

      {isPending && <Placeholder>loading markets…</Placeholder>}
      {error && <Placeholder error>error: {String(error)}</Placeholder>}

      {bundle && (
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: "var(--space-7)",
          }}
        >
          {bundle.groups.map((g) => (
            <GroupSection
              key={g.name}
              name={g.name}
              markets={g.markets}
              prices={prices}
            />
          ))}
          {bundle.ungrouped.length > 0 && (
            <GroupSection name="Other" markets={bundle.ungrouped} prices={prices} />
          )}
        </div>
      )}
    </main>
  );
}

function GroupSection({
  name,
  markets,
  prices,
}: {
  name: string;
  markets: Market[];
  prices: Record<number, MarketPrice>;
}) {
  return (
    <section
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-4)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: "var(--space-3)",
        }}
      >
        <h2
          style={{
            fontFamily: "var(--font-sans)",
            fontWeight: 600,
            fontSize: "var(--fs-20)",
            lineHeight: "var(--lh-20)",
            margin: 0,
            color: "var(--fg-1)",
          }}
        >
          {name}
        </h2>
        <span className="text-meta">{markets.length}</span>
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
          gap: "var(--space-4)",
        }}
      >
        {markets.map((m) => (
          <BinaryCard
            key={m.market_id}
            market={m}
            price={prices[m.market_id]}
          />
        ))}
      </div>
    </section>
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
