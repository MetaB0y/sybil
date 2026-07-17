"use client";

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";
import { formatDollars, parseNanos } from "@/lib/format/nanos";

export type OnboardingPolicy =
  components["schemas"]["OnboardingPolicyResponse"];

export function useOnboardingPolicy() {
  return useQuery({
    queryKey: ["onboarding-policy"],
    queryFn: async (): Promise<OnboardingPolicy> => {
      const { data, error } = await api.GET("/v1/onboarding");
      if (error || !data) throw new Error("fetch /v1/onboarding failed");
      return data;
    },
    staleTime: 60_000,
    refetchOnWindowFocus: false,
  });
}

export function demoGrantCopy(grantNanos: string | undefined): string {
  if (grantNanos === undefined) {
    return "Sybil assigns the same fixed play-money grant to everyone.";
  }
  if (parseNanos(grantNanos) === 0n) {
    return "New demo accounts currently start with $0.";
  }
  return `Every new demo account receives ${formatDollars(grantNanos, {
    decimals: 0,
  })} in play money.`;
}
