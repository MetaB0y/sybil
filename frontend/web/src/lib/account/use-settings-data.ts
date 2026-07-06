"use client";

/**
 * TanStack Query hooks backing the /settings page (SYB-60):
 *   - useAccountProfile  → GET /v1/accounts/{id}          (display_name, avatar_seed)
 *   - useSigningKeys     → GET /v1/accounts/{id}/keys     (signing / agent keys)
 *   - useReadApiKeys     → GET /v1/accounts/{id}/api-keys (read-only bearer keys)
 *
 * These are lower-churn than portfolio data (they only change on explicit
 * settings mutations), so they refetch on demand via query invalidation rather
 * than per-block.
 */

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";

export type AccountProfile = components["schemas"]["AccountResponse"];
export type SigningKey = components["schemas"]["AccountKeyResponse"];
export type ReadApiKey = components["schemas"]["ApiKeyResponse"];

export const settingsQueryKeys = {
  profile: (accountId: number) => ["account", accountId, "profile-meta"] as const,
  signingKeys: (accountId: number) => ["account", accountId, "keys"] as const,
  apiKeys: (accountId: number) => ["account", accountId, "api-keys"] as const,
};

export function useAccountProfile(accountId: number | null) {
  return useQuery({
    enabled: accountId !== null,
    queryKey: settingsQueryKeys.profile(accountId ?? -1),
    queryFn: async (): Promise<AccountProfile> => {
      if (accountId === null) throw new Error("no account");
      const { data, error } = await api.GET("/v1/accounts/{id}", {
        params: { path: { id: accountId } },
      });
      if (error || !data) throw new Error("fetch account failed");
      return data;
    },
    staleTime: 30_000,
    refetchOnWindowFocus: false,
  });
}

export function useSigningKeys(accountId: number | null) {
  return useQuery({
    enabled: accountId !== null,
    queryKey: settingsQueryKeys.signingKeys(accountId ?? -1),
    queryFn: async (): Promise<SigningKey[]> => {
      if (accountId === null) throw new Error("no account");
      const { data, error } = await api.GET("/v1/accounts/{id}/keys", {
        params: { path: { id: accountId } },
      });
      if (error || !data) throw new Error("fetch signing keys failed");
      return data;
    },
    staleTime: 30_000,
    refetchOnWindowFocus: false,
  });
}

export function useReadApiKeys(accountId: number | null) {
  return useQuery({
    enabled: accountId !== null,
    queryKey: settingsQueryKeys.apiKeys(accountId ?? -1),
    queryFn: async (): Promise<ReadApiKey[]> => {
      if (accountId === null) throw new Error("no account");
      const { data, error } = await api.GET("/v1/accounts/{id}/api-keys", {
        params: { path: { id: accountId } },
      });
      if (error || !data) throw new Error("fetch api keys failed");
      return data;
    },
    staleTime: 30_000,
    refetchOnWindowFocus: false,
  });
}
