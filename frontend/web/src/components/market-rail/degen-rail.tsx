"use client";

/**
 * Degen rail — "tap & win" betting flow. Banner → outcome picker → yes/no →
 * amount → CTA. On submit the form area is replaced inline by a live
 * fill-progress card and then a result (DegenProgress). One bet at a time.
 */

import { useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import {
  completeSetReason,
  findCompleteSetBlockers,
} from "@/lib/account/complete-set";
import { cancelSignedOrder, submitSignedOrder } from "@/lib/account/orders";
import { humanizeOrderError } from "@/lib/account/order-errors";
import { notionalNanosCeil } from "@/lib/account/quantity";
import { useAccountSession, useSetConnectModalOpen } from "@/lib/account/use-account";
import type { AccountEvent } from "@/lib/account/use-account-events";
import { useAccountOrders } from "@/lib/account/use-account-orders";
import { useAvailableBalance } from "@/lib/account/use-available-balance";
import { useGroupMarkets } from "@/lib/markets/use-market-groups";
import { ONE_DOLLAR_NANOS, buildDegenOrder, resolveMarkNanos } from "@/lib/degen";
import { priorMaxOrderId } from "@/lib/degen/track";
import {
  useDegenBetTracker,
  type DegenActive,
} from "@/lib/degen/use-degen-bet-tracker";
import type { PendingOrder } from "@/lib/markets/use-pending-orders";
import { ResearchNudge } from "./research-nudge";
import { parseNanos } from "@/lib/format/nanos";
import type { EventGroup } from "@/lib/market-detail/use-event-group";
import { usePriceHistory } from "@/lib/markets/use-price-history";
import {
  selectLatestBlockAnchor,
  selectLatestHeight,
  useStore,
} from "@/lib/store";
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
  const [cancelling, setCancelling] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  // Optimistic cancel: hitting Cancel is a local intent, so we reflect it
  // immediately instead of waiting for the order to drop out of the live feeds
  // and the cancel to round-trip a block. Scope it to the active bet so a new
  // bet naturally starts uncancelled without a reset effect.
  const [optimisticallyCancelledBetKey, setOptimisticallyCancelledBetKey] =
    useState<string | null>(null);
  const activeBetKey = active ? degenActiveKey(active) : null;
  const cancelledLocal =
    activeBetKey !== null && optimisticallyCancelledBetKey === activeBetKey;

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

  // Complete-set preflight inputs. Group membership is NegRisk-only, so it must
  // come from /v1/markets/groups — an event's siblings are a superset.
  const { data: openOrders } = useAccountOrders(session?.accountId ?? null);
  const groupMarkets = useGroupMarkets(selected?.marketId ?? null);

  const { data: pricePoints } = usePriceHistory(selected?.marketId ?? -1);
  const tracking = useDegenBetTracker(active);

  // mark price for the selected side: last price-history point, else clearing.
  const markNanos = useMemo(() => {
    if (!selected) return ONE_DOLLAR_NANOS / 2n;
    const last = pricePoints?.[pricePoints.length - 1];
    const histYes = last ? parseNanos(last.yes_price_nanos) : null;
    const histNo = last ? parseNanos(last.no_price_nanos) : null;
    // Precise YES price in nanos (not the rounded `yesCents`) so the fallback
    // mark matches the real clearing price to sub-cent precision.
    const clearYes = selected.yesPriceNanos;
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
    // Freeze the open batch's block-clock anchor so the progress card shows ONE
    // countdown to the next batch clear (the live current-batch timeline).
    // Carried in DegenActive so it survives a Degen↔Pro toggle.
    const batchAnchorPerfMs =
      selectLatestBlockAnchor(useStore.getState()) ?? performance.now();
    // The actual submit instant — start of the bet's lifetime. `batchAnchorPerfMs`
    // is the batch *start* (up to ~10s earlier); the bar spans submit→expiry, so a
    // late-in-batch bet shows its true shorter remaining time from an empty bar.
    const submitPerfMs = performance.now();
    // Floor that isolates this bet from the account's earlier orders on this
    // market: the highest order id already in the cached events/pending feeds.
    // The new order's id is strictly greater (ids are monotonic per market), so
    // the tracker binds *this* bet and never re-reads a prior, already-resolved
    // order. Without it, a repeat bet on the same market+side instantly read the
    // previous order's "Successfully bet…"/"failed" state.
    const floorOrderId = priorMaxOrderId(
      selected.marketId,
      qc.getQueryData<AccountEvent[]>([
        "account",
        session.accountId,
        "events",
      ]) ?? [],
      qc.getQueryData<PendingOrder[]>([
        "account",
        session.accountId,
        "orders",
      ]) ?? [],
    );
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
        batchAnchorPerfMs,
        submitPerfMs,
        priorMaxOrderId: floorOrderId,
        // DegenActive.expiresAtBlock is number; built.order.expiresAtBlock is bigint.
        expiresAtBlock: Number(built.order.expiresAtBlock),
      });
      qc.invalidateQueries({ queryKey: ["account", session.accountId, "events"] });
      qc.invalidateQueries({ queryKey: ["account", session.accountId, "orders"] });
      qc.invalidateQueries({ queryKey: ["account", session.accountId, "portfolio"] });
      qc.invalidateQueries({ queryKey: ["orders", "pending"] });
    } catch (e) {
      // warn (not error): the rejection is handled and shown humanized below;
      // console.error would trip the Next dev overlay with the raw Rust string.
      console.warn("degen bet submit failed:", e);
      setSubmitError(humanizeOrderError(e, "bet"));
    } finally {
      setSigning(false);
    }
  }

  // Cancel the in-flight bet from the progress card. The order id is bound by
  // the tracker off the events feed (null until the `placed` row lands). On a
  // confirmed cancel, cancelSignedOrder records into the local cancel log, which
  // the tracker observes and flips the card to its terminal state — the same
  // path a cancel from the open-orders table takes. Passing `context` is what
  // triggers that record, so it's required for the bridge, not just cosmetic.
  async function onCancelBet() {
    const orderId = tracking?.orderId ?? null;
    if (!session || !active || orderId === null) return;
    setCancelling(true);
    // Flip the card to its cancelled state now; revert only if the backend
    // rejects (e.g. the order already filled/expired in the meantime).
    setOptimisticallyCancelledBetKey(activeBetKey);
    try {
      const remaining = active.targetQty - (tracking?.filledQty ?? 0n);
      await cancelSignedOrder({
        accountId: session.accountId,
        publicKeyHex: session.publicKeyHex,
        orderId,
        context: {
          marketId: active.marketId,
          side: active.outcome === "YES" ? "BuyYes" : "BuyNo",
          qty: Number(remaining > 0n ? remaining : 0n),
          limitPriceNanos: String(active.limitPriceNanos),
        },
      });
      qc.invalidateQueries({ queryKey: ["account", session.accountId, "events"] });
      qc.invalidateQueries({ queryKey: ["account", session.accountId, "orders"] });
      qc.invalidateQueries({ queryKey: ["account", session.accountId, "portfolio"] });
      qc.invalidateQueries({ queryKey: ["orders", "pending"] });
    } catch (e) {
      // The likely failures are "already filled" / "already expired" — undo the
      // optimistic flip and let the tracker resolve to filled/none on the next
      // block. warn (not error, which trips the dev overlay) is enough.
      console.warn("degen cancel failed:", e);
      setOptimisticallyCancelledBetKey(null);
    } finally {
      setCancelling(false);
    }
  }

  const connected = session !== null;
  // A never-traded market has no clearing price and no price history — the mark
  // then falls back to a neutral 50¢, which we must NOT present as a real quote.
  // Surface it as a "seed the book" state instead of a fabricated payout.
  const hasPrice =
    selected.yesPriceNanos != null ||
    (pricePoints != null && pricePoints.length > 0);
  // Cash the engine would reserve for this bet. Block the CTA when it exceeds
  // what's available so we never trip a server-side InsufficientBalance rejection.
  const requiredNanos = built.ok
    ? notionalNanosCeil(built.order.limitPriceNanos, built.order.maxFill)
    : 0n;
  const insufficient =
    connected &&
    built.ok &&
    availableNanos != null &&
    requiredNanos > availableNanos;

  // NegRisk self-trade prevention: in a market group, a resting buy can make
  // this bet complete a full outcome set, which the engine rejects outright
  // (CompleteSetFormation). Catch it before the bettor signs, and say which
  // order is in the way — the raw rejection explains nothing.
  const blockers = connected
    ? findCompleteSetBlockers({
        groupMarkets,
        restingOrders: openOrders ?? [],
        marketId: selected.marketId,
        side: side === "YES" ? "BuyYes" : "BuyNo",
      })
    : null;
  const completeSet = blockers != null && blockers.length > 0;
  const completeSetReasonText = completeSet
    ? completeSetReason(
        blockers,
        side === "YES" ? "BuyYes" : "BuyNo",
        selected.marketId,
        (m) => group.outcomes.find((o) => o.marketId === m)?.shortLabel ?? null,
      )
    : null;

  const ctaLabel = !connected
    ? "Connect to bet"
    : signing
      ? "Signing…"
      : !built.ok
        ? "Raise your bet"
        : insufficient
          ? "Not enough funds"
          : completeSet
            ? `Cancel your open order to bet ${side}`
            : `Bet $${amountNum} on ${side}${group.isMultiOutcome ? ` · ${selected.shortLabel}` : ""}`;
  const ctaDisabled =
    connected && (signing || !built.ok || insufficient || completeSet);

  // Explainer slot below the form/progress area:
  //  - while a bet is in flight ("tracking"): a compact WaitingAlert with the
  //    "why am I waiting?" copy tucked into an ⓘ tooltip;
  //  - after a missed bet ("none"): the short "why failed?" explainer;
  //  - pre-bet (null) and once a bet lands ("filled"/"partial"): nothing (the
  //    result card already explains itself).
  // The optimistic cancel wins over the tracker's phase; if shares already
  // partially filled before the cancel, that portion stands (read "partial").
  const trackedPhase = tracking?.phase ?? "tracking";
  const effectivePhase = cancelledLocal
    ? (tracking?.filledQty ?? 0n) > 0n
      ? "partial"
      : "cancelled"
    : trackedPhase;
  const resultPhase = active ? effectivePhase : null;
  const showWaiting = resultPhase === "tracking";
  const showFailed = resultPhase === "none";

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      {active ? (
        <DegenProgress
          phase={effectivePhase}
          side={active.outcome}
          secondsLeft={tracking?.secondsLeft ?? 0}
          timeProgress01={tracking?.timeProgress01 ?? 0}
          filledQty={tracking?.filledQty ?? 0n}
          targetQty={active.targetQty}
          betUsd={active.betUsd}
          avgPriceNanos={tracking?.avgPriceNanos ?? null}
          onBetAgain={() => setActive(null)}
          onCancel={onCancelBet}
          canCancel={(tracking?.orderId ?? null) !== null}
          cancelling={cancelling}
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
              seeding={!hasPrice}
            />
          </div>

          <button
            type="button"
            onClick={onBet}
            disabled={ctaDisabled}
            style={{
              marginTop: 4,
              minHeight: 52,
              padding: "16px 0",
              borderRadius: 6,
              border: 0,
              cursor: ctaDisabled ? "not-allowed" : "pointer",
              background: side === "YES" ? "var(--yes)" : "var(--no)",
              color: "var(--fg-on-accent)",
              fontFamily: "var(--font-sans)",
              fontSize: 15,
              fontWeight: 700,
              letterSpacing: "-0.005em",
              opacity: ctaDisabled ? 0.65 : 1,
              transform: signing ? "translateY(1px)" : "none",
            }}
          >
            {ctaLabel}
          </button>

          {completeSetReasonText && !submitError && (
            <div
              style={{
                fontFamily: "var(--font-mono)",
                fontSize: 11,
                lineHeight: "16px",
                color: "var(--fg-3)",
                textAlign: "center",
              }}
            >
              {completeSetReasonText}
            </div>
          )}

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

          <ResearchNudge />
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

function degenActiveKey(active: DegenActive) {
  return [
    active.accountId,
    active.marketId,
    active.outcome,
    active.submitHeight,
    active.submitPerfMs,
    active.expiresAtBlock,
    active.priorMaxOrderId ?? "none",
  ].join(":");
}
