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
import type { AccountAuthScheme } from "./storage";

export interface AccountSession {
  accountId: number;
  publicKeyHex: string;
  authScheme: AccountAuthScheme;
  credentialIdB64url?: string;
  readApiKey: string;
}

interface AccountStore {
  /** `null` until the user connects or imports an account. */
  session: AccountSession | null;
  /** `false` until the AccountProvider has run its localStorage hydration. */
  hydrated: boolean;
  /** Whether the connect modal is currently open. */
  connectModalOpen: boolean;
  /** True only when an invalid read token forced the modal open. */
  connectModalRecovery: boolean;

  setSession: (s: AccountSession | null) => void;
  setHydrated: (h: boolean) => void;
  setConnectModalOpen: (open: boolean) => void;
  openReadAuthRecovery: () => void;
}

export const useAccountStore = create<AccountStore>((set) => ({
  session: null,
  hydrated: false,
  connectModalOpen: false,
  connectModalRecovery: false,
  setSession: (session) => set({ session }),
  setHydrated: (hydrated) => set({ hydrated }),
  setConnectModalOpen: (connectModalOpen) =>
    set({ connectModalOpen, connectModalRecovery: false }),
  openReadAuthRecovery: () =>
    set({ connectModalOpen: true, connectModalRecovery: true }),
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

/** Remove a transient key only if it is still the handle installed by the
 * caller. A newer same-id session must survive an older async cleanup. */
export function clearKeyHandleIfMatches(
  accountId: number,
  expected: CryptoKey,
): void {
  if (KEY_HANDLES.get(accountId) === expected) KEY_HANDLES.delete(accountId);
}

export function clearAllKeyHandles(): void {
  KEY_HANDLES.clear();
}
