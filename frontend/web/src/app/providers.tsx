"use client";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import ReactDOM from "react-dom";
import { AccountProvider } from "@/lib/account/provider";
import { RealtimeProvider } from "@/lib/ws/realtime-provider";
import { ThemeProvider } from "@/lib/theme/provider";

const DEFAULT_API_BASE = "https://172-104-31-54.nip.io";
const API_ORIGIN = apiOrigin(
  process.env.NEXT_PUBLIC_API_BASE ?? DEFAULT_API_BASE,
);

function apiOrigin(apiBase: string): string | null {
  try {
    const origin = new URL(apiBase).origin;
    return origin === "null" ? null : origin;
  } catch {
    return null;
  }
}

export function Providers({ children }: { children: ReactNode }) {
  // Next 16's Metadata API delegates resource hints to ReactDOM. Providers is
  // a client component but is server-rendered, so the hints land in the initial
  // response before hydration discovers the markets API requests.
  if (API_ORIGIN) {
    ReactDOM.preconnect(API_ORIGIN, { crossOrigin: "anonymous" });
    ReactDOM.prefetchDNS(API_ORIGIN);
  }

  const [queryClient] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            staleTime: 30_000,
            refetchOnWindowFocus: false,
          },
        },
      }),
  );

  return (
    <ThemeProvider>
      <QueryClientProvider client={queryClient}>
        <AccountProvider>
          <RealtimeProvider>{children}</RealtimeProvider>
        </AccountProvider>
      </QueryClientProvider>
    </ThemeProvider>
  );
}
