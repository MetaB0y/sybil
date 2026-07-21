import MarketsPageClient from "./markets-page-client";

/**
 * Keep the index shell static and let the browser's canonical markets query
 * populate it after hydration. Passing the full catalog through an App Router
 * render made every request retain a large RSC tree in the long-lived
 * standalone process, eventually exhausting its V8 heap under repeated loads.
 */
export default function MarketsPage() {
  return <MarketsPageClient />;
}
