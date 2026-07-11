"use client";

import { getCategoryColor, pickDisplayCategory } from "@/lib/categorize";
import type { components } from "@/lib/api/schema";

type Market = components["schemas"]["MarketResponse"];

export function CategoryDot({
  market,
  size = 10,
}: {
  market: Market | undefined;
  size?: number;
}) {
  const pick = pickDisplayCategory(
    market?.categories ?? null,
    market?.category ?? null,
  );
  const color = pick.primary ? getCategoryColor(pick.primary) : "var(--fg-4)";
  return (
    <span
      aria-hidden
      style={{
        display: "inline-block",
        width: size,
        height: size,
        background: color,
        borderRadius: 2,
        flexShrink: 0,
      }}
    />
  );
}
