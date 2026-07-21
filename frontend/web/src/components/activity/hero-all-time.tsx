"use client";

/**
 * Editorial hero for the Activity page: giant matched-volume number on the
 * left, 4-cell stat grid on the right.
 *
 * Real (GET /v1/activity/overview, `all_time` bucket): matched volume, active
 * traders, trader admissions, and trader first fills. Also real: `totalBatches`
 * (latestBlock.height) and `liveMarkets` (/v1/markets/summary).
 *
 * Trader execution deliberately excludes one-block MM quotes. An order counts
 * once when admitted and once when it receives its first positive fill.
 */

import { formatCompactInt, formatInt } from "@/lib/format/nanos";
import type { AllTimeStats } from "@/lib/activity/types";
import { Glossary } from "@/components/glossary";

export function HeroAllTime({
  allTime,
  botCount,
}: {
  allTime: AllTimeStats;
  botCount: number | null;
}) {
  return (
    <section
      className="activity-hero-section"
      style={{
        padding: "28px 24px 28px",
        borderTop: "1px solid var(--border-1)",
        borderBottom: "1px solid var(--border-1)",
        position: "relative",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: 14,
          paddingBottom: 18,
        }}
      >
        <h3
          style={{
            fontFamily: "var(--font-sans)",
            fontSize: 13,
            fontWeight: 600,
            margin: 0,
            color: "var(--fg-2)",
            textTransform: "uppercase",
            letterSpacing: "0.06em",
          }}
        >
          All time
        </h3>
        <span className="text-annotation" style={{ fontSize: 11 }}>
          since launch
        </span>
      </div>
      <div className="activity-hero-grid">
        {/* Left: two hero numbers — matched volume + welfare, same size */}
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <div className="activity-hero-numbers">
            <HeroNumber label="Matched volume" value={allTime.matchedVolume} />
            <HeroNumber
              label="Welfare"
              value={allTime.welfare}
              glossaryTerm="All-time welfare"
            />
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
              {allTime.totalBatches == null
                ? "—"
                : formatInt(allTime.totalBatches)}{" "}
              batches ·{" "}
              {allTime.liveMarkets == null
                ? "—"
                : formatInt(allTime.liveMarkets)}{" "}
              live markets
            </span>
          </div>
        </div>

        {/* Right: 2x2 stat grid */}
        <div className="activity-hero-stats">
          <BigKv
            label="Active traders"
            value={
              allTime.traders == null ? "—" : formatCompactInt(allTime.traders)
            }
            sub="addresses placed ≥1 order"
          />
          <BigKv
            label="Trader orders"
            value={
              allTime.traderOrdersAdmitted == null
                ? "—"
                : formatCompactInt(allTime.traderOrdersAdmitted)
            }
          />
          <BigKv
            label="Bots"
            value={botCount == null ? "—" : formatCompactInt(botCount)}
            sub="arena agents"
            accent="var(--accent)"
          />
          <BigKv
            label="Trader orders filled"
            value={
              allTime.traderOrdersFirstFilled == null
                ? "—"
                : formatCompactInt(allTime.traderOrdersFirstFilled)
            }
            accent="var(--yes)"
          />
        </div>
      </div>
    </section>
  );
}

function HeroNumber({
  label,
  value,
  glossaryTerm,
}: {
  label: string;
  value: string;
  /** When set, the eyebrow gets a "?" badge with the glossary definition. */
  glossaryTerm?: string;
}) {
  return (
    <div
      style={{ display: "flex", flexDirection: "column", gap: 10, minWidth: 0 }}
    >
      {glossaryTerm ? (
        <Glossary term={glossaryTerm}>
          <span className="eyebrow">{label}</span>
        </Glossary>
      ) : (
        <span className="eyebrow">{label}</span>
      )}
      <span
        className="activity-hero-value"
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
        {value}
      </span>
    </div>
  );
}

function BigKv({
  label,
  value,
  sub,
  accent = "var(--fg-1)",
}: {
  label: string;
  value: string;
  /** Optional gloss under the number; omitted where the label speaks for itself. */
  sub?: string;
  accent?: string;
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
        {numberEl}
      </div>
      {sub != null && (
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
      )}
    </div>
  );
}
