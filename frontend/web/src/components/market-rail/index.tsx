"use client";

/**
 * MarketRail — entry point for the /m/[id] right rail. Renders the Degen/Pro
 * mode tabs and dispatches to the appropriate rail. Replaces the old
 * `BatchTheater` component in this slot.
 *
 * The rail is a plain column that scrolls with the rest of the page — it is
 * NOT a sticky/independently-scrolling panel.
 */

import { useState } from "react";
import type { DegenActive } from "@/lib/degen/use-degen-bet-tracker";
import { useEventGroup } from "@/lib/market-detail/use-event-group";
import { DegenRail } from "./degen-rail";
import { ModeTabs } from "./mode-tabs";
import { ProRail } from "./pro-rail";
import { useRailMode } from "./use-rail-mode";

export function MarketRail({ marketId }: { marketId: number }) {
  const [mode, setMode] = useRailMode();
  const { group, isPending } = useEventGroup(marketId);
  // Held here, not inside DegenRail, so an in-flight bet's status survives a
  // Degen↔Pro toggle (which unmounts the rail body below).
  const [degenActive, setDegenActive] = useState<DegenActive | null>(null);

  const selected = group
    ? group.outcomes.find((o) => o.marketId === group.currentMarketId) ??
      group.outcomes[0]
    : undefined;
  const closed = selected?.closed === true;

  return (
    <aside
      className="no-scrollbar market-rail-responsive"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 14,
        // The rail is its own scroll column (the grid row gives it a bounded
        // height), so the user can scroll down to the bet CTA without moving
        // the left column at all.
        minHeight: 0,
        overflowY: "auto",
        overscrollBehavior: "contain",
        paddingBottom: "var(--space-5)",
      }}
    >
      {!closed && <ModeTabs value={mode} onChange={setMode} />}
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
      {group && closed && <ClosedRail />}
      {group && !closed && mode === "degen" && (
        <DegenRail
          group={group}
          active={degenActive}
          setActive={setDegenActive}
        />
      )}
      {group && !closed && mode === "pro" && <ProRail group={group} />}
    </aside>
  );
}

/** Read-only replacement for the trade rail on a closed/resolved market. */
function ClosedRail() {
  return (
    <div
      className="text-mono"
      style={{
        padding: "24px 16px",
        borderRadius: "var(--radius-md)",
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        color: "var(--fg-3)",
        fontSize: 12,
        lineHeight: "18px",
        textAlign: "center",
      }}
    >
      This market has closed. Trading is disabled.
    </div>
  );
}
