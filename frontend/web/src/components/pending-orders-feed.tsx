"use client";

import { useMemo } from "react";
import { formatProbability } from "@/lib/format/nanos";
import {
  usePendingOrdersForMarket,
  type PendingOrder,
} from "@/lib/markets/use-pending-orders";
import { parseNanos } from "@/lib/format/nanos";

type Side = "BuyYes" | "SellYes" | "BuyNo" | "SellNo";

const SIDE_TONE: Record<Side, "yes" | "no"> = {
  BuyYes: "yes",
  SellYes: "yes",
  BuyNo: "no",
  SellNo: "no",
};

const SIDE_LABEL: Record<Side, string> = {
  BuyYes: "buy YES",
  SellYes: "sell YES",
  BuyNo: "buy NO",
  SellNo: "sell NO",
};

type Props = {
  marketId: number;
};

/**
 * PendingOrdersFeed — orders queued for the *next* batch in this market.
 * In FBA, each pending order lives one block (created_at_block →
 * expires_at_block = created+1); we refetch per block so this list shows
 * the orders that will all clear together when the batch closes.
 */
export function PendingOrdersFeed({ marketId }: Props) {
  const { data: orders = [], isPending, error } =
    usePendingOrdersForMarket(marketId);

  // Sort: bids descending by limit_price (highest bid first), asks ascending.
  const sorted = useMemo(() => {
    return [...orders].sort((a, b) => {
      const pa = parseNanos(a.limit_price_nanos);
      const pb = parseNanos(b.limit_price_nanos);
      const aBid = a.side === "BuyYes" || a.side === "BuyNo";
      const bBid = b.side === "BuyYes" || b.side === "BuyNo";
      if (aBid && bBid) return pa < pb ? 1 : pa > pb ? -1 : 0;
      if (!aBid && !bBid) return pa < pb ? -1 : pa > pb ? 1 : 0;
      return aBid ? -1 : 1; // bids before asks for simple display
    });
  }, [orders]);

  return (
    <section
      style={{
        padding: "var(--space-4) var(--space-5)",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        boxShadow: "var(--shadow-inset-top)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-3)",
      }}
    >
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "baseline",
        }}
      >
        <div className="eyebrow">{"// pending · next batch"}</div>
        <span className="text-mono tabular" style={{ color: "var(--fg-3)", fontSize: "var(--fs-12)" }}>
          {sorted.length} {sorted.length === 1 ? "order" : "orders"}
        </span>
      </div>

      {error ? (
        <Empty error>error: {String(error)}</Empty>
      ) : isPending && orders.length === 0 ? (
        <Empty>loading…</Empty>
      ) : sorted.length === 0 ? (
        <Empty>no orders waiting for the next batch.</Empty>
      ) : (
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "minmax(80px, 1fr) minmax(80px, 1fr) minmax(60px, auto) minmax(64px, auto)",
            columnGap: "var(--space-4)",
            rowGap: "var(--space-2)",
            fontFamily: "var(--font-mono)",
            fontSize: "var(--fs-13)",
          }}
        >
          {/* Header row */}
          <Header>side</Header>
          <Header style={{ textAlign: "right" }}>limit</Header>
          <Header style={{ textAlign: "right" }}>qty</Header>
          <Header style={{ textAlign: "right" }}>account</Header>

          {sorted.map((o) => (
            <OrderRow key={o.order_id} order={o} />
          ))}
        </div>
      )}
    </section>
  );
}

function Header({
  children,
  style,
}: {
  children: React.ReactNode;
  style?: React.CSSProperties;
}) {
  return (
    <span
      style={{
        color: "var(--fg-3)",
        fontSize: "10px",
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
        paddingBottom: "var(--space-2)",
        borderBottom: "1px solid var(--border-1)",
        ...style,
      }}
    >
      {children}
    </span>
  );
}

function OrderRow({ order }: { order: PendingOrder }) {
  const side = order.side as Side;
  const tone = SIDE_TONE[side] ?? "yes";
  const color = tone === "yes" ? "var(--yes)" : "var(--no)";
  return (
    <>
      <span
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: "var(--space-2)",
          color: "var(--fg-1)",
        }}
      >
        <span
          aria-hidden
          style={{
            width: 4,
            height: 14,
            background: color,
            borderRadius: 1,
          }}
        />
        {SIDE_LABEL[side] ?? order.side}
      </span>
      <span className="tabular" style={{ textAlign: "right", color: "var(--fg-1)" }}>
        {formatProbability(parseNanos(order.limit_price_nanos))}
      </span>
      <span className="tabular" style={{ textAlign: "right", color: "var(--fg-2)" }}>
        {order.remaining_quantity}
      </span>
      <span className="tabular" style={{ textAlign: "right", color: "var(--fg-3)" }}>
        #{order.account_id}
      </span>
    </>
  );
}

function Empty({
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
        color: error ? "var(--no)" : "var(--fg-4)",
        fontSize: "var(--fs-13)",
        padding: "var(--space-3) 0",
      }}
    >
      {children}
    </div>
  );
}
