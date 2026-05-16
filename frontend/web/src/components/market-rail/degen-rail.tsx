"use client";

/**
 * Degen rail — "tap & win" betting flow. Banner → outcome picker → yes/no
 * → amount → big CTA → FBA explainer. Matches `DegenRail` in
 * `fed-right-rail-modes.jsx:293`.
 *
 * The CTA is disabled until wallet/auth lands. All numbers shown are real
 * (countdown, prices, payout math); the only mocked value is "N traders
 * joined" inside the banner.
 */

import { useState } from "react";
import type { EventGroup } from "@/lib/market-detail/use-event-group";
import { DegenAmount } from "./degen-amount";
import { DegenOutcomePicker } from "./degen-outcome-picker";
import { NextBatchBanner } from "./next-batch-banner";
import type { Side } from "./yes-no-toggle";
import { YesNoToggle } from "./yes-no-toggle";
import { WhyWaiting } from "./why-waiting";

export function DegenRail({ group }: { group: EventGroup }) {
  const [side, setSide] = useState<Side>("YES");
  const [amount, setAmount] = useState<string>("100");

  const selected =
    group.outcomes.find((o) => o.marketId === group.currentMarketId) ??
    group.outcomes[0];
  if (!selected) return null;
  const yesCents = selected.yesCents;
  const amountNum = parseFloat(amount) || 0;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <NextBatchBanner marketId={selected.marketId} />

      {group.isMultiOutcome && (
        <div>
          <SectionLabel>pick outcome</SectionLabel>
          <DegenOutcomePicker
            outcomes={group.outcomes}
            currentMarketId={group.currentMarketId}
          />
        </div>
      )}

      <div>
        <SectionLabel>will it happen?</SectionLabel>
        <YesNoToggle value={side} onChange={setSide} />
      </div>

      <div>
        <SectionLabel>your bet</SectionLabel>
        <DegenAmount
          amount={amount}
          setAmount={setAmount}
          yesPriceCents={yesCents}
          side={side}
        />
      </div>

      <button
        type="button"
        disabled
        title="preview · wallet auth coming soon"
        style={{
          marginTop: 4,
          padding: "16px 0",
          borderRadius: 6,
          border: 0,
          cursor: "not-allowed",
          background: side === "YES" ? "var(--yes)" : "var(--no)",
          color: "#0A0E12",
          fontFamily: "var(--font-sans)",
          fontSize: 15,
          fontWeight: 700,
          letterSpacing: "-0.005em",
          opacity: 0.65,
        }}
      >
        Bet ${amountNum} on {side}
        {group.isMultiOutcome ? ` · ${truncate(selected.label, 28)}` : ""}
      </button>
      <div
        style={{
          marginTop: -6,
          fontFamily: "var(--font-mono)",
          fontSize: 9.5,
          color: "var(--fg-4)",
          textAlign: "center",
          textTransform: "uppercase",
          letterSpacing: "0.05em",
        }}
      >
        preview · wallet auth coming soon
      </div>

      <WhyWaiting />
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

function truncate(s: string, max: number): string {
  if (s.length <= max) return s;
  return s.slice(0, max - 1).trimEnd() + "…";
}
