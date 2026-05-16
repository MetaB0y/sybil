"use client";

/**
 * Degen rail amount input + quick-add chips + "to win up to" readout.
 * Matches `DegenAmount` in `fed-right-rail-modes.jsx:187`.
 *
 * Payout math is real:
 *   For YES at p¢: pay p, win 100 → multiplier = 100/p
 *   For NO at (100-p)¢: same shape with the complement.
 * `priceCents` here is the YES-side cents; we flip for NO.
 */

import type { Side } from "./yes-no-toggle";

const CHIPS = [10, 25, 100, 500] as const;

export function DegenAmount({
  amount,
  setAmount,
  yesPriceCents,
  side,
}: {
  amount: string;
  setAmount: (a: string) => void;
  yesPriceCents: number | null;
  side: Side;
}) {
  const effPrice =
    yesPriceCents == null
      ? null
      : side === "YES"
        ? yesPriceCents
        : 100 - yesPriceCents;
  const mult = effPrice != null && effPrice > 0 ? 100 / effPrice : null;
  const num = parseFloat(amount) || 0;
  const win = mult == null ? null : num * mult;

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

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(4, 1fr)",
          gap: 6,
        }}
      >
        {CHIPS.map((c) => (
          <button
            key={c}
            type="button"
            onClick={() => setAmount(String(c))}
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
            {win == null ? "—" : `$${win.toFixed(4)}`}
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
