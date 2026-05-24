"use client";

/**
 * Degen rail — "tap & win" betting flow. Banner → outcome picker → yes/no →
 * amount → CTA. On submit the form area is replaced inline by a live
 * fill-progress card and then a result (DegenProgress). One bet at a time.
 */

import { useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { submitSignedOrder } from "@/lib/account/orders";
import { humanizeOrderError } from "@/lib/account/order-errors";
import { useAccountSession, useSetConnectModalOpen } from "@/lib/account/use-account";
import { usePortfolio } from "@/lib/account/use-portfolio";
import { ONE_DOLLAR_NANOS, buildDegenOrder, resolveMarkNanos } from "@/lib/degen";
import {
  useDegenBetTracker,
  type DegenActive,
} from "@/lib/degen/use-degen-bet-tracker";
import { parseNanos } from "@/lib/format/nanos";
import type { EventGroup } from "@/lib/market-detail/use-event-group";
import { usePriceHistory } from "@/lib/markets/use-price-history";
import { selectLatestHeight, useStore } from "@/lib/store";
import { DegenAmount } from "./degen-amount";
import { DegenOutcomePicker } from "./degen-outcome-picker";
import { DegenProgress } from "./degen-progress";
import { NextBatchBanner } from "./next-batch-banner";
import type { Side } from "./yes-no-toggle";
import { YesNoToggle } from "./yes-no-toggle";
import { WhyWaiting } from "./why-waiting";

export function DegenRail({ group }: { group: EventGroup }) {
  const [side, setSide] = useState<Side>("YES");
  const [amount, setAmount] = useState<string>("10");
  const [active, setActive] = useState<DegenActive | null>(null);
  const [signing, setSigning] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const session = useAccountSession();
  const openConnectModal = useSetConnectModalOpen();
  const qc = useQueryClient();
  const latestHeight = useStore(selectLatestHeight);
  const portfolio = usePortfolio(session?.accountId ?? null);
  const balanceDollars = portfolio.data
    ? Number(parseNanos(portfolio.data.balance_nanos)) / 1e9
    : null;

  const selected =
    group.outcomes.find((o) => o.marketId === group.currentMarketId) ??
    group.outcomes[0];

  const { data: pricePoints } = usePriceHistory(selected?.marketId ?? -1);
  const tracking = useDegenBetTracker(active);

  // mark price for the selected side: last price-history point, else clearing.
  const markNanos = useMemo(() => {
    if (!selected) return ONE_DOLLAR_NANOS / 2n;
    const last = pricePoints?.[pricePoints.length - 1];
    const histYes = last ? parseNanos(last.yes_price_nanos) : null;
    const histNo = last ? parseNanos(last.no_price_nanos) : null;
    const clearYes =
      selected.yesCents == null
        ? null
        : BigInt(Math.round(selected.yesCents * 1e7));
    const clearNo = clearYes == null ? null : ONE_DOLLAR_NANOS - clearYes;
    return side === "YES"
      ? resolveMarkNanos(histYes, clearYes)
      : resolveMarkNanos(histNo, clearNo);
  }, [pricePoints, selected, side]);

  const amountNum = parseFloat(amount) || 0;
  const built = useMemo(() => {
    const betUsdNanos = BigInt(Math.round(amountNum * 1e9));
    return buildDegenOrder({
      side,
      betUsdNanos,
      markNanos,
      latestHeight: BigInt(latestHeight ?? 0),
    });
  }, [amountNum, side, markNanos, latestHeight]);

  if (!selected) return null;

  async function onBet() {
    if (!session) {
      openConnectModal(true);
      return;
    }
    // selected is narrowed to non-undefined by the early return above, but
    // TypeScript can't see across the function boundary — guard here too.
    if (!selected || !built.ok || latestHeight == null) return;
    setSigning(true);
    setSubmitError(null);
    try {
      const res = await submitSignedOrder({
        accountId: session.accountId,
        publicKeyHex: session.publicKeyHex,
        marketId: selected.marketId,
        side: built.order.side,
        limitPriceNanos: built.order.limitPriceNanos,
        maxFill: built.order.maxFill,
        expiresAtBlock: built.order.expiresAtBlock,
      });
      if (!res.accepted) throw new Error("order not accepted");
      setActive({
        accountId: session.accountId,
        marketId: selected.marketId,
        outcome: side,
        targetQty: built.order.maxFill,
        limitPriceNanos: built.order.limitPriceNanos,
        submitHeight: latestHeight,
        // DegenActive.expiresAtBlock is number; built.order.expiresAtBlock is bigint.
        expiresAtBlock: Number(built.order.expiresAtBlock),
      });
      qc.invalidateQueries({ queryKey: ["account", session.accountId, "events"] });
      qc.invalidateQueries({ queryKey: ["account", session.accountId, "orders"] });
      qc.invalidateQueries({ queryKey: ["account", session.accountId, "portfolio"] });
      qc.invalidateQueries({ queryKey: ["orders", "pending"] });
    } catch (e) {
      console.error("degen bet submit failed:", e);
      setSubmitError(humanizeOrderError(e, "bet"));
    } finally {
      setSigning(false);
    }
  }

  const connected = session !== null;
  const ctaLabel = !connected
    ? "Connect to bet"
    : signing
      ? "Signing…"
      : !built.ok
        ? "Raise your bet"
        : `Bet $${amountNum} on ${side}${group.isMultiOutcome ? ` · ${selected.shortLabel}` : ""}`;
  const ctaDisabled = connected && (signing || !built.ok);

  // Bottom explainer: "why am I waiting?" while placing or before a bet,
  // "why failed?" after a missed bet, and nothing once a bet lands (the result
  // card already explains itself).
  const resultPhase = active ? tracking?.phase ?? "tracking" : null;
  const showExplainer = resultPhase !== "filled" && resultPhase !== "partial";
  const explainerVariant = resultPhase === "none" ? "failed" : "waiting";

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <NextBatchBanner marketId={selected.marketId} />

      {active ? (
        <DegenProgress
          phase={tracking?.phase ?? "tracking"}
          side={active.outcome}
          secondsLeft={tracking?.secondsLeft ?? 0}
          timeProgress01={tracking?.timeProgress01 ?? 0}
          filledQty={tracking?.filledQty ?? 0n}
          targetQty={active.targetQty}
          betUsd={amountNum}
          onBetAgain={() => setActive(null)}
        />
      ) : (
        <>
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
              maxFill={built.ok ? built.order.maxFill : null}
              balanceDollars={balanceDollars}
            />
          </div>

          <button
            type="button"
            onClick={onBet}
            disabled={ctaDisabled}
            style={{
              marginTop: 4,
              padding: "16px 0",
              borderRadius: 6,
              border: 0,
              cursor: ctaDisabled ? "not-allowed" : "pointer",
              background: side === "YES" ? "var(--yes)" : "var(--no)",
              color: "#0A0E12",
              fontFamily: "var(--font-sans)",
              fontSize: 15,
              fontWeight: 700,
              letterSpacing: "-0.005em",
              opacity: ctaDisabled ? 0.65 : 1,
            }}
          >
            {ctaLabel}
          </button>

          {submitError && (
            <div
              role="alert"
              style={{
                fontFamily: "var(--font-mono)",
                fontSize: 11,
                color: "var(--no)",
                textAlign: "center",
              }}
            >
              {submitError}
            </div>
          )}
        </>
      )}

      {showExplainer && <WhyWaiting variant={explainerVariant} />}
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
