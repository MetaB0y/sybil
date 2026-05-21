"use client";

/**
 * Open orders tab. Grid rows matching `PortfolioVariants.jsx:104-194`:
 *   dot · market · action · side · remaining · limit · value · TIF · queued · cancel
 *
 * Partial-fill progress is shown as `(original − remaining) / original`
 * using B8's `original_quantity` wire field. Orders persisted before B8
 * landed report `original_quantity: 0`; we hide the progress for them.
 */

import Link from "next/link";
import { useState } from "react";
import { cancelSignedOrder } from "@/lib/account/orders";
import type { AccountOrder } from "@/lib/account/use-account-orders";
import { formatCents, formatDollars, parseNanos } from "@/lib/format/nanos";
import { selectLatestHeight, useStore } from "@/lib/store";
import type { components } from "@/lib/api/schema";
import { CategoryDot } from "./category-dot";
import { SidePill } from "./side-pill";
import { TifCell } from "./tif-cell";

type Market = components["schemas"]["MarketResponse"];

interface Props {
  accountId: number;
  publicKeyHex: string;
  orders: AccountOrder[];
  marketsById: Map<number, Market>;
}

export function OpenOrdersList({
  accountId,
  publicKeyHex,
  orders,
  marketsById,
}: Props) {
  if (orders.length === 0) {
    return <Empty>No open orders.</Empty>;
  }
  return (
    <div
      style={{
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: 6,
        overflow: "hidden",
      }}
    >
      <HeaderRow />
      {orders.map((o) => (
        <OrderRow
          key={o.order_id}
          order={o}
          market={marketsById.get(o.market_id)}
          accountId={accountId}
          publicKeyHex={publicKeyHex}
        />
      ))}
    </div>
  );
}

function HeaderRow() {
  return (
    <div style={rowGrid("var(--fg-4)")}>
      <span />
      <span>Market</span>
      <span>Action</span>
      <span>Side</span>
      <RightCell>Remaining</RightCell>
      <RightCell>Limit</RightCell>
      <RightCell>Value</RightCell>
      <RightCell>TIF</RightCell>
      <RightCell>Queued</RightCell>
      <RightCell>{""}</RightCell>
    </div>
  );
}

function OrderRow({
  order,
  market,
  accountId,
  publicKeyHex,
}: {
  order: AccountOrder;
  market: Market | undefined;
  accountId: number;
  publicKeyHex: string;
}) {
  const [cancelling, setCancelling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const latestHeight = useStore(selectLatestHeight);

  const sideRaw = order.side.toLowerCase();
  const isBuy = sideRaw.includes("buy");
  const outcome = sideRaw.includes("yes") ? "YES" : sideRaw.includes("no") ? "NO" : "";

  const limitNanos = parseNanos(order.limit_price_nanos);
  // value = limit × remaining (nanos × shares = dollars-nanos)
  const valueNanos = limitNanos * BigInt(order.remaining_quantity);
  const queuedFor =
    typeof latestHeight === "number" ? latestHeight + 1 : null;

  async function onCancel(e: React.MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    setError(null);
    setCancelling(true);
    try {
      await cancelSignedOrder({
        accountId,
        publicKeyHex,
        orderId: order.order_id,
        context: {
          marketId: order.market_id,
          side: order.side,
          qty: order.remaining_quantity,
          limitPriceNanos: String(order.limit_price_nanos),
        },
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setCancelling(false);
    }
  }

  return (
    <Link
      href={`/m/${order.market_id}`}
      style={{
        ...rowGrid("var(--fg-2)"),
        textDecoration: "none",
        color: "inherit",
        borderTop: "1px solid var(--border-1)",
      }}
    >
      <CategoryDot market={market} />
      <span
        style={{
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          color: "var(--fg-1)",
          fontFamily: "var(--font-sans)",
          fontSize: 13,
        }}
        title={market?.name ?? `#${order.market_id}`}
      >
        {market?.name ?? `#${order.market_id}`}
      </span>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          color: isBuy ? "var(--accent)" : "var(--no)",
          fontWeight: 600,
          letterSpacing: "var(--track-wide)",
        }}
      >
        {isBuy ? "BUY" : "SELL"}
      </span>
      <SidePill outcome={outcome} />
      <RightCell mono>
        <RemainingCell
          remaining={order.remaining_quantity}
          original={order.original_quantity ?? 0}
        />
      </RightCell>
      <RightCell mono>{formatCents(limitNanos)}</RightCell>
      <RightCell mono>
        {formatDollars(valueNanos, { decimals: 2 })}
      </RightCell>
      <RightCell>
        <TifCell expiresAtBlock={order.expires_at_block} />
      </RightCell>
      <RightCell mono>
        {queuedFor == null ? "—" : `#${queuedFor.toLocaleString()}`}
      </RightCell>
      <RightCell>
        <button
          type="button"
          onClick={onCancel}
          disabled={cancelling}
          title={error ?? "Cancel order"}
          style={{
            padding: "3px 9px",
            background: "transparent",
            border: "1px solid color-mix(in srgb, var(--no) 32%, transparent)",
            borderRadius: 3,
            color: "var(--no)",
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            cursor: cancelling ? "not-allowed" : "pointer",
            textTransform: "uppercase",
            letterSpacing: "var(--track-wide)",
          }}
        >
          {cancelling ? "…" : "Cancel"}
        </button>
      </RightCell>
    </Link>
  );
}

function RemainingCell({
  remaining,
  original,
}: {
  remaining: number;
  original: number;
}) {
  // Pre-B8 orders persisted without an `original_quantity` show
  // `original = 0`. Skip the progress bar for those — just the count.
  if (original === 0 || remaining >= original) {
    return <>{remaining}</>;
  }
  const filled = original - remaining;
  const pct = Math.min(1, Math.max(0, filled / original));
  return (
    <span
      style={{
        display: "inline-flex",
        flexDirection: "column",
        alignItems: "flex-end",
        gap: 2,
      }}
      title={`${filled} of ${original} filled`}
    >
      <span>{`${remaining} / ${original}`}</span>
      <span
        style={{
          height: 2,
          width: 60,
          background: "var(--border-1)",
          borderRadius: 1,
          overflow: "hidden",
        }}
      >
        <span
          style={{
            display: "block",
            height: "100%",
            width: `${pct * 100}%`,
            background: "var(--accent)",
          }}
        />
      </span>
    </span>
  );
}

function rowGrid(color: string): React.CSSProperties {
  return {
    display: "grid",
    gridTemplateColumns:
      "14px minmax(0, 1.4fr) 50px 50px 80px 60px 80px 110px 90px 70px",
    gap: 10,
    alignItems: "center",
    padding: "10px 14px",
    color,
    fontFamily: "var(--font-mono)",
    fontSize: 11,
    letterSpacing: "var(--track-wide)",
  };
}

function RightCell({
  children,
  mono,
}: {
  children: React.ReactNode;
  mono?: boolean;
}) {
  return (
    <span
      style={{
        textAlign: "right",
        fontFamily: mono ? "var(--font-mono)" : "inherit",
        fontSize: mono ? 12 : undefined,
        color: mono ? "var(--fg-1)" : undefined,
        whiteSpace: "nowrap",
      }}
    >
      {children}
    </span>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        padding: "32px 16px",
        background: "var(--surface-1)",
        border: "1px dashed var(--border-1)",
        borderRadius: 6,
        color: "var(--fg-4)",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        textAlign: "center",
      }}
    >
      {children}
    </div>
  );
}
