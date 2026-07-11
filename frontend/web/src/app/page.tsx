import MarketsPageClient from "./markets-page-client";
import type { Market } from "@/lib/markets/use-markets";

const DEFAULT_API_BASE = "https://172-104-31-54.nip.io";
const INITIAL_MARKETS_TIMEOUT_MS = 2_500;

/**
 * Put the first market list into the server-rendered HTML so the index does not
 * wait for hydration before discovering its LCP content. This is deliberately
 * best-effort: a slow or unavailable API must not turn a recoverable client
 * loading/error state into a failed page request.
 */
async function getInitialMarkets(): Promise<Market[] | undefined> {
  const apiBase = process.env.NEXT_PUBLIC_API_BASE ?? DEFAULT_API_BASE;

  try {
    const response = await fetch(`${apiBase.replace(/\/$/, "")}/v1/markets`, {
      cache: "no-store",
      signal: AbortSignal.timeout(INITIAL_MARKETS_TIMEOUT_MS),
    });
    if (!response.ok) return undefined;
    const body: unknown = await response.json();
    return Array.isArray(body) ? (body as Market[]) : undefined;
  } catch {
    return undefined;
  }
}

export default async function MarketsPage() {
  const initialMarkets = await getInitialMarkets();
  return <MarketsPageClient initialMarkets={initialMarkets} />;
}
