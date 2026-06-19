"use client";

/**
 * Cash an account can actually commit to a NEW order — total balance minus the
 * cash already reserved by its resting BUY orders.
 *
 * Mirrors the matching engine's reservation model (`validation.rs` +
 * `order_book.rs`, one global book keyed by account):
 *   - A BUY reserves `limit_price_nanos × remaining_quantity` cash.
 *   - A SELL reserves position (shares), NOT cash.
 * The engine rejects a buy when `limit_price × max_fill > balance − reserved`,
 * surfacing `InsufficientBalance { required, available }`. The frontend only
 * has total `balance_nanos`, so without subtracting reservations the "MAX"
 * button and balance line can propose an amount the engine then rejects.
 *
 * `useAccountOrders` already returns every open order for the account across
 * all markets, which matches the engine's account-global reservation.
 */

import { useMemo } from "react";
import { parseNanos } from "@/lib/format/nanos";
import { useAccountOrders } from "./use-account-orders";
import { usePortfolio } from "./use-portfolio";

export type AvailableBalance = {
  /** Total cash balance in nanos, or null until the portfolio loads. */
  balanceNanos: bigint | null;
  /** Cash locked by resting buy orders (nanos). 0 when none / not yet loaded. */
  reservedNanos: bigint;
  /** Cash free to bet: `max(0, balance − reserved)`, or null until loaded. */
  availableNanos: bigint | null;
  isPending: boolean;
};

export function useAvailableBalance(accountId: number | null): AvailableBalance {
  const portfolio = usePortfolio(accountId);
  const orders = useAccountOrders(accountId);

  return useMemo(() => {
    const balanceNanos = portfolio.data
      ? parseNanos(portfolio.data.balance_nanos)
      : null;

    // Sum buy-order cash reservations. Sells reserve shares, not cash.
    let reservedNanos = 0n;
    for (const o of orders.data ?? []) {
      if (!o.side?.toLowerCase().includes("buy")) continue;
      reservedNanos += parseNanos(o.limit_price_nanos) * BigInt(o.remaining_quantity);
    }

    const availableNanos =
      balanceNanos == null
        ? null
        : balanceNanos > reservedNanos
          ? balanceNanos - reservedNanos
          : 0n;

    return {
      balanceNanos,
      reservedNanos,
      availableNanos,
      isPending: portfolio.isPending || orders.isPending,
    };
  }, [portfolio.data, portfolio.isPending, orders.data, orders.isPending]);
}
