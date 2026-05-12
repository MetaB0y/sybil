"use client";

/**
 * Editorial hero for the Activity page: giant matched-volume number on the
 * left, 4-cell stat grid on the right.
 *
 * Most fields are mocked (see OPEN_QUESTIONS #3) — the live ones are
 * `totalBatches` (from latestBlock.height) and `liveMarkets` (from
 * /v1/markets/summary). Mocked values render with a <MockValue> wrap.
 */

import { MockValue } from "@/components/mock-value";
import { formatInt } from "@/lib/format/nanos";
import type { AllTimeStats } from "@/lib/activity/types";

export function HeroAllTime({ allTime }: { allTime: AllTimeStats }) {
  return (
    <section
      style={{
        padding: "28px 24px 28px",
        borderBottom: "1px solid var(--border-1)",
        position: "relative",
      }}
    >
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(0, 1.1fr) minmax(0, 1fr)",
          gap: 48,
          alignItems: "start",
        }}
      >
        {/* Left: hero number */}
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span className="eyebrow">All-time matched volume</span>
            <span className="text-annotation" style={{ fontSize: 11 }}>
              since genesis ·{" "}
              <MockValue hint="genesis age — backend doesn't track this yet">
                {allTime.genesisAge}
              </MockValue>
            </span>
          </div>
          <div
            style={{
              fontFamily: "var(--font-sans)",
              fontWeight: 600,
              fontSize: "clamp(48px, 5.4vw, 80px)",
              lineHeight: 0.95,
              letterSpacing: "-0.02em",
              color: "var(--fg-1)",
              fontVariantNumeric: "tabular-nums",
            }}
          >
            <MockValue hint="all-time matched volume — needs /v1/activity/overview backend endpoint (OPEN_QUESTIONS #3)">
              {allTime.matchedVolume}
            </MockValue>
          </div>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 14,
              paddingTop: 6,
            }}
          >
            <span
              style={{
                fontFamily: "var(--font-mono)",
                fontSize: 11,
                color: "var(--fg-3)",
                textTransform: "uppercase",
                letterSpacing: "0.04em",
              }}
            >
              {formatInt(allTime.totalBatches)} batches ·{" "}
              {formatInt(allTime.liveMarkets)} live markets · uptime{" "}
              <span style={{ color: "var(--yes)" }}>
                <MockValue hint="uptime % — not tracked on backend, leaving as decision">
                  {allTime.uptime}
                </MockValue>
              </span>
            </span>
          </div>
        </div>

        {/* Right: 2x2 stat grid */}
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "1fr 1fr",
            columnGap: 36,
            rowGap: 22,
            alignSelf: "start",
            paddingTop: 24,
          }}
        >
          <BigKv
            label="Active traders"
            value={formatInt(allTime.traders)}
            sub="addresses placed ≥1 order"
            mocked
            mockHint="unique-traders-all-time — backend ask (OPEN_QUESTIONS #3)"
          />
          <BigKv
            label="Placed orders"
            value={formatInt(allTime.ordersPlaced)}
            sub="across all batches"
            mocked
            mockHint="orders all-time — backend ask (OPEN_QUESTIONS #3)"
          />
          <BigKv
            label="Matched orders"
            value={formatInt(allTime.ordersMatched)}
            sub="successfully filled at clear"
            accent="var(--yes)"
            mocked
            mockHint="orders all-time — backend ask (OPEN_QUESTIONS #3)"
          />
          <BigKv
            label="Unmatched orders"
            value={formatInt(allTime.ordersUnmatched)}
            sub="cancelled or expired"
            accent="var(--fg-2)"
            mocked
            mockHint="orders all-time — backend ask (OPEN_QUESTIONS #3)"
          />
        </div>
      </div>
    </section>
  );
}

function BigKv({
  label,
  value,
  sub,
  accent = "var(--fg-1)",
  mocked = false,
  mockHint = "",
}: {
  label: string;
  value: string;
  sub: string;
  accent?: string;
  mocked?: boolean;
  mockHint?: string;
}) {
  const numberEl = (
    <span
      style={{
        fontFamily: "var(--font-sans)",
        fontSize: 30,
        fontWeight: 600,
        color: accent,
        fontVariantNumeric: "tabular-nums",
        letterSpacing: "-0.01em",
        lineHeight: 1,
      }}
    >
      {value}
    </span>
  );
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}>
      <span className="eyebrow">{label}</span>
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: 12,
          justifyContent: "space-between",
        }}
      >
        {mocked ? <MockValue hint={mockHint}>{numberEl}</MockValue> : numberEl}
      </div>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 10,
          color: "var(--fg-3)",
          textTransform: "uppercase",
          letterSpacing: "0.04em",
        }}
      >
        {sub}
      </span>
    </div>
  );
}
