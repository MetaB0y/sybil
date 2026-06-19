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
import { useAvailableBalance } from "@/lib/account/use-available-balance";
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
import type { Side } from "./yes-no-toggle";
import { YesNoToggle } from "./yes-no-toggle";
import { WaitingAlert } from "./waiting-alert";
import { WhyWaiting } from "./why-waiting";

export function DegenRail({
  group,
  active,
  setActive,
}: {
  group: EventGroup;
  // Lifted into the (always-mounted) MarketRail so an in-flight bet survives a
  // toggle to Pro and back — this rail unmounts on mode switch.
  active: DegenActive | null;
  setActive: (a: DegenActive | null) => void;
}) {
  const [side, setSide] = useState<Side>("YES");
  const [amount, setAmount] = useState<string>("10");
  const [signing, setSigning] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const session = useAccountSession();
  const openConnectModal = useSetConnectModalOpen();
  const qc = useQueryClient();
  const latestHeight = useStore(selectLatestHeight);
  // "Available for betting" = balance minus cash reserved by resting buy orders
  // (the engine rejects against this, not raw balance). See useAvailableBalance.
  const { availableNanos, reservedNanos } = useAvailableBalance(
    session?.accountId ?? null,
  );
  const availableDollars =
    availableNanos == null ? null : Number(availableNanos) / 1e9;
  const reservedDollars = Number(reservedNanos) / 1e9;

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
        betUsd: amountNum,
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
  // Cash the engine would reserve for this bet (limit × shares). Block the CTA
  // when it exceeds what's available so we never trip a server-side
  // InsufficientBalance rejection.
  const requiredNanos = built.ok
    ? built.order.limitPriceNanos * built.order.maxFill
    : 0n;
  const insufficient =
    connected &&
    built.ok &&
    availableNanos != null &&
    requiredNanos > availableNanos;
  const ctaLabel = !connected
    ? "Connect to bet"
    : signing
      ? "Signing…"
      : !built.ok
        ? "Raise your bet"
        : insufficient
          ? "Not enough funds"
          : `Bet $${amountNum} on ${side}${group.isMultiOutcome ? ` · ${selected.shortLabel}` : ""}`;
  const ctaDisabled = connected && (signing || !built.ok || insufficient);

  // Explainer slot below the form/progress area:
  //  - while a bet is in flight ("tracking"): a compact WaitingAlert with the
  //    "why am I waiting?" copy tucked into an ⓘ tooltip;
  //  - after a missed bet ("none"): the short "why failed?" explainer;
  //  - pre-bet (null) and once a bet lands ("filled"/"partial"): nothing (the
  //    result card already explains itself).
  const resultPhase = active ? tracking?.phase ?? "tracking" : null;
  const showWaiting = resultPhase === "tracking";
  const showFailed = resultPhase === "none";

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      {active ? (
        <DegenProgress
          phase={tracking?.phase ?? "tracking"}
          side={active.outcome}
          secondsLeft={tracking?.secondsLeft ?? 0}
          timeProgress01={tracking?.timeProgress01 ?? 0}
          filledQty={tracking?.filledQty ?? 0n}
          targetQty={active.targetQty}
          betUsd={active.betUsd}
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
              availableDollars={availableDollars}
              reservedDollars={reservedDollars}
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

      {showWaiting && <WaitingAlert />}
      {showFailed && <WhyWaiting variant="failed" />}
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
