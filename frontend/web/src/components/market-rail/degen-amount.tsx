"use client";

/**
 * Degen rail amount input + quick-add chips + "to win up to" readout.
 * Matches `DegenAmount` in `fed-right-rail-modes.jsx:187`.
 *
 * Payout is computed from the *actual* degen order the rail will submit
 * (`maxFill` shares at the degen-taxed limit price), not the indicative
 * clearing price — each share pays $1 if the bet wins, so the max payout is
 * `maxFill / 1000 × $1` and the multiplier is `payout / bet`. `maxFill` is
 * `null` when the bet is below the minimum 0.001 share.
 */

import { unitsToShares } from "@/lib/account/quantity";

const CHIPS = [10, 25, 100, 500] as const;

export function DegenAmount({
  amount,
  setAmount,
  maxFill,
  availableDollars,
  reservedDollars = 0,
  seeding = false,
}: {
  amount: string;
  setAmount: (a: string) => void;
  /** Share-units the built degen order will buy, or null when below minimum. */
  maxFill: bigint | null;
  /** Cash available to bet (balance − cash reserved by open orders), or null
   *  if unknown. This is what the engine checks, so MAX/headroom use it. */
  availableDollars: number | null;
  /** Cash reserved by resting buy orders, surfaced as a small hint. */
  reservedDollars?: number;
  /** Market has never traded (no price / history), so the mark is a neutral
   *  fallback. Show a "seed the book" note instead of a fabricated payout. */
  seeding?: boolean;
}) {
  const bet = parseFloat(amount) || 0;
  const win = maxFill == null ? null : unitsToShares(maxFill);
  const mult = win != null && bet > 0 ? win / bet : null;
  const add = (delta: number) =>
    setAmount(String(Math.round(((parseFloat(amount) || 0) + delta) * 100) / 100));

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          background: "var(--bg-2)",
          border: "1px solid var(--border-2)",
          borderRadius: 6,
          padding: "14px 16px",
        }}
      >
        <span
          style={{
            fontFamily: "var(--font-sans)",
            fontSize: 24,
            fontWeight: 500,
            color: "var(--fg-3)",
            lineHeight: 1,
          }}
        >
          $
        </span>
        <input
          type="text"
          inputMode="decimal"
          aria-label="Bet amount in dollars"
          value={amount}
          onChange={(e) => setAmount(e.target.value.replace(/[^0-9.]/g, ""))}
          placeholder="0"
          style={{
            flex: 1,
            minWidth: 0,
            background: "transparent",
            border: 0,
            outline: "none",
            color: "var(--fg-1)",
            fontFamily: "var(--font-sans)",
            fontSize: 30,
            fontWeight: 600,
            letterSpacing: "-0.01em",
            padding: 0,
            fontVariantNumeric: "tabular-nums",
          }}
        />
      </div>

      {/* Available-to-bet line — balance minus cash locked by open orders, so
          the headroom shown matches what the engine will actually accept. */}
      <div
        style={{
          display: "flex",
          justifyContent: "flex-end",
          fontFamily: "var(--font-mono)",
          fontSize: 10,
          color: "var(--fg-4)",
          marginTop: -4,
          minHeight: 12,
        }}
      >
        {availableDollars != null && (
          <span
            title={
              reservedDollars > 0
                ? `$${reservedDollars.toFixed(2)} reserved by your open orders`
                : undefined
            }
          >
            available ${availableDollars.toFixed(2)}
            {reservedDollars > 0 && (
              <span style={{ opacity: 0.7 }}>
                {" "}
                · ${reservedDollars.toFixed(2)} in orders
              </span>
            )}
          </span>
        )}
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(5, 1fr)",
          gap: 6,
        }}
      >
        {CHIPS.map((c) => (
          <button
            key={c}
            type="button"
            onClick={() => add(c)}
            style={{
              minHeight: 40,
              padding: "8px 0",
              background: "var(--bg-2)",
              border: "1px solid var(--border-1)",
              borderRadius: 4,
              color: "var(--fg-2)",
              fontFamily: "var(--font-mono)",
              fontSize: 11,
              cursor: "pointer",
            }}
          >
            +${c}
          </button>
        ))}
        <button
          type="button"
          disabled={availableDollars == null}
          onClick={() => {
            if (availableDollars != null) setAmount(availableDollars.toFixed(2));
          }}
          title={
            availableDollars == null
              ? "Connect to bet your full balance"
              : "Bet your full available balance"
          }
          style={{
            minHeight: 40,
            padding: "8px 0",
            background: "var(--bg-2)",
            border: "1px solid var(--border-1)",
            borderRadius: 4,
            color: availableDollars == null ? "var(--fg-4)" : "var(--accent)",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            fontWeight: 600,
            cursor: availableDollars == null ? "not-allowed" : "pointer",
            opacity: availableDollars == null ? 0.5 : 1,
          }}
        >
          MAX
        </button>
      </div>

      {seeding ? (
        <div
          style={{
            background: "var(--bg-2)",
            border: "1px solid var(--border-2)",
            borderRadius: 6,
            padding: "12px 14px",
            display: "flex",
            flexDirection: "column",
            gap: 4,
          }}
        >
          <span
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 9.5,
              color: "var(--fg-3)",
              textTransform: "uppercase",
              letterSpacing: "0.06em",
            }}
          >
            no price yet
          </span>
          <span
            style={{
              fontFamily: "var(--font-sans)",
              fontSize: 13,
              color: "var(--fg-2)",
              lineHeight: 1.35,
            }}
          >
            This market hasn&apos;t traded — your bet would seed the book. The
            payout can&apos;t be estimated until it clears.
          </span>
        </div>
      ) : (
        <div
          style={{
            background:
              "linear-gradient(135deg, color-mix(in srgb, var(--yes) 10%, transparent), color-mix(in srgb, var(--yes) 2%, transparent))",
            border: "1px solid color-mix(in srgb, var(--yes) 30%, transparent)",
            borderRadius: 6,
            padding: "12px 14px",
            display: "flex",
            alignItems: "baseline",
            justifyContent: "space-between",
            gap: 10,
          }}
        >
          <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
            <span
              style={{
                fontFamily: "var(--font-mono)",
                fontSize: 9.5,
                color: "var(--yes)",
                textTransform: "uppercase",
                letterSpacing: "0.06em",
              }}
            >
              to win up to
            </span>
            <span
              style={{
                fontFamily: "var(--font-sans)",
                fontSize: 22,
                fontWeight: 600,
                color: "var(--yes)",
                letterSpacing: "-0.01em",
                fontVariantNumeric: "tabular-nums",
              }}
            >
              {win == null ? "—" : `$${win.toFixed(2)}`}
            </span>
          </div>
          <span
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 14,
              fontWeight: 600,
              color: "var(--yes)",
              fontVariantNumeric: "tabular-nums",
            }}
          >
            {mult == null ? "—" : `${mult.toFixed(2)}×`}
          </span>
        </div>
      )}
    </div>
  );
}
