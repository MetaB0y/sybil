"use client";

/**
 * Open orders tab. Grid rows:
 *   dot · market · action · side · created · placed/filled · limit · avg fill · value · TIF · cancel
 *
 * - Placed/filled uses B8's `original_quantity` (placed) and `original −
 *   remaining` (filled). Orders persisted before B8 report `original_quantity:
 *   0`; we fall back to the bare remaining count for them.
 * - Fill count + avg fill price are derived from the account's `/fills` feed by
 *   `order_id`. Bounded by the fills window, so very old / heavily-filled orders
 *   may undercount — fine for typical recent open orders.
 * - Created time is the exact `created_at_ms` from `PendingOrderResponse`
 *   (falls back to the block height for orders admitted before that field).
 */

import Link from "next/link";
import { useMemo, useState } from "react";
import { cancelSignedOrder } from "@/lib/account/orders";
import type { AccountFill } from "@/lib/account/use-account-fills";
import type { AccountOrder } from "@/lib/account/use-account-orders";
import {
  formatAge,
  formatCents,
  formatDollars,
  parseNanos,
} from "@/lib/format/nanos";
import { selectLatestBlock, useStore } from "@/lib/store";
import type { components } from "@/lib/api/schema";
import { CategoryDot } from "./category-dot";
import { SidePill } from "./side-pill";
import { TifCell } from "./tif-cell";

type Market = components["schemas"]["MarketResponse"];

interface OrderFillAgg {
  count: number;
  avgPriceNanos: bigint | null;
}

interface Props {
  accountId: number;
  publicKeyHex: string;
  orders: AccountOrder[];
  fills: AccountFill[];
  marketsById: Map<number, Market>;
}

export function OpenOrdersList({
  accountId,
  publicKeyHex,
  orders,
  fills,
  marketsById,
}: Props) {
  // Aggregate the account's visible fills by order_id → count + WAC fill price.
  const fillsByOrder = useMemo(() => {
    const acc = new Map<number, { count: number; qty: bigint; cost: bigint }>();
    for (const f of fills) {
      const e = acc.get(f.order_id) ?? { count: 0, qty: 0n, cost: 0n };
      const qty = BigInt(f.fill_qty);
      const price = parseNanos(f.fill_price_nanos);
      e.count += 1;
      e.qty += qty;
      e.cost += qty * price;
      acc.set(f.order_id, e);
    }
    const out = new Map<number, OrderFillAgg>();
    for (const [id, e] of acc) {
      out.set(id, {
        count: e.count,
        avgPriceNanos: e.qty > 0n ? e.cost / e.qty : null,
      });
    }
    return out;
  }, [fills]);

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
          agg={fillsByOrder.get(o.order_id)}
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
      <span>Created</span>
      <RightCell>Placed / Filled</RightCell>
      <RightCell>Limit</RightCell>
      <RightCell>Avg fill</RightCell>
      <RightCell>Value</RightCell>
      <RightCell>TIF</RightCell>
      <RightCell>{""}</RightCell>
    </div>
  );
}

function OrderRow({
  order,
  market,
  agg,
  accountId,
  publicKeyHex,
}: {
  order: AccountOrder;
  market: Market | undefined;
  agg: OrderFillAgg | undefined;
  accountId: number;
  publicKeyHex: string;
}) {
  const [cancelling, setCancelling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const latestBlock = useStore(selectLatestBlock);

  const sideRaw = order.side.toLowerCase();
  const isBuy = sideRaw.includes("buy");
  const outcome = sideRaw.includes("yes") ? "YES" : sideRaw.includes("no") ? "NO" : "";

  const limitNanos = parseNanos(order.limit_price_nanos);
  // value = limit × remaining (nanos × shares = dollars-nanos)
  const valueNanos = limitNanos * BigInt(order.remaining_quantity);

  const placed = order.original_quantity ?? 0;
  const filled = placed > 0 ? Math.max(0, placed - order.remaining_quantity) : 0;

  // Exact created time from the backend (created_at_ms on PendingOrderResponse).
  // Falls back to null (→ block-height display) for orders admitted before the
  // field shipped, which report created_at_ms: 0.
  const createdMs =
    order.created_at_ms && order.created_at_ms > 0 ? order.created_at_ms : null;

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
      <CreatedCell
        ms={createdMs}
        block={order.created_at_block}
        nowMs={latestBlock?.timestamp_ms ?? null}
      />
      <RightCell mono>
        <FilledCell
          placed={placed}
          filled={filled}
          remaining={order.remaining_quantity}
        />
      </RightCell>
      <RightCell mono>{formatCents(limitNanos)}</RightCell>
      <RightCell mono>
        <AvgFillCell agg={agg} />
      </RightCell>
      <RightCell mono>{formatDollars(valueNanos, { decimals: 2 })}</RightCell>
      <RightCell>
        <TifCell expiresAtBlock={order.expires_at_block} />
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

/** Created-time cell — exact wall-clock from backend `created_at_ms`. */
function CreatedCell({
  ms,
  block,
  nowMs,
}: {
  ms: number | null;
  block: number;
  nowMs: number | null;
}) {
  return (
    <span
      style={{
        display: "inline-flex",
        flexDirection: "column",
        gap: 1,
        fontFamily: "var(--font-mono)",
      }}
    >
      <span style={{ fontSize: 11, color: "var(--fg-2)" }}>
        {ms == null || nowMs == null ? "—" : `${formatAge(nowMs - ms)} ago`}
      </span>
      <span
        style={{
          fontSize: 9.5,
          color: "var(--fg-4)",
          letterSpacing: "var(--track-wide)",
        }}
      >
        #{block.toLocaleString()}
      </span>
    </span>
  );
}

/** Placed / filled cell with a thin filled-fraction progress bar. */
function FilledCell({
  placed,
  filled,
  remaining,
}: {
  placed: number;
  filled: number;
  remaining: number;
}) {
  // Pre-B8 orders have no authoritative placed count — show bare remaining.
  if (placed === 0) {
    return <>{remaining}</>;
  }
  const pct = Math.min(1, Math.max(0, filled / placed));
  return (
    <span
      style={{
        display: "inline-flex",
        flexDirection: "column",
        alignItems: "flex-end",
        gap: 2,
      }}
      title={`${filled} filled of ${placed} placed`}
    >
      <span>{`${placed} / ${filled}`}</span>
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

/** Avg fill price (WAC of matched fills) with fill count beneath. */
function AvgFillCell({ agg }: { agg: OrderFillAgg | undefined }) {
  const count = agg?.count ?? 0;
  return (
    <span
      style={{
        display: "inline-flex",
        flexDirection: "column",
        alignItems: "flex-end",
        gap: 1,
        fontFamily: "var(--font-mono)",
      }}
    >
      <span style={{ fontSize: 12, color: count > 0 ? "var(--fg-1)" : "var(--fg-3)" }}>
        {agg?.avgPriceNanos != null ? formatCents(agg.avgPriceNanos) : "—"}
      </span>
      <span
        style={{
          fontSize: 9.5,
          color: "var(--fg-4)",
          letterSpacing: "var(--track-wide)",
        }}
      >
        {count === 1 ? "1 fill" : `${count} fills`}
      </span>
    </span>
  );
}

function rowGrid(color: string): React.CSSProperties {
  return {
    display: "grid",
    gridTemplateColumns:
      "14px minmax(0, 1.25fr) 46px 44px 80px 96px 52px 70px 78px 96px 60px",
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
