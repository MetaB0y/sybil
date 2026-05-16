"use client";

/**
 * Pro rail — batch hero + outcome list + (placeholder) order form + last-N
 * disclosure. Matches `ProRail` in `fed-variations.jsx:125`.
 *
 * The order form lands in phase 4 of the UX rework — for now we render a
 * placeholder card with the disabled-state banner copy.
 */

import type { EventGroup } from "@/lib/market-detail/use-event-group";
import { BatchHero } from "./batch-hero";
import { BuyBox } from "./buy-box";
import { LastBatchesDisclosure } from "./last-batches-disclosure";
import { OutcomeRadioList } from "./outcome-radio-list";

export function ProRail({ group }: { group: EventGroup }) {
  const selected =
    group.outcomes.find((o) => o.marketId === group.currentMarketId) ??
    group.outcomes[0];
  if (!selected) return null;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <BatchHero outcome={selected} />

      <OutcomeRadioList
        outcomes={group.outcomes}
        currentMarketId={group.currentMarketId}
      />

      <BuyBox outcome={selected} />

      <LastBatchesDisclosure marketId={selected.marketId} />
    </div>
  );
}
