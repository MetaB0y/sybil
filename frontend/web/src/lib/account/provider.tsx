"use client";

/**
 * SSR-safe account state hydration. Mirrors `lib/ws/realtime-provider.tsx`:
 *   - server renders with `session: null` (constant initial Zustand state)
 *   - client mounts effect, reads localStorage, imports JWK, populates store
 *   - subscribes to `storage` events so other-tab logins/logouts apply here
 */

import { useEffect, type ReactNode } from "react";
import { invalidateReadSession, rehydrateFromStorage } from "./actions";
import { READ_AUTH_INVALID_EVENT } from "@/lib/api/client";
import { STORAGE_KEYS } from "./storage";
import { useAccountStore } from "./store";

export function AccountProvider({ children }: { children: ReactNode }) {
  useEffect(() => {
    let cancelled = false;
    (async () => {
      await rehydrateFromStorage();
      if (!cancelled) useAccountStore.getState().setHydrated(true);
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const interesting = new Set<string>([
      STORAGE_KEYS.ACCOUNT_ID,
      STORAGE_KEYS.PUBKEY,
      STORAGE_KEYS.AUTH_SCHEME,
      STORAGE_KEYS.JWK,
      STORAGE_KEYS.CREDENTIAL_ID,
      STORAGE_KEYS.READ_API_KEY,
      STORAGE_KEYS.REVISION,
    ]);
    function onStorage(e: StorageEvent) {
      if (e.key !== null && !interesting.has(e.key)) return;
      void rehydrateFromStorage();
    }
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  useEffect(() => {
    function onReadAuthInvalid() {
      invalidateReadSession();
    }
    window.addEventListener(READ_AUTH_INVALID_EVENT, onReadAuthInvalid);
    return () =>
      window.removeEventListener(READ_AUTH_INVALID_EVENT, onReadAuthInvalid);
  }, []);

  return <>{children}</>;
}
