"use client";

/**
 * Pro-mode order entry form. Wires the form to the live `/v1/orders/signed` endpoint via
 * `submitSignedOrder`. Side mapping:
 *   - buy + YES → BuyYes
 *   - buy + NO  → BuyNo
 *   - sell + YES → SellYes
 *   - sell + NO  → SellNo
 *
 * TTL → time_in_force + expires_at_block (relative to latestHeight from the store):
 *   - "GTC" → time_in_force GTC, no expiry (backend default; rests until cancel)
 *   - "IOC" → time_in_force IOC, expires_at_block = latestHeight + 1 (next batch only)
 *   - "GTD" → time_in_force GTD, expires_at_block = latestHeight + N batches (picker)
 * IOC/GTD both sign `expires_at_block`; the server verifies it against the P256
 * signature. IOC is confirmed server-supported (TimeInForce enum GTC|IOC|GTD).
 */

import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import {
  submitSignedOrder,
  type OrderSide,
  type SubmitTimeInForce,
} from "@/lib/account/orders";
import { humanizeOrderError } from "@/lib/account/order-errors";
import type { AccountOrder } from "@/lib/account/use-account-orders";
import {
  formatShareUnits,
  notionalNanosCeil,
  sharesToUnits,
  unitsToShares,
} from "@/lib/account/quantity";
import {
  useAccountSession,
  useSetConnectModalOpen,
} from "@/lib/account/use-account";
import { useAvailableBalance } from "@/lib/account/use-available-balance";
import { usePortfolio } from "@/lib/account/use-portfolio";
import { formatBatchSeconds, formatDollars } from "@/lib/format/nanos";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";
import { useBatchCountdown } from "./use-batch-countdown";

type Direction = "buy" | "sell";
type OutcomeSide = "YES" | "NO";
type Unit = "usd" | "shares";
type Tif = SubmitTimeInForce;

const TIF_OPTS: Tif[] = ["GTC", "IOC", "GTD"];
const TIF_HELP: Record<Tif, string> = {
  GTC: "rests until you cancel",
  IOC: "next batch only, then expires",
  GTD: "rests for a chosen number of batches",
};

function orderSideFor(dir: Direction, side: OutcomeSide): OrderSide {
  if (dir === "buy") return side === "YES" ? "BuyYes" : "BuyNo";
  return side === "YES" ? "SellYes" : "SellNo";
}

export function BuyBox({
  outcome,
  requireConfirmation = false,
}: {
  outcome: EventOutcome;
  /** Modal flow only: review once, then explicitly confirm before signing. */
  requireConfirmation?: boolean;
}) {
  const session = useAccountSession();
  const openConnectModal = useSetConnectModalOpen();
  const qc = useQueryClient();
  const { secondsLeftPrecise, latestHeight } = useBatchCountdown();
  const batchNumber = latestHeight == null ? null : latestHeight + 1;
  const portfolio = usePortfolio(session?.accountId ?? null);
  const { availableNanos, isPending: balancePending } = useAvailableBalance(
    session?.accountId ?? null,
  );

  // A never-traded market has no price yet (absent from /v1/markets/prices).
  // We still need a numeric seed for the limit slider, but we must NOT present
  // that seed as a real indicative quote — see `hasPrice` gating below.
  const hasPrice = outcome.yesCents != null;
  const yesCents = outcome.yesCents ?? 50;
  const noCents = 100 - yesCents;

  const [dir, setDir] = useState<Direction>("buy");
  const [outcomeSide, setOutcomeSide] = useState<OutcomeSide>("YES");
  const [unit, setUnit] = useState<Unit>("usd");
  const [amount, setAmount] = useState("25");
  const [shares, setShares] = useState("100");
  const [tif, setTif] = useState<Tif>("GTC");
  // GTD block-height picker: how many batches ahead the order stays eligible.
  const [gtdBatches, setGtdBatches] = useState(5);

  const indicativeCents = outcomeSide === "YES" ? yesCents : noCents;
  const [limit, setLimit] = useState<number>(indicativeCents);
  const [limitText, setLimitText] = useState<string>(String(indicativeCents));

  // When the user flips YES↔NO, default the limit slider to the new side's
  // indicative. They can still override after.
  /* eslint-disable react-hooks/set-state-in-effect -- re-anchor limit when side flips */
  useEffect(() => {
    setLimit(indicativeCents);
  }, [outcomeSide, indicativeCents]);
  // Mirror limit value into the (controlled) text field.
  useEffect(() => {
    setLimitText(String(limit));
  }, [limit]);
  /* eslint-enable */

  const [submitting, setSubmitting] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  // Accepted receipt: the order-id (best-effort, looked up from the refreshed
  // pending list — the signed endpoint returns only `{ accepted }`) plus the
  // block it will clear in and a one-line summary.
  const [accepted, setAccepted] = useState<{
    orderId: number | null;
    block: number | null;
    summary: string;
  } | null>(null);

  // Any edit in the trader section makes the last submit's receipt stale — drop
  // the confirmation (and any prior error) so it doesn't linger while the user
  // lines up a different order (e.g. flips to Sell NO).
  /* eslint-disable react-hooks/set-state-in-effect -- clear stale receipt on edit */
  useEffect(() => {
    setConfirming(false);
    setAccepted(null);
    setSubmitError(null);
  }, [dir, outcomeSide, unit, amount, shares, tif, gtdBatches, limit]);
  /* eslint-enable react-hooks/set-state-in-effect */

  const limitDec = Math.max(1, Math.min(99, limit)) / 100;
  const usd = parseFloat(amount) || 0;
  const sh = parseFloat(shares) || 0;
  const sharesIfUsd = limitDec > 0 ? usd / limitDec : 0;
  const limitCentsPreview = Math.max(1, Math.min(99, Math.round(limit)));
  const limitPriceNanosPreview = BigInt(limitCentsPreview) * 10_000_000n;

  // Unified receipt quantities. `qtyUnits` is the order size the engine will see
  // (`max_fill`), floored to the protocol's 0.001-share increment.
  const qtyUnits = sharesToUnits(unit === "usd" ? sharesIfUsd : sh);
  const qtyShares = unitsToShares(qtyUnits);
  const grossAtLimit =
    Number(notionalNanosCeil(limitPriceNanosPreview, qtyUnits)) / 1e9;
  const payoutIfWin = qtyShares; // qty × $1

  // Live clearing ESTIMATE from the side's indicative price. A batch auction
  // gives no firm quote, so this is only "what the next batch would fill near
  // if it clears at today's indicative" — surfaced as an estimate, never a quote.
  const estClearingCents = Math.max(
    1,
    Math.min(99, Math.round(indicativeCents)),
  );
  const estFillDollars = qtyShares * (estClearingCents / 100);

  // Cash available to BUY = balance − cash reserved by resting buy orders.
  // (Sells are gated by held shares below, not cash.) Matches the engine so a
  // buy MAX / headroom never proposes more than will be accepted.
  const availableDollars =
    availableNanos == null ? null : Number(availableNanos) / 1e9;

  // Shares of THIS outcome+side the user currently holds — what they can sell.
  // Positions carry the outcome as "YES"/"NO" (accounts route), matching
  // `outcomeSide`.
  const heldUnits =
    portfolio.data?.positions?.find(
      (p) => p.market_id === outcome.marketId && p.outcome === outcomeSide,
    )?.quantity ?? 0;
  const heldShares = unitsToShares(heldUnits);

  // Quick-amount chips on the order input. `+N` is additive; `MAX` fills the
  // available balance (needs a known balance). Mirrors the handoff BuyBox.
  // On a sell, MAX means "all the shares you hold"; on a buy it fills the
  // available cash balance (as $ or balance/limit shares).
  const quickChips: { label: string; disabled?: boolean; apply: () => void }[] =
    unit === "usd"
      ? [
          {
            label: "+10",
            apply: () => setAmount(String((parseFloat(amount) || 0) + 10)),
          },
          {
            label: "+50",
            apply: () => setAmount(String((parseFloat(amount) || 0) + 50)),
          },
          dir === "sell"
            ? {
                label: "MAX",
                disabled: heldShares <= 0,
                apply: () => setAmount((heldShares * limitDec).toFixed(2)),
              }
            : {
                label: "MAX",
                disabled: availableDollars == null,
                apply: () => {
                  if (availableDollars != null)
                    setAmount(availableDollars.toFixed(2));
                },
              },
        ]
      : [
          {
            label: "+10",
            apply: () => setShares(String((parseFloat(shares) || 0) + 10)),
          },
          {
            label: "+100",
            apply: () => setShares(String((parseFloat(shares) || 0) + 100)),
          },
          dir === "sell"
            ? {
                label: "MAX",
                disabled: heldShares <= 0,
                apply: () => setShares(formatShareUnits(heldUnits)),
              }
            : {
                label: "MAX",
                disabled: availableDollars == null,
                apply: () => {
                  if (availableDollars != null)
                    setShares(
                      formatShareUnits(
                        sharesToUnits(availableDollars / limitDec),
                      ),
                    );
                },
              },
        ];

  const accent = outcomeSide === "YES" ? "var(--yes)" : "var(--no)";
  const accentSoft =
    outcomeSide === "YES"
      ? "color-mix(in srgb, var(--yes) 14%, transparent)"
      : "color-mix(in srgb, var(--no) 14%, transparent)";

  const connected = session !== null;
  const disabledInputs = !connected || submitting;

  // Block a BUY whose cost exceeds available cash, so we never trip a
  // server-side InsufficientBalance. Sells are gated by held shares instead.
  const buyCostNanos = notionalNanosCeil(limitPriceNanosPreview, qtyUnits);
  const insufficientBuy =
    connected &&
    dir === "buy" &&
    availableNanos != null &&
    buyCostNanos > availableNanos;
  // Block a SELL of more shares than held — the mirror of insufficientBuy — so
  // we never trip a server-side InsufficientPosition. Only enforced once the
  // portfolio has loaded (heldShares defaults to 0 while that query is pending,
  // which would otherwise reject every sell).
  const positionsLoaded = portfolio.data != null;
  const insufficientSell =
    connected &&
    dir === "sell" &&
    positionsLoaded &&
    qtyUnits > BigInt(heldUnits);
  const ctaState = tradeCtaState({
    connected,
    submitting,
    direction: dir,
    balanceKnown: availableNanos != null,
    balancePending,
    positionsKnown: positionsLoaded,
    positionsPending: portfolio.isPending,
    insufficientBuy,
    insufficientSell,
  });
  const ctaOff = ctaState !== "connect" && ctaState !== "ready";

  const ctaLabel = (() => {
    if (ctaState === "connect") return "Connect to trade";
    if (ctaState === "signing") return "Signing…";
    if (ctaState === "waiting_balance") return "Loading balance…";
    if (ctaState === "balance_unavailable") return "Balance unavailable";
    if (ctaState === "waiting_positions") return "Loading positions…";
    if (ctaState === "positions_unavailable") return "Positions unavailable";
    if (ctaState === "insufficient_buy") return "Not enough funds";
    if (ctaState === "insufficient_sell") return "Not enough shares";
    const sideWord = requireConfirmation
      ? confirming
        ? dir === "buy"
          ? "confirm buy"
          : "confirm sell"
        : dir === "buy"
          ? "review buy"
          : "review sell"
      : dir === "buy"
        ? "queue buy"
        : "queue sell";
    const batchSuffix =
      batchNumber == null ? "" : ` → batch #${batchNumber.toLocaleString()}`;
    return `${sideWord}${batchSuffix}`;
  })();

  async function onCtaClick() {
    if (!connected) {
      openConnectModal(true);
      return;
    }
    if (!session) return;
    // Never start a passkey ceremony against unknown private account state.
    // Query errors recover on the next block/poll; until then the disabled CTA
    // above truthfully reports that balance/positions are unavailable.
    if (dir === "buy" && availableNanos == null) return;
    if (dir === "sell" && !positionsLoaded) return;
    setSubmitError(null);
    setAccepted(null);

    // Resolve qty (max_fill) and basic validation.
    const maxFill = sharesToUnits(unit === "usd" ? sharesIfUsd : sh);
    if (maxFill < 1n) {
      setSubmitError("max_fill must be at least 0.001 share");
      return;
    }
    const limitCents = Math.max(1, Math.min(99, Math.round(limit)));
    const limitPriceNanos = BigInt(limitCents) * 10_000_000n; // cents × 1e7

    let expiresAtBlock: bigint | undefined;
    if (tif !== "GTC") {
      if (latestHeight == null) {
        setSubmitError("waiting for latest block — try again in a moment");
        return;
      }
      // IOC commits to the very next block; GTD rests for the picked horizon.
      const horizon = tif === "IOC" ? 1 : Math.max(1, Math.round(gtdBatches));
      expiresAtBlock = BigInt(latestHeight + horizon);
    }

    // The focused modal is intentionally two-step: the first press freezes a
    // clear review state; the second is the only press that starts signing.
    // Editing any order field clears this state via the effect above.
    if (requireConfirmation && !confirming) {
      setConfirming(true);
      return;
    }

    setSubmitting(true);
    try {
      const res = await submitSignedOrder({
        accountId: session.accountId,
        publicKeyHex: session.publicKeyHex,
        marketId: outcome.marketId,
        side: orderSideFor(dir, outcomeSide),
        limitPriceNanos,
        maxFill,
        timeInForce: tif,
        ...(expiresAtBlock !== undefined ? { expiresAtBlock } : {}),
      });
      if (!res.accepted) {
        setSubmitError("server returned accepted=false");
      } else {
        // Refresh per-account caches and the chain-wide pending list (consumed
        // by market-rail's pending feed). Await the orders refetch so we can
        // recover the new order-id from it below.
        await qc.invalidateQueries({
          queryKey: ["account", session.accountId, "orders"],
        });
        qc.invalidateQueries({
          queryKey: ["account", session.accountId, "portfolio"],
        });
        qc.invalidateQueries({ queryKey: ["orders", "pending"] });

        // Prefer the sequencer's authoritative id from the submit response.
        // Older API builds return no `order_ids`, so fall back to a best-effort
        // recovery from the refreshed pending list (newest open order for this
        // market). A filled IOC leaves nothing pending → null.
        const orderId =
          res.orderIds[0] ??
          latestOrderIdFor(
            qc.getQueryData<AccountOrder[]>([
              "account",
              session.accountId,
              "orders",
            ]),
            outcome.marketId,
          );
        setAccepted({
          orderId,
          block: batchNumber,
          summary: `${formatShareUnits(maxFill)} sh @ ${limitCents}¢ · ${tif}`,
        });
        setConfirming(false);
      }
    } catch (e) {
      // warn (not error): the rejection is handled and shown humanized below;
      // console.error would trip the Next dev overlay with the raw Rust string.
      console.warn("order submit failed:", e);
      setSubmitError(humanizeOrderError(e, "order"));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div
      style={{
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: 8,
        padding: "14px 16px",
        display: "flex",
        flexDirection: "column",
        gap: 10,
        position: "relative",
      }}
    >
      <div
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 10,
          color: "var(--fg-3)",
          textTransform: "uppercase",
          letterSpacing: "0.04em",
        }}
      >
        place order
      </div>

      {/* Buy/Sell toggle */}
      <div
        style={{
          display: "flex",
          background: "var(--bg-2)",
          border: "1px solid var(--border-1)",
          borderRadius: 4,
          padding: 2,
          gap: 2,
        }}
      >
        {(["buy", "sell"] as Direction[]).map((s) => {
          const active = dir === s;
          return (
            <button
              key={s}
              type="button"
              onClick={() => setDir(s)}
              disabled={disabledInputs}
              style={{
                flex: 1,
                minHeight: 40,
                padding: "7px 0",
                border: 0,
                borderRadius: 3,
                cursor: disabledInputs ? "not-allowed" : "pointer",
                background: active ? "var(--surface-2)" : "transparent",
                color: active ? "var(--fg-1)" : "var(--fg-3)",
                fontFamily: "var(--font-sans)",
                fontSize: 12,
                fontWeight: active ? 600 : 500,
                textTransform: "capitalize",
                opacity: disabledInputs ? 0.7 : 1,
              }}
            >
              {s}
            </button>
          );
        })}
      </div>

      {/* Outcome context + YES/NO sub-toggle */}
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 8,
            fontFamily: "var(--font-sans)",
            fontSize: 12,
            color: "var(--fg-3)",
          }}
        >
          <span
            style={{
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              minWidth: 0,
            }}
            title={outcome.label}
          >
            {outcome.shortLabel}
          </span>
          <span
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 10,
              color: "var(--fg-4)",
              letterSpacing: "0.04em",
              textTransform: "uppercase",
            }}
          >
            side
          </span>
        </div>
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "1fr 1fr",
            gap: 4,
          }}
        >
          {(["YES", "NO"] as OutcomeSide[]).map((s) => {
            const active = outcomeSide === s;
            const cents = s === "YES" ? yesCents : noCents;
            const sideColor = s === "YES" ? "var(--yes)" : "var(--no)";
            return (
              <button
                key={s}
                type="button"
                onClick={() => setOutcomeSide(s)}
                disabled={disabledInputs}
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  minHeight: 44,
                  padding: "8px 10px",
                  borderRadius: 4,
                  border: active
                    ? `1px solid ${sideColor}`
                    : "1px solid var(--border-1)",
                  background: active
                    ? `color-mix(in srgb, ${sideColor} 14%, transparent)`
                    : "var(--bg-2)",
                  cursor: disabledInputs ? "not-allowed" : "pointer",
                  color: active ? sideColor : "var(--fg-2)",
                  fontFamily: "var(--font-sans)",
                  fontSize: 12,
                  fontWeight: active ? 600 : 500,
                  opacity: disabledInputs ? 0.7 : 1,
                }}
              >
                <span>{s}</span>
                <span
                  className="tabular"
                  style={{
                    fontFamily: "var(--font-mono)",
                    fontSize: 13,
                  }}
                >
                  {hasPrice ? `${cents}¢` : "—"}
                </span>
              </button>
            );
          })}
        </div>
      </div>

      {/* Order in $ vs shares */}
      <div>
        <div
          style={{
            display: "flex",
            alignItems: "baseline",
            justifyContent: "space-between",
            marginBottom: 5,
          }}
        >
          <Eyebrow>order in</Eyebrow>
          <span
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 10,
              color: "var(--fg-4)",
            }}
          >
            {dir === "sell"
              ? `balance ${formatShareUnits(heldUnits)} sh`
              : availableDollars == null
                ? ""
                : unit === "usd"
                  ? `available ${formatDollars(BigInt(Math.floor(availableDollars * 1e9)), { decimals: 2 })}`
                  : `max ${(availableDollars / limitDec).toFixed(0)} sh`}
          </span>
        </div>
        <div style={{ display: "flex", gap: 4, marginBottom: 6 }}>
          {(["usd", "shares"] as Unit[]).map((u) => {
            const active = unit === u;
            return (
              <button
                key={u}
                type="button"
                onClick={() => setUnit(u)}
                disabled={disabledInputs}
                style={{
                  flex: 1,
                  minHeight: 40,
                  padding: "6px 0",
                  borderRadius: 3,
                  cursor: disabledInputs ? "not-allowed" : "pointer",
                  background: active ? "var(--surface-2)" : "var(--bg-2)",
                  border: `1px solid ${active ? "var(--border-3)" : "var(--border-1)"}`,
                  color: active ? "var(--fg-1)" : "var(--fg-3)",
                  fontFamily: "var(--font-mono)",
                  fontSize: 10.5,
                  opacity: disabledInputs ? 0.7 : 1,
                }}
              >
                {u === "usd" ? "$ amount" : "shares"}
              </button>
            );
          })}
        </div>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 4,
            background: "var(--bg-2)",
            border: "1px solid var(--border-1)",
            borderRadius: 4,
            padding: "6px 10px",
          }}
        >
          <span
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 18,
              color: "var(--fg-3)",
            }}
          >
            {unit === "usd" ? "$" : "#"}
          </span>
          <input
            type="text"
            inputMode="decimal"
            aria-label={
              unit === "usd"
                ? "Order amount in dollars"
                : "Order quantity in shares"
            }
            value={unit === "usd" ? amount : shares}
            disabled={disabledInputs}
            onChange={(e) =>
              unit === "usd"
                ? setAmount(e.target.value.replace(/[^0-9.]/g, ""))
                : setShares(e.target.value.replace(/[^0-9.]/g, ""))
            }
            style={{
              flex: 1,
              background: "transparent",
              border: 0,
              outline: 0,
              padding: "4px 4px",
              color: "var(--fg-1)",
              fontFamily: "var(--font-mono)",
              fontSize: 18,
              fontVariantNumeric: "tabular-nums",
              cursor: disabledInputs ? "not-allowed" : "text",
            }}
          />
        </div>
        <div style={{ display: "flex", gap: 4, marginTop: 6 }}>
          {quickChips.map((c) => {
            const off = disabledInputs || c.disabled;
            return (
              <button
                key={c.label}
                type="button"
                disabled={off}
                onClick={c.apply}
                style={{
                  flex: 1,
                  minHeight: 40,
                  padding: "6px 0",
                  borderRadius: 3,
                  background: "var(--bg-2)",
                  border: "1px solid var(--border-1)",
                  color: "var(--fg-3)",
                  fontFamily: "var(--font-mono)",
                  fontSize: 10.5,
                  cursor: off ? "not-allowed" : "pointer",
                  opacity: off ? 0.5 : 1,
                }}
              >
                {c.label}
              </button>
            );
          })}
        </div>
      </div>

      {/* Limit price input + slider */}
      <div>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "baseline",
            marginBottom: 5,
          }}
        >
          <Eyebrow>limit price</Eyebrow>
          <button
            type="button"
            onClick={() => setLimit(indicativeCents)}
            disabled={disabledInputs || !hasPrice}
            style={{
              background: "transparent",
              border: 0,
              minHeight: 40,
              padding: "0 var(--space-1)",
              cursor: disabledInputs || !hasPrice ? "not-allowed" : "pointer",
              color: !hasPrice
                ? "var(--fg-4)"
                : limit === indicativeCents
                  ? "var(--fg-3)"
                  : "var(--accent)",
              fontFamily: "var(--font-mono)",
              fontSize: 10,
              textDecoration: hasPrice ? "underline" : "none",
              textUnderlineOffset: 2,
            }}
          >
            {hasPrice
              ? `set indicative ${indicativeCents}¢`
              : "no indicative yet"}
          </button>
        </div>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            background: "var(--bg-2)",
            border: "1px solid var(--border-1)",
            borderRadius: 4,
            padding: "6px 10px",
            marginBottom: 8,
          }}
        >
          <input
            type="text"
            inputMode="decimal"
            aria-label="Limit price in cents"
            value={limitText}
            disabled={disabledInputs}
            onChange={(e) => {
              const v = e.target.value.replace(/[^0-9.]/g, "");
              setLimitText(v);
              const n = parseFloat(v);
              if (!Number.isNaN(n)) setLimit(Math.max(1, Math.min(99, n)));
            }}
            style={{
              flex: 1,
              background: "transparent",
              border: 0,
              outline: 0,
              padding: "2px 0",
              color: "var(--fg-1)",
              fontFamily: "var(--font-mono)",
              fontSize: 16,
              fontVariantNumeric: "tabular-nums",
              cursor: disabledInputs ? "not-allowed" : "text",
            }}
          />
          <span
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 14,
              color: "var(--fg-3)",
            }}
          >
            ¢
          </span>
        </div>
        <input
          type="range"
          aria-label="Limit price slider in cents"
          min={1}
          max={99}
          value={limit}
          disabled={disabledInputs}
          onChange={(e) => setLimit(Number(e.target.value))}
          style={{
            width: "100%",
            minHeight: 40,
            cursor: disabledInputs ? "not-allowed" : "pointer",
          }}
        />
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            fontFamily: "var(--font-mono)",
            fontSize: 9,
            color: "var(--fg-4)",
          }}
        >
          <span>1¢</span>
          <span>
            {hasPrice ? `indicative ${indicativeCents}¢` : "seed the book"}
          </span>
          <span>99¢</span>
        </div>
      </div>

      {/* Time-in-force: GTC / IOC / GTD (with a batch-height picker for GTD) */}
      <div>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "baseline",
          }}
        >
          <Eyebrow>time in force</Eyebrow>
          <span
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 9.5,
              color: "var(--fg-4)",
            }}
          >
            {TIF_HELP[tif]}
          </span>
        </div>
        <div style={{ display: "flex", gap: 4, marginTop: 5 }}>
          {TIF_OPTS.map((t) => {
            const active = tif === t;
            return (
              <button
                key={t}
                type="button"
                onClick={() => setTif(t)}
                disabled={disabledInputs}
                style={{
                  flex: 1,
                  minHeight: 40,
                  padding: "6px 0",
                  borderRadius: 3,
                  cursor: disabledInputs ? "not-allowed" : "pointer",
                  background: active ? "var(--surface-2)" : "var(--bg-2)",
                  border: `1px solid ${active ? "var(--border-3)" : "var(--border-1)"}`,
                  color: active ? "var(--fg-1)" : "var(--fg-3)",
                  fontFamily: "var(--font-mono)",
                  fontSize: 11,
                  fontWeight: active ? 600 : 500,
                  opacity: disabledInputs ? 0.7 : 1,
                }}
              >
                {t}
              </button>
            );
          })}
        </div>
        {tif === "GTD" && (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              gap: 6,
              marginTop: 6,
              padding: "6px 8px",
              background: "var(--bg-2)",
              border: "1px solid var(--border-1)",
              borderRadius: 4,
            }}
          >
            <span
              style={{
                fontFamily: "var(--font-mono)",
                fontSize: 10,
                color: "var(--fg-3)",
              }}
            >
              expires in
            </span>
            <div style={{ display: "flex", alignItems: "center", gap: 4 }}>
              <StepButton
                label="−"
                disabled={disabledInputs || gtdBatches <= 1}
                onClick={() => setGtdBatches((n) => Math.max(1, n - 1))}
              />
              <span
                className="tabular"
                style={{
                  minWidth: 60,
                  textAlign: "center",
                  fontFamily: "var(--font-mono)",
                  fontSize: 12,
                  color: "var(--fg-1)",
                }}
              >
                {gtdBatches} {gtdBatches === 1 ? "batch" : "batches"}
              </span>
              <StepButton
                label="+"
                disabled={disabledInputs || gtdBatches >= 60}
                onClick={() => setGtdBatches((n) => Math.min(60, n + 1))}
              />
            </div>
          </div>
        )}
        {tif !== "GTC" && latestHeight != null && (
          <div
            style={{
              marginTop: 5,
              fontFamily: "var(--font-mono)",
              fontSize: 9.5,
              color: "var(--fg-4)",
              textAlign: "right",
            }}
          >
            expires block #
            {(
              latestHeight + (tif === "IOC" ? 1 : Math.max(1, gtdBatches))
            ).toLocaleString()}
          </div>
        )}
      </div>

      {/* Receipt */}
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: 5,
          padding: "10px 12px",
          background: accentSoft,
          border: `1px dashed color-mix(in srgb, ${accent} 32%, transparent)`,
          borderRadius: 4,
          fontFamily: "var(--font-mono)",
          fontSize: 11,
        }}
      >
        {/* Live clearing estimate — labelled an estimate; a batch auction gives
            no firm quote until the next batch clears. For a never-traded market
            there is no price to estimate against, so instead of a fabricated
            ~50% fill we say the order would seed the book at the chosen limit. */}
        {hasPrice ? (
          <ReceiptRow
            label={
              dir === "buy"
                ? "est. fill · next batch"
                : "est. proceeds · next batch"
            }
            value={
              <span style={{ color: "var(--fg-2)" }}>
                ~${estFillDollars.toFixed(2)} at ~{estClearingCents}%
              </span>
            }
          />
        ) : (
          <div style={{ color: "var(--fg-3)", lineHeight: 1.35 }}>
            no price yet — your order would seed the book at{" "}
            <span style={{ color: "var(--fg-1)" }}>{limitCentsPreview}¢</span>
          </div>
        )}
        {dir === "buy" ? (
          <>
            {/* Buy: pay AT MOST limit×qty (uniform clearing may be cheaper),
                receive qty shares, each worth $1 if the outcome resolves in. */}
            <ReceiptRow
              label="max cost"
              value={`≤ $${grossAtLimit.toFixed(2)}`}
            />
            <ReceiptRow
              label="shares (if filled)"
              value={formatShareUnits(qtyUnits)}
            />
            <ReceiptRow
              label="payout if it wins"
              value={`$${payoutIfWin.toFixed(2)}`}
            />
          </>
        ) : (
          <>
            {/* Sell: receive AT LEAST limit×qty (uniform clearing may pay more)
                in exchange for the shares you're selling. */}
            <ReceiptRow
              label="min receive"
              value={`≥ $${grossAtLimit.toFixed(2)}`}
            />
            <ReceiptRow
              label="shares to sell"
              value={formatShareUnits(qtyUnits)}
            />
          </>
        )}
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            color: "var(--fg-3)",
            borderTop: "1px solid var(--border-1)",
            paddingTop: 5,
            marginTop: 2,
          }}
        >
          <span>queued for batch</span>
          <span style={{ color: "var(--accent)" }}>
            #{batchNumber == null ? "—" : batchNumber.toLocaleString()} ·{" "}
            {formatBatchSeconds(secondsLeftPrecise)}s
          </span>
        </div>
      </div>

      {requireConfirmation && confirming && !accepted && (
        <div
          role="status"
          data-testid="order-review"
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 5,
            padding: "9px 10px",
            borderRadius: 4,
            border:
              "1px solid color-mix(in srgb, var(--accent) 38%, transparent)",
            background: "color-mix(in srgb, var(--accent) 10%, transparent)",
            color: "var(--fg-2)",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
          }}
        >
          <strong style={{ color: "var(--fg-1)", fontWeight: 600 }}>
            Confirm {dir} {outcomeSide}
          </strong>
          <span>
            {formatShareUnits(qtyUnits)} shares @ {limitCentsPreview}¢ · {tif}
          </span>
          <span style={{ color: "var(--fg-3)" }}>
            {dir === "buy"
              ? `maximum cost $${grossAtLimit.toFixed(2)}`
              : `minimum receive $${grossAtLimit.toFixed(2)}`}
          </span>
        </div>
      )}

      {/* CTA */}
      <button
        type="button"
        onClick={onCtaClick}
        disabled={ctaOff}
        style={{
          marginTop: 2,
          minHeight: 48,
          padding: "12px 0",
          border: 0,
          borderRadius: 4,
          cursor: ctaOff ? "not-allowed" : "pointer",
          background: connected ? accent : "var(--accent)",
          color: "var(--fg-on-accent)",
          fontFamily: "var(--font-sans)",
          fontSize: 14,
          fontWeight: 600,
          letterSpacing: "0.01em",
          opacity: ctaOff ? 0.55 : 1,
          transform: submitting ? "translateY(1px)" : "none",
        }}
      >
        {ctaLabel}
      </button>

      {submitError && (
        <div
          role="alert"
          style={{
            padding: "6px 10px",
            background: "color-mix(in srgb, var(--no) 12%, transparent)",
            border: "1px solid color-mix(in srgb, var(--no) 32%, transparent)",
            borderRadius: 4,
            color: "var(--no)",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            wordBreak: "break-word",
          }}
        >
          {submitError}
        </div>
      )}
      {accepted && !submitError && (
        <div
          role="status"
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 4,
            minHeight: 40,
            padding: "8px 10px",
            background: "color-mix(in srgb, var(--yes) 12%, transparent)",
            border: "1px solid color-mix(in srgb, var(--yes) 32%, transparent)",
            borderRadius: 4,
            color: "var(--yes)",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
          }}
        >
          <div
            style={{ display: "flex", justifyContent: "space-between", gap: 8 }}
          >
            <span>order accepted</span>
            <span>
              {accepted.orderId != null ? `#${accepted.orderId}` : "queued"}
            </span>
          </div>
          <div style={{ color: "var(--fg-2)" }}>{accepted.summary}</div>
          <div style={{ color: "var(--fg-3)" }}>
            clears in block{" "}
            {accepted.block == null
              ? "—"
              : `#${accepted.block.toLocaleString()}`}
          </div>
        </div>
      )}

      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 9.5,
          color: "var(--fg-4)",
          textAlign: "center",
        }}
      >
        clears at the uniform price · could fill better than your limit
      </span>
    </div>
  );
}

export type TradeCtaState =
  | "connect"
  | "signing"
  | "waiting_balance"
  | "balance_unavailable"
  | "waiting_positions"
  | "positions_unavailable"
  | "insufficient_buy"
  | "insufficient_sell"
  | "ready";

/** Pure state machine for the authenticated Pro trade action. */
export function tradeCtaState({
  connected,
  submitting,
  direction,
  balanceKnown,
  balancePending,
  positionsKnown,
  positionsPending,
  insufficientBuy,
  insufficientSell,
}: {
  connected: boolean;
  submitting: boolean;
  direction: Direction;
  balanceKnown: boolean;
  balancePending: boolean;
  positionsKnown: boolean;
  positionsPending: boolean;
  insufficientBuy: boolean;
  insufficientSell: boolean;
}): TradeCtaState {
  if (!connected) return "connect";
  if (submitting) return "signing";
  if (direction === "buy" && !balanceKnown) {
    return balancePending ? "waiting_balance" : "balance_unavailable";
  }
  if (direction === "sell" && !positionsKnown) {
    return positionsPending ? "waiting_positions" : "positions_unavailable";
  }
  if (insufficientBuy) return "insufficient_buy";
  if (insufficientSell) return "insufficient_sell";
  return "ready";
}

/** Newest (highest order_id) open order for a market — best-effort id recovery
 * from the refreshed pending list, since the signed endpoint returns no id. */
function latestOrderIdFor(
  orders: AccountOrder[] | undefined,
  marketId: number,
): number | null {
  if (!orders || orders.length === 0) return null;
  let best: number | null = null;
  for (const o of orders) {
    if (o.market_id !== marketId) continue;
    if (best === null || o.order_id > best) best = o.order_id;
  }
  return best;
}

function StepButton({
  label,
  disabled,
  onClick,
}: {
  label: string;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={label === "+" ? "increase batches" : "decrease batches"}
      style={{
        width: 40,
        height: 40,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        borderRadius: 3,
        border: "1px solid var(--border-1)",
        background: "var(--surface-2)",
        color: "var(--fg-2)",
        fontFamily: "var(--font-mono)",
        fontSize: 13,
        lineHeight: 1,
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.4 : 1,
      }}
    >
      {label}
    </button>
  );
}

function Eyebrow({ children }: { children: React.ReactNode }) {
  return (
    <span
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        color: "var(--fg-3)",
        textTransform: "uppercase",
        letterSpacing: "0.04em",
      }}
    >
      {children}
    </span>
  );
}

function ReceiptRow({
  label,
  value,
}: {
  label: string;
  value: React.ReactNode;
}) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "space-between",
        color: "var(--fg-2)",
      }}
    >
      <span>{label}</span>
      <span style={{ color: "var(--fg-1)" }}>{value}</span>
    </div>
  );
}
