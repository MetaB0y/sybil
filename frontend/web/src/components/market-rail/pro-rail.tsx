"use client";

/**
 * Pro rail — batch hero + outcome picker + order form + last-N disclosure.
 * Matches `ProRail` in `fed-variations.jsx:125`.
 *
 * The outcome picker is the same compact dropdown used by the Degen rail
 * (`DegenOutcomePicker`) — selected outcome shown big, the rest collapse
 * into a "switch outcome" dropdown.
 */

import type { EventGroup } from "@/lib/market-detail/use-event-group";
import { BatchHero } from "./batch-hero";
import { BuyBox } from "./buy-box";
import { DegenOutcomePicker } from "./degen-outcome-picker";
import { LastBatchesDisclosure } from "./last-batches-disclosure";

export function ProRail({ group }: { group: EventGroup }) {
  const selected =
    group.outcomes.find((o) => o.marketId === group.currentMarketId) ??
    group.outcomes[0];
  if (!selected) return null;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <BatchHero outcome={selected} />

      <div>
        <SectionLabel>pick an outcome</SectionLabel>
        <DegenOutcomePicker
          outcomes={group.outcomes}
          currentMarketId={group.currentMarketId}
        />
      </div>

      <BuyBox outcome={selected} />

      <LastBatchesDisclosure marketId={selected.marketId} />
    </div>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        color: "var(--fg-3)",
        textTransform: "uppercase",
        letterSpacing: "0.06em",
        marginBottom: 8,
      }}
    >
      {children}
    </div>
  );
}
