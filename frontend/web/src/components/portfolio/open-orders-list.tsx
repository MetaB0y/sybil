"use client";

/**
 * Open orders tab. Grid rows:
 *   dot · market · action · side · placed/filled · limit · avg fill · value ·
 *   created · TIF · cancel
 *
 * - Placed/filled uses B8's `original_quantity` (placed) and `original −
 *   remaining` (filled). Orders persisted before B8 report `original_quantity:
 *   0`; we fall back to the bare remaining count for them.
 * - Fill count + avg fill price come from the account's durable history log
 *   (`partial_fill`/`filled` events aggregated by `order_id` in `fillAggByOrder`),
 *   NOT the `/fills` endpoint — which returns `[]` in prod, so this column used
 *   to read "— / 0 fills" even for orders that had clearly filled.
 * - Created time is the exact `created_at_ms` from `PendingOrderResponse`
 *   (falls back to the block height for orders admitted before that field).
 * - Every column is click-to-sort; default order is newest-first by created
 *   time. Paginated at PORTFOLIO_PAGE_SIZE rows/page.
 *
 * The row itself is not a link — it carries a Cancel button — so only the
 * market cell navigates. Row chrome comes from `./table` like every other tab.
 */

import Link from "next/link";
import { useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { cancelSignedOrder } from "@/lib/account/orders";
import { formatShareUnits, notionalNanosCeil } from "@/lib/account/quantity";
import type { AccountOrder } from "@/lib/account/use-account-orders";
import type { OrderFillAgg } from "@/lib/account/use-account-history";
import {
  formatAge,
  formatCentsPrecise,
  formatDollars,
  parseNanos,
} from "@/lib/format/nanos";
import { selectLatestBlock, useStore } from "@/lib/store";
import type { components } from "@/lib/api/schema";
import { MarketThumb } from "@/components/market-thumb";
import {
  Pager,
  usePaged,
  PORTFOLIO_PAGE_SIZE,
} from "@/components/event-list-pager";
import { PortfolioToolbar } from "./portfolio-toolbar";
import { SearchField } from "./search-field";
import { SidePill } from "./side-pill";
import { TifCell } from "./tif-cell";
import {
  ActionCell,
  bodyRowGrid,
  cmpBig,
  cmpNullableBig,
  Empty,
  MarketLabel,
  nextSort,
  PagerFooter,
  RightCell,
  SortHeader,
  TableCard,
  TableHead,
  type Column,
  type Sort,
} from "./table";
import { DataCard } from "@/components/data-card";
import { useCompactLayout } from "@/lib/responsive/use-compact";

type Market = components["schemas"]["MarketResponse"];

/** An order with every sortable value derived once. */
export interface OpenRow {
  order: AccountOrder;
  market: Market | undefined;
  label: string;
  action: "BUY" | "SELL";
  outcome: string;
  placed: number;
  filled: number;
  remaining: number;
  limitNanos: bigint;
  valueNanos: bigint;
  avgPriceNanos: bigint | null;
  fillCount: number;
  createdMs: number | null;
  createdBlock: number;
  expiresAtBlock: number;
}

type SortKey =
  | "market"
  | "action"
  | "side"
  | "placed"
  | "limit"
  | "avgfill"
  | "value"
  | "created"
  | "tif";

const GRID =
  "28px minmax(0, 1.3fr) 56px 48px 100px 56px 84px 82px 76px 92px 64px";

const COLUMNS: Column<SortKey>[] = [
  { key: "market", label: "Market", align: "left" },
  { key: "action", label: "Action", align: "left" },
  { key: "side", label: "Side", align: "left" },
  { key: "placed", label: "Filled / Placed", align: "right" },
  { key: "limit", label: "Limit", align: "right" },
  { key: "avgfill", label: "Avg fill", align: "right" },
  { key: "value", label: "Value", align: "right" },
  { key: "created", label: "Created", align: "right" },
  { key: "tif", label: "TIF", align: "right" },
];

/** Ascending comparison; null avg-fill / created sort lowest. */
function compareBy(a: OpenRow, b: OpenRow, key: SortKey): number {
  switch (key) {
    case "market":
      return a.label.localeCompare(b.label);
    case "action":
      return a.action.localeCompare(b.action);
    case "side":
      return a.outcome.localeCompare(b.outcome);
    case "placed":
      return (a.placed || a.remaining) - (b.placed || b.remaining);
    case "limit":
      return cmpBig(a.limitNanos, b.limitNanos);
    case "avgfill":
      return cmpNullableBig(a.avgPriceNanos, b.avgPriceNanos);
    case "value":
      return cmpBig(a.valueNanos, b.valueNanos);
    case "created":
      return (a.createdMs ?? 0) - (b.createdMs ?? 0);
    case "tif":
      return a.expiresAtBlock - b.expiresAtBlock;
  }
}

interface Props {
  tabs: React.ReactNode;
  accountId: number;
  publicKeyHex: string;
  orders: AccountOrder[];
  /** Per-order fill count + avg price, aggregated from the durable history log
   *  (see `fillAggByOrder`) — the `/fills` endpoint is empty in prod. */
  fillsByOrder: Map<number, OrderFillAgg>;
  marketsById: Map<number, Market>;
  /** Natural question titles where a Polymarket snapshot is available. */
  titleByMarket: Map<number, string>;
}

export function OpenOrdersList({
  tabs,
  accountId,
  publicKeyHex,
  orders,
  fillsByOrder,
  marketsById,
  titleByMarket,
}: Props) {
  const [sort, setSort] = useState<Sort<SortKey> | null>(null);
  const [query, setQuery] = useState("");
  const qc = useQueryClient();
  const nowMs = useStore(selectLatestBlock)?.timestamp_ms ?? null;

  function onCancelled(orderId: number) {
    qc.setQueryData<AccountOrder[]>(
      ["account", accountId, "orders"],
      (current) => current?.filter((order) => order.order_id !== orderId),
    );
    void Promise.allSettled([
      qc.invalidateQueries({ queryKey: ["account", accountId, "orders"] }),
      qc.invalidateQueries({ queryKey: ["account", accountId, "portfolio"] }),
      qc.invalidateQueries({ queryKey: ["orders", "pending"] }),
    ]);
  }

  const rows = useMemo<OpenRow[]>(() => {
    const decorated = orders.map((o) => {
      const sideRaw = o.side.toLowerCase();
      const agg = fillsByOrder.get(o.order_id);
      const placed = o.original_quantity ?? 0;
      const limitNanos = parseNanos(o.limit_price_nanos);
      return {
        order: o,
        market: marketsById.get(o.market_id),
        label:
          titleByMarket.get(o.market_id) ??
          marketsById.get(o.market_id)?.name ??
          `#${o.market_id}`,
        action: sideRaw.includes("buy") ? "BUY" : "SELL",
        outcome: sideRaw.includes("yes")
          ? "YES"
          : sideRaw.includes("no")
            ? "NO"
            : "",
        placed,
        filled: placed > 0 ? Math.max(0, placed - o.remaining_quantity) : 0,
        remaining: o.remaining_quantity,
        limitNanos,
        valueNanos: notionalNanosCeil(limitNanos, o.remaining_quantity),
        avgPriceNanos: agg?.avgPriceNanos ?? null,
        fillCount: agg?.count ?? 0,
        createdMs:
          o.created_at_ms && o.created_at_ms > 0 ? o.created_at_ms : null,
        createdBlock: o.created_at_block,
        expiresAtBlock: o.expires_at_block,
      } satisfies OpenRow;
    });
    if (!sort) {
      // Default: newest-first by created time.
      return [...decorated].sort(
        (a, b) => (b.createdMs ?? 0) - (a.createdMs ?? 0),
      );
    }
    const factor = sort.dir === "asc" ? 1 : -1;
    return [...decorated].sort((a, b) => compareBy(a, b, sort.key) * factor);
  }, [orders, fillsByOrder, marketsById, titleByMarket, sort]);

  const visibleRows = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return rows;
    return rows.filter((r) => r.label.toLowerCase().includes(q));
  }, [rows, query]);

  const paged = usePaged(visibleRows, PORTFOLIO_PAGE_SIZE);

  const onSort = (key: SortKey) => {
    setSort((s) =>
      nextSort(s, key, key !== "market" && key !== "action" && key !== "side"),
    );
    paged.setPage(0);
  };

  const onSearch = (v: string) => {
    setQuery(v);
    paged.setPage(0);
  };

  const isEmpty = orders.length === 0;
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-3)",
      }}
    >
      <PortfolioToolbar
        tabs={tabs}
        search={!isEmpty && <SearchField value={query} onChange={onSearch} />}
      />
      {isEmpty ? (
        <Empty>No open orders.</Empty>
      ) : visibleRows.length === 0 ? (
        <Empty>No open orders match “{query}”.</Empty>
      ) : (
        <TableCard>
          <TableHead columns={GRID}>
            <span />
            {COLUMNS.map((col) => (
              <SortHeader key={col.key} col={col} sort={sort} onSort={onSort} />
            ))}
            <span />
          </TableHead>
          {paged.visible.map((r) => (
            <OrderRow
              key={r.order.order_id}
              row={r}
              nowMs={nowMs}
              accountId={accountId}
              publicKeyHex={publicKeyHex}
              onCancelled={onCancelled}
            />
          ))}
          <PagerFooter>
            <Pager paged={paged} />
          </PagerFooter>
        </TableCard>
      )}
    </div>
  );
}

export function OrderRow({
  row,
  nowMs,
  accountId,
  publicKeyHex,
  onCancelled,
}: {
  row: OpenRow;
  nowMs: number | null;
  accountId: number;
  publicKeyHex: string;
  onCancelled: (orderId: number) => void;
}) {
  const { order, market, label, action, outcome, placed, filled, remaining } =
    row;
  const [cancelling, setCancelling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const compact = useCompactLayout();

  async function onCancel() {
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
      onCancelled(order.order_id);
      setCancelling(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setCancelling(false);
    }
  }

  const cancelButton = (
    <button
      type="button"
      onClick={onCancel}
      disabled={cancelling}
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
  );

  const cancelError = error && (
    <span
      role="alert"
      style={{
        gridColumn: "1 / -1",
        color: "var(--no)",
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        lineHeight: 1.4,
      }}
    >
      Couldn&apos;t cancel order: {error}
    </span>
  );

  if (compact) {
    // Cancel is a button inside the row, so the card cannot be a link — the
    // market name carries the navigation instead, as it does in the desktop
    // Orders row for the same reason.
    return (
      <DataCard
        thumb={
          <MarketThumb
            marketId={order.market_id}
            name={label}
            imageUrl={market?.market_image_url ?? market?.event_image_url ?? null}
            fallbackIconUrl={
              market?.market_icon_url ?? market?.event_icon_url ?? null
            }
            size={28}
          />
        }
        title={
          // Deliberately not `MarketLabel`: that one truncates to a single
          // nowrap line for the desktop column, which on a card runs the
          // question straight off the right edge.
          <Link
            href={`/m/${order.market_id}`}
            style={{ color: "inherit", textDecoration: "none" }}
          >
            {label}
          </Link>
        }
        chips={
          <>
            <ActionCell side={action} />
            <SidePill outcome={outcome} />
            <TifCell expiresAtBlock={order.expires_at_block} />
          </>
        }
        pairs={[
          {
            label: "Filled / placed",
            value: (
              <FilledCell
                placed={placed}
                filled={filled}
                remaining={remaining}
              />
            ),
            wide: true,
          },
          { label: "Limit", value: formatCentsPrecise(row.limitNanos) },
          {
            label: "Avg fill",
            value: (
              <AvgFillCell
                agg={{ count: row.fillCount, avgPriceNanos: row.avgPriceNanos }}
              />
            ),
          },
          {
            label: "Value",
            value: formatDollars(row.valueNanos, { decimals: 2 }),
          },
          {
            label: "Created",
            value: (
              <CreatedCell
                ms={row.createdMs}
                block={row.createdBlock}
                nowMs={nowMs}
              />
            ),
          },
        ]}
        footer={
          <>
            {cancelButton}
            {cancelError}
          </>
        }
      />
    );
  }

  return (
    <div
      className="portfolio-row"
      data-order-id={order.order_id}
      style={bodyRowGrid(GRID)}
    >
      <Link
        href={`/m/${order.market_id}`}
        style={{
          gridColumn: "1 / span 2",
          display: "grid",
          gridTemplateColumns: "28px minmax(0, 1fr)",
          gap: 12,
          alignItems: "center",
          minWidth: 0,
          borderRadius: 3,
          color: "inherit",
          textDecoration: "none",
        }}
      >
        <MarketThumb
          marketId={order.market_id}
          name={label}
          imageUrl={market?.market_image_url ?? market?.event_image_url ?? null}
          fallbackIconUrl={
            market?.market_icon_url ?? market?.event_icon_url ?? null
          }
          size={28}
        />
        <MarketLabel>{label}</MarketLabel>
      </Link>
      <ActionCell side={action} />
      <SidePill outcome={outcome} />
      <RightCell mono>
        <FilledCell placed={placed} filled={filled} remaining={remaining} />
      </RightCell>
      <RightCell mono>{formatCentsPrecise(row.limitNanos)}</RightCell>
      <RightCell mono>
        <AvgFillCell
          agg={{ count: row.fillCount, avgPriceNanos: row.avgPriceNanos }}
        />
      </RightCell>
      <RightCell mono>
        {formatDollars(row.valueNanos, { decimals: 2 })}
      </RightCell>
      <RightCell mono>
        <CreatedCell
          ms={row.createdMs}
          block={row.createdBlock}
          nowMs={nowMs}
        />
      </RightCell>
      <RightCell>
        <TifCell expiresAtBlock={order.expires_at_block} />
      </RightCell>
      <RightCell>{cancelButton}</RightCell>
      {cancelError}
    </div>
  );
}

/** Compact relative admission time; exact wall clock and block remain on hover. */
function CreatedCell({
  ms,
  block,
  nowMs,
}: {
  ms: number | null;
  block: number;
  nowMs: number | null;
}) {
  if (ms == null || nowMs == null) {
    return <span style={{ color: "var(--fg-4)" }}>—</span>;
  }
  return (
    <span
      title={`${new Date(ms).toLocaleString()} · batch #${block.toLocaleString()}`}
      style={{ whiteSpace: "nowrap" }}
    >
      {formatAge(nowMs - ms)} ago
    </span>
  );
}

/** Filled / placed on one line, matching the vetted market-detail table. */
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
    return <>{formatShareUnits(remaining, 1)}</>;
  }
  return (
    <span>
      {`${formatShareUnits(filled, 1)} / ${formatShareUnits(placed, 1)}`}
    </span>
  );
}

/** Avg fill price with the fill count as a quiet one-line suffix. */
function AvgFillCell({ agg }: { agg: OrderFillAgg }) {
  const count = agg.count;
  return (
    <span
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        whiteSpace: "nowrap",
      }}
    >
      <span style={{ color: count > 0 ? "var(--fg-1)" : "var(--fg-3)" }}>
        {agg.avgPriceNanos != null
          ? formatCentsPrecise(agg.avgPriceNanos)
          : "—"}
      </span>
      {count > 0 && (
        <span
          style={{ color: "var(--fg-4)", fontSize: 10 }}
        >{` ·${count}`}</span>
      )}
    </span>
  );
}
