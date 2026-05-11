"use client";

import Link from "next/link";
import { formatProbability } from "@/lib/format/nanos";
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
        gap: "var(--space-6)",
      }}
    >
      <header style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
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
        <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-7)" }}>
          {bundle.groups.map((g) => (
            <GroupSection key={g.name} name={g.name} markets={g.markets} prices={prices} />
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
        gap: "var(--space-3)",
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", gap: "var(--space-3)" }}>
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
          background: "var(--surface-1)",
          border: "1px solid var(--border-1)",
          borderRadius: "var(--radius-lg)",
          boxShadow: "var(--shadow-inset-top)",
          overflow: "hidden",
        }}
      >
        {markets.map((m, i) => (
          <MarketRow
            key={m.market_id}
            market={m}
            price={prices[m.market_id]}
            withTopBorder={i > 0}
          />
        ))}
      </div>
    </section>
  );
}

function MarketRow({
  market,
  price,
  withTopBorder,
}: {
  market: Market;
  price: MarketPrice | undefined;
  withTopBorder: boolean;
}) {
  return (
    <Link
      href={`/m/${market.market_id}`}
      style={{
        display: "grid",
        gridTemplateColumns: "1fr 96px 96px",
        gap: "var(--space-4)",
        alignItems: "center",
        padding: "var(--space-3) var(--space-5)",
        borderTop: withTopBorder ? "1px solid var(--border-1)" : "0",
        color: "var(--fg-1)",
        textDecoration: "none",
        transition: "background var(--dur-fast) var(--ease-standard)",
      }}
      onMouseEnter={(e) => (e.currentTarget.style.background = "var(--surface-2)")}
      onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
    >
      <span
        style={{
          fontFamily: "var(--font-sans)",
          fontSize: "var(--fs-14)",
          fontWeight: 500,
          color: "var(--fg-1)",
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {market.name}
      </span>
      <PriceCell value={price?.yes} tone="yes" />
      <PriceCell value={price?.no} tone="no" />
    </Link>
  );
}

function PriceCell({ value, tone }: { value: bigint | undefined; tone: "yes" | "no" }) {
  return (
    <span
      className="text-mono tabular"
      style={{
        fontSize: "var(--fs-14)",
        textAlign: "right",
        color: value == null ? "var(--fg-4)" : tone === "yes" ? "var(--yes)" : "var(--no)",
      }}
    >
      {value == null ? "—" : formatProbability(value)}
    </span>
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
