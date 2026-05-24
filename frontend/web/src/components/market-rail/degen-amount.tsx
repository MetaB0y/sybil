"use client";

/**
 * Degen rail amount input + quick-add chips + "to win up to" readout.
 * Matches `DegenAmount` in `fed-right-rail-modes.jsx:187`.
 *
 * Payout is computed from the *actual* degen order the rail will submit
 * (`maxFill` shares at the degen-taxed limit price), not the indicative
 * clearing price — each share pays $1 if the bet wins, so the max payout is
 * `maxFill × $1` and the multiplier is `payout / bet`. `maxFill` is `null`
 * when the bet is below the one-share minimum.
 */

const CHIPS = [10, 25, 100, 500] as const;

export function DegenAmount({
  amount,
  setAmount,
  maxFill,
  balanceDollars,
}: {
  amount: string;
  setAmount: (a: string) => void;
  /** Shares the built degen order will buy, or null when below minimum. */
  maxFill: bigint | null;
  /** Connected account's cash balance in dollars, or null if unknown. */
  balanceDollars: number | null;
}) {
  const bet = parseFloat(amount) || 0;
  const win = maxFill == null ? null : Number(maxFill);
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

      {/* Balance line — mirrors Pro mode so the bettor sees their headroom. */}
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
        {balanceDollars != null && <span>balance ${balanceDollars.toFixed(2)}</span>}
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
          disabled={balanceDollars == null}
          onClick={() => {
            if (balanceDollars != null) setAmount(balanceDollars.toFixed(2));
          }}
          title={
            balanceDollars == null
              ? "Connect to bet your full balance"
              : "Bet your full balance"
          }
          style={{
            padding: "8px 0",
            background: "var(--bg-2)",
            border: "1px solid var(--border-1)",
            borderRadius: 4,
            color: balanceDollars == null ? "var(--fg-4)" : "var(--accent)",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            fontWeight: 600,
            cursor: balanceDollars == null ? "not-allowed" : "pointer",
            opacity: balanceDollars == null ? 0.5 : 1,
          }}
        >
          MAX
        </button>
      </div>

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
    </div>
  );
}
