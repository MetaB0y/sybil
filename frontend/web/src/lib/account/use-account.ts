"use client";

/**
 * Selector hooks for account session state.
 *
 * Components should prefer these narrow hooks over subscribing to the whole
 * store — Zustand re-renders only when the selected slice changes.
 */

import { useAccountStore } from "./store";

export function useAccountSession() {
  return useAccountStore((s) => s.session);
}

export function useAccountHydrated() {
  return useAccountStore((s) => s.hydrated);
}

export function useConnectModalOpen() {
  return useAccountStore((s) => s.connectModalOpen);
}

export function useSetConnectModalOpen() {
  return useAccountStore((s) => s.setConnectModalOpen);
}
