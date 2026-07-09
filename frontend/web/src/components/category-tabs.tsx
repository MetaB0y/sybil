"use client";

/**
 * CategoryTabs — text-link tabs that filter the markets index by category.
 *
 * Drives the `?category=` URL param; the page reads it and filters by
 * pickDisplayCategory().primary (primary-only semantics — a market only
 * appears under the tab matching its chip).
 *
 * Tab order is independent of categorize.ts priority. Priority controls
 * which chip wins on a multi-category market; tab order controls UX. The
 * categories that actually hold live markets (AI / Finance today) lead the
 * list; the rest render faded so it's clear they're currently empty. The
 * "active" set is derived from the live bundle, so a category un-fades on its
 * own once markets land there.
 */

import { useCallback, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useMarketsList } from "@/lib/markets/use-markets";
import { buildIndexCards } from "@/lib/markets/build-index-cards";

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
  "Weather",
  "Mentions",
] as const;

export function CategoryTabs() {
  const searchParams = useSearchParams();
  const router = useRouter();
  const pathname = usePathname();
  const active = searchParams.get("category") ?? "All";
  // A "soon" tooltip floats just above the cursor while hovering a faded
  // (inactive) category. `pos` is the live cursor position in viewport coords.
  const [hovered, setHovered] = useState<string | null>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);

  // Which categories have at least one open market right now. Reads the shared,
  // already-cached markets query (react-query dedupes — no extra fetch).
  const { bundle } = useMarketsList();
  const populated = useMemo(() => {
    const set = new Set<string>();
    if (bundle) {
      for (const card of buildIndexCards(bundle)) {
        if (!card.closed && card.primaryCategory) set.add(card.primaryCategory);
      }
    }
    return set;
  }, [bundle]);

  const select = useCallback(
    (cat: string) => {
      const params = new URLSearchParams(searchParams.toString());
      if (cat === "All") params.delete("category");
      else params.set("category", cat);
      const qs = params.toString();
      router.replace(qs ? `${pathname}?${qs}` : pathname, { scroll: false });
    },
    [pathname, router, searchParams]
  );

  return (
    <>
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
          // "All" and any category with live markets read at full strength;
          // empty categories fade and are fully non-interactive — they can't be
          // selected. Hovering one floats a "soon" tooltip near the cursor.
          const populatedTab = cat === "All" || populated.has(cat);
          const faded = !populatedTab && !isActive;
          const trackCursor = (e: React.MouseEvent) =>
            setPos({ x: e.clientX, y: e.clientY });
          return (
            <button
              key={cat}
              type="button"
              onClick={faded ? undefined : () => select(cat)}
              onMouseEnter={
                faded
                  ? (e) => {
                      setHovered(cat);
                      trackCursor(e);
                    }
                  : undefined
              }
              onMouseMove={faded ? trackCursor : undefined}
              onMouseLeave={
                faded
                  ? () => {
                      setHovered((h) => (h === cat ? null : h));
                      setPos(null);
                    }
                  : undefined
              }
              aria-disabled={faded || undefined}
              tabIndex={faded ? -1 : undefined}
              style={{
                position: "relative",
                flexShrink: 0,
                background: "transparent",
                border: 0,
                padding: "var(--space-2) 0",
                fontFamily: "var(--font-sans)",
                fontSize: "var(--fs-14)",
                fontWeight: 500,
                color: isActive
                  ? "var(--fg-1)"
                  : faded
                    ? hovered === cat
                      ? "var(--fg-3)"
                      : "var(--fg-4)"
                    : "var(--fg-3)",
                opacity: faded ? (hovered === cat ? 0.85 : 0.5) : 1,
                cursor: faded ? "default" : "pointer",
                transition:
                  "color var(--dur-fast) var(--ease-standard), opacity var(--dur-fast) var(--ease-standard)",
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
      {hovered && pos && typeof document !== "undefined"
        ? createPortal(<SoonTooltip x={pos.x} y={pos.y} />, document.body)
        : null}
    </>
  );
}

/** Floating "soon" hint anchored just above the cursor. */
function SoonTooltip({ x, y }: { x: number; y: number }) {
  return (
    <div
      role="tooltip"
      aria-hidden
      style={{
        position: "fixed",
        left: x,
        top: y - 14,
        transform: "translate(-50%, -100%)",
        pointerEvents: "none",
        zIndex: 100,
        padding: "3px 7px",
        background: "var(--surface-2)",
        border: "1px solid var(--border-2)",
        borderRadius: "var(--radius-sm)",
        boxShadow: "0 6px 18px rgba(0,0,0,0.35)",
        fontFamily: "var(--font-mono)",
        fontSize: "9px",
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
        color: "var(--accent)",
        whiteSpace: "nowrap",
        animation: "sybil-tooltip-in var(--dur-fast) var(--ease-standard)",
      }}
    >
      soon
    </div>
  );
}
