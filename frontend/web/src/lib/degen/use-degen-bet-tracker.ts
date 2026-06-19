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
  /** `performance.now()` captured at submit. The countdown anchors to this
   *  (persisted in the lifted DegenActive) rather than to a mount timestamp, so
   *  it reflects true elapsed time and doesn't restart — or over-run — when the
   *  rail unmounts/remounts on a Degen↔Pro toggle. */
  submitPerfMs: number;
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

  const submitHeight = active?.submitHeight ?? null;
  const expiresAtBlock = active?.expiresAtBlock ?? null;
  const submitPerfMs = active?.submitPerfMs ?? null;

  // Wall-clock GTD window (ms) for the bet.
  const windowMs =
    submitHeight === null || expiresAtBlock === null
      ? null
      : Math.max(1, (expiresAtBlock - submitHeight) * BLOCK_INTERVAL_MS);

  // Progress over that window, anchored to the PERSISTED submit time (carried in
  // DegenActive), not a per-mount timestamp. So a Degen↔Pro toggle — which
  // unmounts this hook — resumes at the true elapsed point and ends when the
  // order actually expires, instead of restarting from 0 over a fresh full
  // window. Start at 0 and let the first RAF tick fill in the real value within
  // ~one frame, mirroring useBatchCountdown (which fixed the same remount bug).
  const [timeProgress01, setTimeProgress01] = useState(0);
  const rafRef = useRef<number>(0);

  useEffect(() => {
    if (submitPerfMs === null || windowMs === null) return;
    let last = 0;
    const step = (t: number) => {
      if (t - last >= 100) {
        last = t;
        const elapsed = performance.now() - submitPerfMs;
        setTimeProgress01(Math.min(1, Math.max(0, elapsed / windowMs)));
      }
      rafRef.current = requestAnimationFrame(step);
    };
    rafRef.current = requestAnimationFrame(step);
    return () => cancelAnimationFrame(rafRef.current);
  }, [submitPerfMs, windowMs]);

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
