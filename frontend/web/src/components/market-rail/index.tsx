"use client";

/**
 * MarketRail — entry point for the /m/[id] right rail. Renders the Degen/Pro
 * mode tabs and dispatches to the appropriate rail. Replaces the old
 * `BatchTheater` component in this slot.
 *
 * The container is sticky so the rail stays in view as the user scrolls
 * the long left column (chart + description + rules + discussion).
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
        position: "sticky",
        top: 72,
        alignSelf: "start",
        display: "flex",
        flexDirection: "column",
        gap: 14,
        // Cap the height so internal scrolling can engage if the rail ever
        // grows past the viewport (Pro mode + multi-outcome + open disclosure).
        maxHeight: "calc(100vh - 88px)",
        overflowY: "auto",
        paddingRight: 2,
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
