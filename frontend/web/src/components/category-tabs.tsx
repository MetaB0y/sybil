"use client";

/**
 * CategoryTabs — text-link tabs that filter the markets index by category.
 *
 * Drives the `?category=` URL param; the page reads it and filters by
 * pickDisplayCategory().primary (primary-only semantics — a market only
 * appears under the tab matching its chip).
 *
 * Tab order is independent of categorize.ts priority. Priority controls
 * which chip wins on a multi-category market; tab order controls UX.
 */

import { useCallback } from "react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";

const CATEGORIES = [
  "All",
  "AI",
  "Finance",
  "Politics",
  "Geopolitics",
  "Elections",
  "Tech",
  "Economy",
  "Culture",
  "Science",
  "World",
  "Business",
  "Weather",
  "Mentions",
  "Sports",
  "Crypto",
  "Commodities",
] as const;

export function CategoryTabs() {
  const searchParams = useSearchParams();
  const router = useRouter();
  const pathname = usePathname();
  const active = searchParams.get("category") ?? "All";

  const select = useCallback(
    (cat: string) => {
      const params = new URLSearchParams(searchParams.toString());
      if (cat === "All") params.delete("category");
      else params.set("category", cat);
      const qs = params.toString();
      router.replace(qs ? `${pathname}?${qs}` : pathname, { scroll: false });
    },
    [pathname, router, searchParams],
  );

  return (
    <nav
      aria-label="market categories"
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-4)",
        minWidth: 0,
        flex: "1 1 auto",
        overflowX: "auto",
        scrollbarWidth: "none",
      }}
    >
      {CATEGORIES.map((cat) => {
        const isActive = cat === active;
        return (
          <button
            key={cat}
            type="button"
            aria-pressed={isActive}
            onClick={() => select(cat)}
            onMouseEnter={(event) => {
              if (!isActive) event.currentTarget.style.color = "var(--fg-2)";
            }}
            onMouseLeave={(event) => {
              if (!isActive) event.currentTarget.style.color = "var(--fg-3)";
            }}
            style={{
              position: "relative",
              flexShrink: 0,
              background: "transparent",
              border: 0,
              padding: "var(--space-2) 0",
              fontFamily: "var(--font-sans)",
              fontSize: "var(--fs-14)",
              fontWeight: 500,
              color: isActive ? "var(--fg-1)" : "var(--fg-3)",
              cursor: "pointer",
              transition: "color var(--dur-fast) var(--ease-standard)",
            }}
          >
            {cat}
            {isActive && (
              <span
                aria-hidden
                style={{
                  position: "absolute",
                  bottom: 0,
                  left: 0,
                  right: 0,
                  height: 2,
                  background: "var(--accent)",
                  borderRadius: 1,
                }}
              />
            )}
          </button>
        );
      })}
    </nav>
  );
}
