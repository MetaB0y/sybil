"use client";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { AccountProvider } from "@/lib/account/provider";
import { RealtimeProvider } from "@/lib/ws/realtime-provider";

export function Providers({ children }: { children: ReactNode }) {
  const [queryClient] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            staleTime: 30_000,
            refetchOnWindowFocus: false,
          },
        },
      })
  );

  return (
    <QueryClientProvider client={queryClient}>
      <AccountProvider>
        <RealtimeProvider>{children}</RealtimeProvider>
      </AccountProvider>
    </QueryClientProvider>
  );
}
