"use client";

import { useEffect, useRef, useState } from "react";
import { useAccountEvents } from "@/lib/account/use-account-events";
import { BLOCK_INTERVAL_MS } from "@/lib/constants";
import { parseNanos } from "@/lib/format/nanos";
import { selectLatestHeight, useStore } from "@/lib/store";
import type { DegenSide } from "./degen";
import {
  findDegenOrderId,
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
  expiresAtBlock: number;
}

export interface DegenTracking extends DegenBetState {
  secondsLeft: number;
  timeProgress01: number;
}

/**
 * Track an in-flight degen bet off the account events feed. Returns null when
 * inactive. `timeProgress01`/`secondsLeft` are a RAF countdown over the bet's
 * GTD window; `phase`/`filledQty`/`avgPriceNanos` come from the pure reducers.
 */
export function useDegenBetTracker(
  active: DegenActive | null,
): DegenTracking | null {
  const { data: rawEvents } = useAccountEvents(active?.accountId ?? null);
  const latestHeight = useStore(selectLatestHeight);

  const [timeProgress01, setTimeProgress01] = useState(0);
  const anchorRef = useRef<number | null>(null);
  const rafRef = useRef<number>(0);

  const submitHeight = active?.submitHeight ?? null;
  const expiresAtBlock = active?.expiresAtBlock ?? null;

  /* eslint-disable react-hooks/set-state-in-effect -- reset the countdown when the tracked bet's identity changes (new bet / cleared) */
  useEffect(() => {
    anchorRef.current = active ? performance.now() : null;
    setTimeProgress01(0);
  }, [active?.accountId, submitHeight, active?.marketId, active?.outcome]);
  /* eslint-enable react-hooks/set-state-in-effect */

  useEffect(() => {
    if (submitHeight === null || expiresAtBlock === null) return;
    const totalMs = Math.max(
      1,
      (expiresAtBlock - submitHeight) * BLOCK_INTERVAL_MS,
    );
    let last = 0;
    const step = (t: number) => {
      if (anchorRef.current !== null && t - last >= 100) {
        last = t;
        const elapsed = performance.now() - anchorRef.current;
        setTimeProgress01(Math.min(1, elapsed / totalMs));
      }
      rafRef.current = requestAnimationFrame(step);
    };
    rafRef.current = requestAnimationFrame(step);
    return () => cancelAnimationFrame(rafRef.current);
  }, [submitHeight, expiresAtBlock]);

  if (!active) return null;

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

  const boundId = findDegenOrderId(events, {
    marketId: active.marketId,
    outcome: active.outcome,
    submitHeight: active.submitHeight,
  });
  const ours = boundId === null ? [] : events.filter((e) => e.orderId === boundId);

  const state = resolveDegenBet({
    targetQty: active.targetQty,
    currentHeight: latestHeight ?? active.submitHeight,
    expiresAtBlock: active.expiresAtBlock,
    events: ours,
  });

  const totalMs = Math.max(
    1,
    (active.expiresAtBlock - active.submitHeight) * BLOCK_INTERVAL_MS,
  );
  const secondsLeft = Math.max(
    0,
    Math.ceil((totalMs * (1 - timeProgress01)) / 1000),
  );

  return { ...state, secondsLeft, timeProgress01 };
}
