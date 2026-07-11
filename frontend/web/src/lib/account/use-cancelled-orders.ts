"use client";

/**
 * localStorage-backed immediate overlay for orders this browser cancelled.
 * The backend emits a committed `OrderCancelled` event in the next block;
 * this cache lets interaction-specific UI settle before that event arrives.
 *
 * Bounded ring of the last 100 cancels per account. Reactivity comes from
 * a tiny Zustand store; mutations flush to localStorage on every write.
 */

import { useEffect, useMemo } from "react";
import { create } from "zustand";

export interface TrackedCancel {
  accountId: number;
  orderId: number;
  marketId: number;
  side: string; // e.g. "BuyYes" — server casing varies, we keep raw
  qty: number;
  limitPriceNanos: string;
  timestampMs: number;
}

const STORAGE_KEY = "sybil:auth:cancelled_orders";
const CAP = 100;

interface CancelStore {
  cancels: TrackedCancel[];
  hydrated: boolean;
  hydrate: () => void;
  record: (c: TrackedCancel) => void;
}

const useCancelStore = create<CancelStore>((set, get) => ({
  cancels: [],
  hydrated: false,
  hydrate: () => {
    if (typeof window === "undefined") return;
    if (get().hydrated) return;
    try {
      const raw = window.localStorage.getItem(STORAGE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw) as TrackedCancel[];
        if (Array.isArray(parsed)) {
          set({ cancels: parsed, hydrated: true });
          return;
        }
      }
    } catch {
      // fall through
    }
    set({ hydrated: true });
  },
  record: (c) =>
    set((s) => {
      const next = [c, ...s.cancels].slice(0, CAP);
      try {
        window.localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
      } catch {
        // ignore quota errors
      }
      return { cancels: next };
    }),
}));

export function recordCancel(c: TrackedCancel) {
  useCancelStore.getState().record(c);
}

/** Hook: cancels for one account, newest first. Auto-hydrates on mount. */
export function useTrackedCancels(accountId: number | null): TrackedCancel[] {
  const all = useCancelStore((s) => s.cancels);
  const hydrate = useCancelStore((s) => s.hydrate);

  useEffect(() => {
    hydrate();
  }, [hydrate]);

  return useMemo(() => {
    if (accountId === null) return [];
    return all.filter((c) => c.accountId === accountId);
  }, [all, accountId]);
}
