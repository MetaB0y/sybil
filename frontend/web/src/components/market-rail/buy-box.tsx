"use client";

/**
 * Pro-mode order entry form. Matches `BuyBox` in
 * `frontend/handoff/data/fed-fba-panel.jsx:206`.
 *
 * Wires the form to the live `/v1/orders/signed` endpoint via
 * `submitSignedOrder`. Side mapping:
 *   - buy + YES → BuyYes
 *   - buy + NO  → BuyNo
 *   - sell + YES → SellYes
 *   - sell + NO  → SellNo
 *
 * TTL → expires_at_block (relative to latestHeight from the store):
 *   - "1 batch"     → +1   (effectively IOC)
 *   - "5 batches"   → +5   (short GTD, replay-safe)
 *   - "until cancel" → undefined (GTC; backend default)
 */

import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import {
  submitSignedOrder,
  type OrderSide,
} from "@/lib/account/orders";
import { humanizeOrderError } from "@/lib/account/order-errors";
import {
  useAccountSession,
  useSetConnectModalOpen,
} from "@/lib/account/use-account";
import { useAvailableBalance } from "@/lib/account/use-available-balance";
import { usePortfolio } from "@/lib/account/use-portfolio";
import {
  formatBatchSeconds,
  formatDollars,
  formatInt,
} from "@/lib/format/nanos";
import type { EventOutcome } from "@/lib/market-detail/use-event-group";
import { useBatchCountdown } from "./use-batch-countdown";

type Direction = "buy" | "sell";
type OutcomeSide = "YES" | "NO";
type Unit = "usd" | "shares";
type Ttl = "1 batch" | "5 batches" | "until cancel";

const TTL_OPTS: Ttl[] = ["1 batch", "5 batches", "until cancel"];

function orderSideFor(dir: Direction, side: OutcomeSide): OrderSide {
  if (dir === "buy") return side === "YES" ? "BuyYes" : "BuyNo";
  return side === "YES" ? "SellYes" : "SellNo";
}

export function BuyBox({ outcome }: { outcome: EventOutcome }) {
  const session = useAccountSession();
  const openConnectModal = useSetConnectModalOpen();
  const qc = useQueryClient();
  const { secondsLeftPrecise, latestHeight } = useBatchCountdown();
  const batchNumber = latestHeight == null ? null : latestHeight + 1;
  const portfolio = usePortfolio(session?.accountId ?? null);
  const { availableNanos } = useAvailableBalance(session?.accountId ?? null);

  const yesCents = outcome.yesCents ?? 50;
  const noCents = 100 - yesCents;

  const [dir, setDir] = useState<Direction>("buy");
  const [outcomeSide, setOutcomeSide] = useState<OutcomeSide>("YES");
  const [unit, setUnit] = useState<Unit>("usd");
  const [amount, setAmount] = useState("25");
  const [shares, setShares] = useState("100");
  const [ttl, setTtl] = useState<Ttl>("until cancel");

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
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [submitOk, setSubmitOk] = useState<string | null>(null);

  // Any edit in the trader section makes the last submit's receipt stale — drop
  // the "queued …" confirmation (and any prior error) so it doesn't linger while
  // the user lines up a different order (e.g. flips to Sell NO).
  /* eslint-disable react-hooks/set-state-in-effect -- clear stale receipt on edit */
  useEffect(() => {
    setSubmitOk(null);
    setSubmitError(null);
  }, [dir, outcomeSide, unit, amount, shares, ttl, limit]);
  /* eslint-enable react-hooks/set-state-in-effect */

  const limitDec = Math.max(1, Math.min(99, limit)) / 100;
  const usd = parseFloat(amount) || 0;
  const sh = parseFloat(shares) || 0;
  const sharesIfUsd = limitDec > 0 ? usd / limitDec : 0;
  const maxCostIfShares = sh * limitDec;

  // Unified receipt quantities. `qtyShares` is the order size the engine will
  // see (`max_fill`), floored to whole shares exactly like onCtaClick — so the
  // receipt matches what's actually submitted in all four modes (buy/sell ×
  // $/shares). `grossAtLimit` is the cash value of that order at the limit
  // price; `payoutIfWin` is what those shares return ($1 each) if in-the-money.
  const qtyShares = Math.max(
    0,
    Math.floor(unit === "usd" ? sharesIfUsd : sh),
  );
  const grossAtLimit = qtyShares * limitDec;
  const payoutIfWin = qtyShares; // qty × $1

  // Cash available to BUY = balance − cash reserved by resting buy orders.
  // (Sells are gated by held shares below, not cash.) Matches the engine so a
  // buy MAX / headroom never proposes more than will be accepted.
  const availableDollars =
    availableNanos == null ? null : Number(availableNanos) / 1e9;

  // Shares of THIS outcome+side the user currently holds — what they can sell.
  // Positions carry the outcome as "YES"/"NO" (accounts route), matching
  // `outcomeSide`.
  const heldShares =
    portfolio.data?.positions?.find(
      (p) => p.market_id === outcome.marketId && p.outcome === outcomeSide,
    )?.quantity ?? 0;

  // Quick-amount chips on the order input. `+N` is additive; `MAX` fills the
  // available balance (needs a known balance). Mirrors the handoff BuyBox.
  // On a sell, MAX means "all the shares you hold"; on a buy it fills the
  // available cash balance (as $ or balance/limit shares).
  const quickChips: { label: string; disabled?: boolean; apply: () => void }[] =
    unit === "usd"
      ? [
          { label: "+10", apply: () => setAmount(String((parseFloat(amount) || 0) + 10)) },
          { label: "+50", apply: () => setAmount(String((parseFloat(amount) || 0) + 50)) },
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
          { label: "+10", apply: () => setShares(String((parseFloat(shares) || 0) + 10)) },
          { label: "+100", apply: () => setShares(String((parseFloat(shares) || 0) + 100)) },
          dir === "sell"
            ? {
                label: "MAX",
                disabled: heldShares <= 0,
                apply: () => setShares(String(heldShares)),
              }
            : {
                label: "MAX",
                disabled: availableDollars == null,
                apply: () => {
                  if (availableDollars != null)
                    setShares(String(Math.floor(availableDollars / limitDec)));
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
  const buyCostDollars = unit === "usd" ? usd : maxCostIfShares;
  const insufficientBuy =
    connected &&
    dir === "buy" &&
    availableDollars != null &&
    buyCostDollars > availableDollars;
  // Block a SELL of more shares than held — the mirror of insufficientBuy — so
  // we never trip a server-side InsufficientPosition. Only enforced once the
  // portfolio has loaded (heldShares defaults to 0 while that query is pending,
  // which would otherwise reject every sell).
  const positionsLoaded = portfolio.data != null;
  const insufficientSell =
    connected && dir === "sell" && positionsLoaded && qtyShares > heldShares;
  const ctaOff = submitting || insufficientBuy || insufficientSell;

  const ctaLabel = (() => {
    if (!connected) return "Connect to trade";
    if (submitting) return "Signing…";
    if (insufficientBuy) return "Not enough funds";
    if (insufficientSell) return "Not enough shares";
    const sideWord = dir === "buy" ? "queue buy" : "queue sell";
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
    setSubmitError(null);
    setSubmitOk(null);

    // Resolve qty (max_fill) and basic validation.
    const maxFill =
      unit === "usd"
        ? Math.max(0, Math.floor(sharesIfUsd))
        : Math.max(0, Math.floor(sh));
    if (maxFill < 1) {
      setSubmitError("max_fill must be at least 1 share");
      return;
    }
    const limitCents = Math.max(1, Math.min(99, Math.round(limit)));
    const limitPriceNanos = BigInt(limitCents) * 10_000_000n; // cents × 1e7

    let expiresAtBlock: bigint | undefined;
    if (ttl !== "until cancel") {
      if (latestHeight == null) {
        setSubmitError("waiting for latest block — try again in a moment");
        return;
      }
      const horizon = ttl === "1 batch" ? 1 : 5;
      expiresAtBlock = BigInt(latestHeight + horizon);
    }

    setSubmitting(true);
    try {
      const res = await submitSignedOrder({
        accountId: session.accountId,
        publicKeyHex: session.publicKeyHex,
        marketId: outcome.marketId,
        side: orderSideFor(dir, outcomeSide),
        limitPriceNanos,
        maxFill: BigInt(maxFill),
        ...(expiresAtBlock !== undefined ? { expiresAtBlock } : {}),
      });
      if (!res.accepted) {
        setSubmitError("server returned accepted=false");
      } else {
        setSubmitOk(
          `queued · ${maxFill} sh @ ${limitCents}¢ (${ttl})`,
        );
        // Refresh both per-account caches and the chain-wide pending list
        // (consumed by market-rail's pending feed).
        qc.invalidateQueries({
          queryKey: ["account", session.accountId, "orders"],
        });
        qc.invalidateQueries({
          queryKey: ["account", session.accountId, "portfolio"],
        });
        qc.invalidateQueries({ queryKey: ["orders", "pending"] });
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
                  {cents}¢
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
              ? `balance ${formatInt(heldShares)} sh`
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
            disabled={disabledInputs}
            style={{
              background: "transparent",
              border: 0,
              padding: 0,
              cursor: disabledInputs ? "not-allowed" : "pointer",
              color:
                limit === indicativeCents ? "var(--fg-3)" : "var(--accent)",
              fontFamily: "var(--font-mono)",
              fontSize: 10,
              textDecoration: "underline",
              textUnderlineOffset: 2,
            }}
          >
            set indicative {indicativeCents}¢
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
          min={1}
          max={99}
          value={limit}
          disabled={disabledInputs}
          onChange={(e) => setLimit(Number(e.target.value))}
          style={{
            width: "100%",
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
          <span>indicative {indicativeCents}¢</span>
          <span>99¢</span>
        </div>
      </div>

      {/* TTL */}
      <div>
        <Eyebrow>good for</Eyebrow>
        <div style={{ display: "flex", gap: 4, marginTop: 5 }}>
          {TTL_OPTS.map((t) => {
            const active = ttl === t;
            return (
              <button
                key={t}
                type="button"
                onClick={() => setTtl(t)}
                disabled={disabledInputs}
                style={{
                  flex: 1,
                  padding: "6px 0",
                  borderRadius: 3,
                  cursor: disabledInputs ? "not-allowed" : "pointer",
                  background: active ? "var(--surface-2)" : "var(--bg-2)",
                  border: `1px solid ${active ? "var(--border-3)" : "var(--border-1)"}`,
                  color: active ? "var(--fg-1)" : "var(--fg-3)",
                  fontFamily: "var(--font-mono)",
                  fontSize: 10,
                  opacity: disabledInputs ? 0.7 : 1,
                }}
              >
                {t}
              </button>
            );
          })}
        </div>
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
        {dir === "buy" ? (
          <>
            {/* Buy: pay AT MOST limit×qty (uniform clearing may be cheaper),
                receive qty shares, each worth $1 if the outcome resolves in. */}
            <ReceiptRow label="max cost" value={`≤ $${grossAtLimit.toFixed(2)}`} />
            <ReceiptRow label="shares (if filled)" value={`${qtyShares}`} />
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
            <ReceiptRow label="shares to sell" value={`${qtyShares}`} />
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

      {/* CTA */}
      <button
        type="button"
        onClick={onCtaClick}
        disabled={ctaOff}
        style={{
          marginTop: 2,
          padding: "12px 0",
          border: 0,
          borderRadius: 4,
          cursor: ctaOff ? "not-allowed" : "pointer",
          background: connected ? accent : "var(--accent)",
          color: "#0A0E12",
          fontFamily: "var(--font-sans)",
          fontSize: 14,
          fontWeight: 600,
          letterSpacing: "0.01em",
          opacity: ctaOff ? 0.55 : 1,
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
            border:
              "1px solid color-mix(in srgb, var(--no) 32%, transparent)",
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
      {submitOk && !submitError && (
        <div
          style={{
            padding: "6px 10px",
            background: "color-mix(in srgb, var(--yes) 12%, transparent)",
            border:
              "1px solid color-mix(in srgb, var(--yes) 32%, transparent)",
            borderRadius: 4,
            color: "var(--yes)",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
          }}
        >
          {submitOk}
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
