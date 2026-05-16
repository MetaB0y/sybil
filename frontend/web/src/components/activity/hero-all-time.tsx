"use client";

/**
 * Editorial hero for the Activity page: giant matched-volume number on the
 * left, 4-cell stat grid on the right.
 *
 * Real (GET /v1/activity/overview, `all_time` bucket): matched volume, active
 * traders, matched orders, unmatched orders. Also real: `totalBatches`
 * (latestBlock.height) and `liveMarkets` (/v1/markets/summary). Still mocked:
 * placed orders (the backend counter is batch-participation, not an order
 * count — see PLACED_HINT), genesis age, uptime — each wrapped in MockValue.
 */

import { MockValue } from "@/components/mock-value";
import { formatCompactInt, formatInt } from "@/lib/format/nanos";
import type { AllTimeStats } from "@/lib/activity/types";

const PLACED_HINT =
  "backend 'placed' counts per-batch participations, not distinct orders — pending a backend fix";

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
              <MockValue
                hint="genesis age — not tracked on backend"
                variant="underline"
              >
                {allTime.genesisAge}
              </MockValue>
            </span>
          </div>
          <span
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
            {allTime.matchedVolume}
          </span>
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
              <MockValue
                hint="uptime % — not tracked"
                variant="underline"
                style={{ color: "var(--yes)" }}
              >
                {allTime.uptime}
              </MockValue>
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
            value={
              allTime.traders == null
                ? "—"
                : formatCompactInt(allTime.traders)
            }
            sub="addresses placed ≥1 order"
          />
          <BigKv
            label="Placed orders"
            value={formatCompactInt(allTime.ordersPlaced)}
            sub="across all batches"
            mocked
          />
          <BigKv
            label="Matched orders"
            value={
              allTime.ordersMatched == null
                ? "—"
                : formatCompactInt(allTime.ordersMatched)
            }
            sub="successfully filled at clear"
            accent="var(--yes)"
          />
          <BigKv
            label="Unmatched orders"
            value={
              allTime.ordersUnmatched == null
                ? "—"
                : formatCompactInt(allTime.ordersUnmatched)
            }
            sub="expired without a fill"
            accent="var(--fg-2)"
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
}: {
  label: string;
  value: string;
  sub: string;
  accent?: string;
  mocked?: boolean;
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
    <div
      style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}
    >
      <span className="eyebrow">{label}</span>
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: 12,
          justifyContent: "space-between",
        }}
      >
        {mocked ? (
          <MockValue hint={PLACED_HINT} variant="pill">{numberEl}</MockValue>
        ) : (
          numberEl
        )}
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
