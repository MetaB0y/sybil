"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { useAccountEvents } from "@/lib/account/use-account-events";
import { useTrackedCancels } from "@/lib/account/use-cancelled-orders";
import { api } from "@/lib/api/client";
import { BLOCK_INTERVAL_MS } from "@/lib/constants";
import { parseNanos } from "@/lib/format/nanos";
import type { PendingOrder } from "@/lib/markets/use-pending-orders";
import { selectLatestHeight, useStore } from "@/lib/store";
import type { DegenSide } from "./degen";
import {
  findDegenOrderId,
  findDegenPendingOrderId,
  resolveDegenBet,
  type DegenBetState,
  type DegenEvent,
} from "./track";

export interface DegenActive {
  accountId: number;
  marketId: number;
  outcome: DegenSide;
  targetQty: bigint;
  /** The dollar stake at submit time. Carried here (not read from the live
   *  form) so the result card stays correct after the rail unmounts/remounts
   *  — e.g. when the user toggles to Pro and back. */
  betUsd: number;
  limitPriceNanos: bigint; // for display only
  submitHeight: number;
  /** `latestBlockAnchorPerf` (monotonic perf-clock) of the block the submit batch
   *  built on — i.e. when the current open batch started. Used to LOCATE the bet's
   *  on-chain expiry on the perf clock (`batchAnchorPerfMs + gtdWindowMs`), which
   *  is pinned to block production, not to the submit instant. Frozen at submit so
   *  it survives a Degen↔Pro toggle. */
  batchAnchorPerfMs: number;
  /** `performance.now()` at submit — the start of the bet's lifetime. The progress
   *  bar and ⏱ span from here to expiry, so a bet placed late in a batch shows its
   *  true (shorter) remaining time from an EMPTY bar — e.g. 112s when submitted
   *  8s into the batch, not the nominal 120s already 8s in. Frozen so it survives a
   *  Degen↔Pro toggle. */
  submitPerfMs: number;
  expiresAtBlock: number;
  /**
   * Highest order id already present for this market at submit (snapshot of the
   * events + pending feeds). The matchers only bind ids strictly above this, so
   * a fresh bet can never re-bind one of the account's earlier orders on the
   * same market+side — the bug where a repeat bet instantly read
   * "Successfully bet…" off the previous (already-filled) order. Null on the
   * first bet (nothing to exclude). See `priorMaxOrderId`.
   */
  priorMaxOrderId: number | null;
}

export interface DegenTracking extends DegenBetState {
  secondsLeft: number;
  timeProgress01: number;
  /**
   * The bet's bound order id — from the pending-orders feed (~1s after submit,
   * mid-batch) or the events feed (`placed`/fill row, at the next clear),
   * whichever lands first. The progress card's Cancel button needs this to call
   * `cancelSignedOrder`; it stays disabled only until the id first registers.
   */
  orderId: number | null;
}

/**
 * Track an in-flight degen bet off the account events feed. Returns null when
 * inactive. One cumulative RAF clock drives the progress card: both the bar
 * (`timeProgress01`) and the ⏱ number (`secondsLeft`) span the bet's whole GTD
 * window (submit batch → expiry) from the frozen submit anchor — so `secondsLeft`
 * counts the *total* time left until the bet resolves/expires (not the per-batch
 * clear) and always agrees with the bar; both fill/drain once and never reset.
 * `phase`/`filledQty`/`avgPriceNanos` come from the pure reducers.
 */
export function useDegenBetTracker(
  active: DegenActive | null,
): DegenTracking | null {
  const { data: rawEvents } = useAccountEvents(active?.accountId ?? null);
  // Local cancel log — the bridge that lets a cancel from anywhere (the
  // open-orders table or this card) flip the bet to its terminal state, since
  // the backend emits no OrderCancelled event ([[use-cancelled-orders]]).
  const trackedCancels = useTrackedCancels(active?.accountId ?? null);
  const latestHeight = useStore(selectLatestHeight);

  // Normalize the events feed up front so we can bind the order id from it.
  const events: DegenEvent[] = (rawEvents ?? []).map((e) => ({
    type: e.type,
    blockHeight: e.block_height,
    marketId: e.market_id ?? null,
    orderId: e.order_id ?? null,
    side: e.side ?? null,
    outcome: e.outcome ?? null,
    qty: e.qty != null ? BigInt(e.qty) : 0n,
    priceNanos: e.price_nanos != null ? parseNanos(e.price_nanos) : 0n,
  }));
  // Authoritative id from the events feed — but the `placed` row only commits at
  // the next batch clear, so it's null for the first ~batch.
  const eventsBoundId = active
    ? findDegenOrderId(events, {
        marketId: active.marketId,
        outcome: active.outcome,
        submitHeight: active.submitHeight,
        minOrderIdExclusive: active.priorMaxOrderId,
      })
    : null;
  // Faster path: the backend assigns the id at submit and lists the resting order
  // in the pending feed within ~1s — during the open batch. Bind from there so
  // Cancel unlocks immediately instead of waiting for `placed` at the next clear.
  const pendingStatus = usePendingOrderStatus(
    active,
    eventsBoundId,
    latestHeight,
  );
  const pendingBoundId = pendingStatus.pendingBoundId;

  // Cumulative clock anchored to the *submit moment*, spanning the bet's real
  // remaining lifetime (submit → on-chain expiry). Expiry is pinned to block
  // production: `expiresAtBlock` is produced `gtdWindowMs` after the submit batch's
  // block (`batchAnchorPerfMs`), so a bet placed late in a batch has LESS than the
  // nominal GTD left — submit with 2s to clear → 112s, not 120s. The bar fills 0→1
  // over THAT lifetime (starts empty, never partway), and the ⏱ number counts the
  // same span down — one interval, no per-batch reset.
  const submitPerf = active?.submitPerfMs ?? null;
  const gtdWindowMs =
    active == null
      ? 0
      : Math.max(
          1,
          (active.expiresAtBlock - active.submitHeight) * BLOCK_INTERVAL_MS,
        );
  const lifetimeMs =
    active == null
      ? 1
      : Math.max(
          1,
          active.batchAnchorPerfMs + gtdWindowMs - active.submitPerfMs,
        );

  const [progressSnapshot, setProgressSnapshot] =
    useState<ProgressSnapshot | null>(null);
  const rafRef = useRef<number>(0);
  const timeProgress01 =
    submitPerf === null
      ? 0
      : progressSnapshot?.submitPerf === submitPerf &&
          progressSnapshot.lifetimeMs === lifetimeMs
        ? progressSnapshot.value
        : computeProgress01(submitPerf, lifetimeMs);

  useEffect(() => {
    if (submitPerf === null) return;
    let last = 0;
    const step = (t: number) => {
      if (t - last >= 100) {
        last = t;
        setProgressSnapshot({
          submitPerf,
          lifetimeMs,
          value: computeProgress01(submitPerf, lifetimeMs),
        });
      }
      rafRef.current = requestAnimationFrame(step);
    };
    rafRef.current = requestAnimationFrame(step);
    return () => cancelAnimationFrame(rafRef.current);
  }, [submitPerf, lifetimeMs]);

  if (!active) return null;

  // Prefer the events id (carries fills); fall back to the pending id so Cancel
  // is live during the open batch. Both resolve to the same order once placed.
  const boundId = eventsBoundId ?? pendingBoundId;
  const ours =
    boundId === null ? [] : events.filter((e) => e.orderId === boundId);

  // The bet is cancelled once we've bound an order id and the local cancel log
  // has a record for it. Pre-binding (boundId null) we can't correlate a cancel,
  // but a still-unacked order has nothing resting to cancel anyway.
  const cancelled =
    boundId !== null && trackedCancels.some((c) => c.orderId === boundId);

  const state = resolveDegenBet({
    targetQty: active.targetQty,
    currentHeight: latestHeight ?? active.submitHeight,
    expiresAtBlock: active.expiresAtBlock,
    events: ours,
    orderOpen: pendingStatus.orderOpen,
    cancelled,
  });

  // Whole-seconds left until the bet resolves/expires — the same lifetime span as
  // the bar, viewed as a countdown, so the ⏱ number and the bar always agree.
  const secondsLeft = Math.max(
    0,
    Math.ceil((lifetimeMs * (1 - timeProgress01)) / 1000),
  );

  return { ...state, secondsLeft, timeProgress01, orderId: boundId };
}

/**
 * Bind the active bet's order id from the pending-orders feed
 * (`/v1/accounts/{id}/orders`). Shares the cache key with `useAccountOrders`,
 * but while `unbound` (the bet is live and the events feed hasn't surfaced the
 * id yet) it polls at ~1s so the id — which the backend lists within ~1s of
 * submit, mid-batch — is picked up promptly and Cancel unlocks without waiting
 * for the next clear. Polling stops the moment the bet binds or clears.
 */
function usePendingOrderStatus(
  active: DegenActive | null,
  eventsBoundId: number | null,
  latestHeight: number | null,
): DegenPendingStatus {
  const accountId = active?.accountId ?? null;
  const qc = useQueryClient();
  const { data, isSuccess } = useQuery({
    enabled: accountId !== null,
    queryKey: ["account", accountId, "orders"],
    queryFn: async (): Promise<PendingOrder[]> => {
      if (accountId === null) throw new Error("no account");
      const { data, error } = await api.GET("/v1/accounts/{id}/orders", {
        params: { path: { id: accountId } },
      });
      if (error || !data) throw new Error("fetch account orders failed");
      return data;
    },
    refetchInterval:
      accountId !== null && eventsBoundId === null ? 1000 : false,
    staleTime: 0,
    refetchOnWindowFocus: false,
  });

  // Once the order has an events-feed id, the fast registration poll above can
  // stop. Keep refreshing open orders on each block so expiry is confirmed by
  // the order actually disappearing from the live feed, even when the terminal
  // `expired` event is delayed or missed.
  useEffect(() => {
    if (accountId === null) return;
    qc.invalidateQueries({ queryKey: ["account", accountId, "orders"] });
  }, [accountId, latestHeight, qc]);

  return resolveDegenPendingStatus(
    data ?? [],
    active,
    eventsBoundId,
    isSuccess,
  );
}

export interface DegenPendingStatus {
  pendingBoundId: number | null;
  /** True/false after a successful open-orders fetch; null while unknown. */
  orderOpen: boolean | null;
}

/** Pure pending-feed correlation used by the tracker and its lifecycle tests. */
export function resolveDegenPendingStatus(
  pending: PendingOrder[],
  active: DegenActive | null,
  eventsBoundId: number | null,
  ordersLoaded: boolean,
): DegenPendingStatus {
  if (!active) return { pendingBoundId: null, orderOpen: null };

  const pendingBoundId = findDegenPendingOrderId(pending, {
    marketId: active.marketId,
    outcome: active.outcome,
    submitHeight: active.submitHeight,
    minOrderIdExclusive: active.priorMaxOrderId,
  });
  const boundId = eventsBoundId ?? pendingBoundId;
  const orderOpen = ordersLoaded
    ? boundId !== null && pending.some((o) => o.order_id === boundId)
    : null;

  return { pendingBoundId, orderOpen };
}

interface ProgressSnapshot {
  submitPerf: number;
  lifetimeMs: number;
  value: number;
}

function computeProgress01(submitPerf: number, lifetimeMs: number) {
  const now =
    typeof performance === "undefined" ? submitPerf : performance.now();
  return Math.min(1, Math.max(0, (now - submitPerf) / lifetimeMs));
}
