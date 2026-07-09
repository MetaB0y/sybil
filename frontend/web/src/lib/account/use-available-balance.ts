"use client";

/**
 * Cash an account can actually commit to a NEW order — total balance minus the
 * cash already reserved by its resting BUY orders.
 *
 * Mirrors the matching engine's reservation model (`validation.rs` +
 * `order_book.rs`, one global book keyed by account):
 *   - A BUY reserves `ceil(limit_price_nanos × remaining_quantity / 1000)` cash.
 *   - A SELL reserves position (shares), NOT cash.
 * The engine rejects a buy when `limit_price × max_fill > balance − reserved`,
 * surfacing `InsufficientBalance { required, available }`.
 *
 * The portfolio response now carries `available_balance_nanos` /
 * `reserved_balance_nanos` computed authoritatively server-side, so we prefer
 * those. Older API builds omit them — then we fall back to summing the account's
 * open BUY reservations client-side (`useAccountOrders` returns every open order
 * across all markets, matching the engine's account-global reservation).
 */

import { useMemo } from "react";
import { parseNanos } from "@/lib/format/nanos";
import type { Portfolio } from "./use-portfolio";
import { notionalNanosCeil } from "./quantity";
import { useAccountOrders, type AccountOrder } from "./use-account-orders";
import { usePortfolio } from "./use-portfolio";

export type AvailableBalance = {
  /** Total (gross) cash balance in nanos, or null until the portfolio loads. */
  balanceNanos: bigint | null;
  /** Cash locked by resting buy orders (nanos). 0 when none / not yet loaded. */
  reservedNanos: bigint;
  /** Cash free to bet: `max(0, balance − reserved)`, or null until loaded. */
  availableNanos: bigint | null;
  isPending: boolean;
};

/**
 * Derive spendable / reserved cash from a loaded portfolio. Prefers the
 * server-computed fields; when they're absent (older API) it optionally uses a
 * client-side reservation sum (`fallbackReservedNanos`, from open orders), and
 * otherwise reports the full balance as available.
 */
export function selectBalances(
  portfolio: Portfolio | null | undefined,
  fallbackReservedNanos = 0n,
): { balanceNanos: bigint | null; reservedNanos: bigint; availableNanos: bigint | null } {
  if (!portfolio) {
    return { balanceNanos: null, reservedNanos: 0n, availableNanos: null };
  }

  const balanceNanos = parseNanos(portfolio.balance_nanos);

  // Server-authoritative path: both fields present.
  if (
    portfolio.available_balance_nanos != null &&
    portfolio.reserved_balance_nanos != null
  ) {
    return {
      balanceNanos,
      reservedNanos: parseNanos(portfolio.reserved_balance_nanos),
      availableNanos: parseNanos(portfolio.available_balance_nanos),
    };
  }

  // Fallback (older API): subtract the client-computed reservation.
  const reservedNanos = fallbackReservedNanos;
  const availableNanos =
    balanceNanos > reservedNanos ? balanceNanos - reservedNanos : 0n;
  return { balanceNanos, reservedNanos, availableNanos };
}

/** Sum cash reserved by an account's open BUY orders. Sells reserve shares. */
function sumBuyReservations(orders: AccountOrder[] | undefined): bigint {
  let reservedNanos = 0n;
  for (const o of orders ?? []) {
    if (!o.side?.toLowerCase().includes("buy")) continue;
    reservedNanos += notionalNanosCeil(
      parseNanos(o.limit_price_nanos),
      o.remaining_quantity,
    );
  }
  return reservedNanos;
}

export function useAvailableBalance(accountId: number | null): AvailableBalance {
  const portfolio = usePortfolio(accountId);
  const orders = useAccountOrders(accountId);

  return useMemo(() => {
    const fallbackReserved = sumBuyReservations(orders.data);
    const { balanceNanos, reservedNanos, availableNanos } = selectBalances(
      portfolio.data,
      fallbackReserved,
    );
    return {
      balanceNanos,
      reservedNanos,
      availableNanos,
      isPending: portfolio.isPending || orders.isPending,
    };
  }, [portfolio.data, portfolio.isPending, orders.data, orders.isPending]);
}
