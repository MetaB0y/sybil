"use client";

/**
 * Account session state.
 *
 * Two split pieces:
 *  1. **Serializable** (in Zustand): accountId + publicKeyHex + UI flags.
 *     Safe for DevTools/persist middleware to peek at.
 *  2. **Non-serializable** (module-level `Map<accountId, CryptoKey>`): the
 *     WebCrypto private key handle. CryptoKey can't be cloned/serialized,
 *     so it lives outside store state. Mirrors the WS singleton pattern.
 */

import { create } from "zustand";

export interface AccountSession {
  accountId: number;
  publicKeyHex: string;
}

interface AccountStore {
  /** `null` until the user connects or imports an account. */
  session: AccountSession | null;
  /** `false` until the AccountProvider has run its localStorage hydration. */
  hydrated: boolean;
  /** Whether the connect modal is currently open. */
  connectModalOpen: boolean;

  setSession: (s: AccountSession | null) => void;
  setHydrated: (h: boolean) => void;
  setConnectModalOpen: (open: boolean) => void;
}

export const useAccountStore = create<AccountStore>((set) => ({
  session: null,
  hydrated: false,
  connectModalOpen: false,
  setSession: (session) => set({ session }),
  setHydrated: (hydrated) => set({ hydrated }),
  setConnectModalOpen: (connectModalOpen) => set({ connectModalOpen }),
}));

// --- non-serializable key handle registry --------------------------------

const KEY_HANDLES = new Map<number, CryptoKey>();

export function setKeyHandle(accountId: number, key: CryptoKey): void {
  KEY_HANDLES.set(accountId, key);
}

export function getKeyHandle(accountId: number): CryptoKey | null {
  return KEY_HANDLES.get(accountId) ?? null;
}

export function clearKeyHandle(accountId: number): void {
  KEY_HANDLES.delete(accountId);
}

export function clearAllKeyHandles(): void {
  KEY_HANDLES.clear();
}
