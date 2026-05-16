"use client";

/**
 * MarketRail — entry point for the /m/[id] right rail. Renders the Degen/Pro
 * mode tabs and dispatches to the appropriate rail. Replaces the old
 * `BatchTheater` component in this slot.
 *
 * The rail is a plain column that scrolls with the rest of the page — it is
 * NOT a sticky/independently-scrolling panel. Matches `V2BatchTheater` in
 * `frontend/handoff/data/fed-variations.jsx:99`.
 */

import { useEventGroup } from "@/lib/market-detail/use-event-group";
import { DegenRail } from "./degen-rail";
import { ModeTabs } from "./mode-tabs";
import { ProRail } from "./pro-rail";
import { useRailMode } from "./use-rail-mode";

export function MarketRail({ marketId }: { marketId: number }) {
  const [mode, setMode] = useRailMode();
  const { group, isPending } = useEventGroup(marketId);

  return (
    <aside
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 14,
      }}
    >
      <ModeTabs value={mode} onChange={setMode} />
      {isPending && (
        <div
          style={{
            padding: "24px 12px",
            color: "var(--fg-3)",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            textAlign: "center",
          }}
        >
          loading rail…
        </div>
      )}
      {group && mode === "degen" && <DegenRail group={group} />}
      {group && mode === "pro" && <ProRail group={group} />}
    </aside>
  );
}
