"use client";

/**
 * Unified history feed — the single History tab (Activity merged in).
 *
 * Reverse-chronological event log grouped by day, with category filter chips.
 * Renders the normalized `HistoryEvent` model from `useAccountHistory`. The data
 * is mocked until the backend `/events` endpoint lands (OPEN: see
 * docs/superpowers/specs/2026-05-21-portfolio-history-feed-design.md), so the
 * whole feed wears a MockValue banner.
 */

import Link from "next/link";
import { useMemo, useState } from "react";
import { MockValue } from "@/components/mock-value";
import {
  CATEGORY_OF,
  type HistoryCategory,
  type HistoryEvent,
  type HistoryEventType,
} from "@/lib/account/use-account-history";
import { formatCents, formatDollars } from "@/lib/format/nanos";
import type { components } from "@/lib/api/schema";
import { SidePill } from "./side-pill";

type Market = components["schemas"]["MarketResponse"];

interface Props {
  events: HistoryEvent[];
  marketsById: Map<number, Market>;
  isMock?: boolean;
}

const CHIPS: { id: HistoryCategory; label: string }[] = [
  { id: "all", label: "All" },
  { id: "trades", label: "Trades" },
  { id: "funding", label: "Funding" },
  { id: "settlement", label: "Settlement" },
];

export function HistoryFeed({ events, marketsById, isMock }: Props) {
  const [category, setCategory] = useState<HistoryCategory>("all");

  const filtered = useMemo(() => {
    const rows =
      category === "all"
        ? events
        : events.filter((e) => CATEGORY_OF[e.type] === category);
    return [...rows].sort((a, b) => b.timestampMs - a.timestampMs);
  }, [events, category]);

  const days = useMemo(() => groupByDay(filtered), [filtered]);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-3)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <div style={{ display: "flex", gap: 6 }}>
          {CHIPS.map((c) => (
            <Chip
              key={c.id}
              label={c.label}
              active={category === c.id}
              onClick={() => setCategory(c.id)}
            />
          ))}
        </div>
        {isMock && (
          <span style={{ marginLeft: "auto" }}>
            <MockValue
              hint="history feed is mocked; pending backend /events endpoint (per-account event log)"
              variant="pill"
            >
              {" "}
            </MockValue>
          </span>
        )}
      </div>

      {days.length === 0 ? (
        <Empty>No history yet.</Empty>
      ) : (
        <div
          style={{
            background: "var(--surface-1)",
            border: "1px solid var(--border-1)",
            borderRadius: 6,
            overflow: "hidden",
          }}
        >
          {days.map(({ label, rows }) => (
            <div key={label}>
              <DayDivider label={label} />
              {rows.map((e) => (
                <EventRow key={e.id} event={e} market={marketsById.get(e.marketId ?? -1)} />
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function EventRow({ event, market }: { event: HistoryEvent; market: Market | undefined }) {
  const marketName = market?.name ?? (event.marketId != null ? `#${event.marketId}` : "");
  const time = new Date(event.timestampMs).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });

  const body = (
    <>
      <span style={{ color: "var(--fg-4)", fontFamily: "var(--font-mono)", fontSize: 11 }}>
        {time}
      </span>
      <TypeBadge type={event.type} side={event.side} />
      <Description event={event} marketName={marketName} />
      <RightCell mono>{priceLabel(event)}</RightCell>
      <AmountCell event={event} />
      <RightCell mono>
        <span style={{ color: "var(--accent)" }}>
          #{event.blockHeight.toLocaleString()}
        </span>
      </RightCell>
    </>
  );

  const style: React.CSSProperties = {
    ...rowGrid(),
    borderTop: "1px solid var(--border-1)",
  };

  if (event.marketId != null) {
    return (
      <Link
        href={`/m/${event.marketId}`}
        style={{ ...style, textDecoration: "none", color: "inherit" }}
      >
        {body}
      </Link>
    );
  }
  return <div style={style}>{body}</div>;
}

function Description({
  event,
  marketName,
}: {
  event: HistoryEvent;
  marketName: string;
}) {
  const qty = event.qty ?? 0;
  let text: React.ReactNode;
  switch (event.type) {
    case "created":
      text = "Account created";
      break;
    case "deposit":
      text = "Deposit";
      break;
    case "withdrawal":
      text = "Withdrawal";
      break;
    case "resolved":
      text = (
        <>
          {marketName} <span style={{ color: "var(--fg-4)" }}>resolved →</span>{" "}
          {event.payoutOutcome ?? ""}
        </>
      );
      break;
    case "placed":
    case "filled":
      text = (
        <>
          {event.side} {qty} {marketName}
        </>
      );
      break;
    case "partial_fill":
      text = (
        <>
          +{qty} {marketName} <span style={{ color: "var(--fg-4)" }}>(partial)</span>
        </>
      );
      break;
    case "cancelled":
      text = (
        <>
          {qty} returned · {marketName}
        </>
      );
      break;
    case "expired":
      text = (
        <>
          {qty} expired · {marketName}
        </>
      );
      break;
  }

  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        minWidth: 0,
      }}
    >
      {event.outcome && <SidePill outcome={event.outcome} />}
      <span
        style={{
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          color: "var(--fg-1)",
          fontFamily: "var(--font-sans)",
          fontSize: 13,
        }}
      >
        {text}
      </span>
    </span>
  );
}

function priceLabel(event: HistoryEvent): string {
  if (event.priceNanos != null && ["placed", "partial_fill", "filled"].includes(event.type)) {
    return formatCents(event.priceNanos);
  }
  return "—";
}

function AmountCell({ event }: { event: HistoryEvent }) {
  const bold = ["deposit", "withdrawal", "partial_fill", "filled", "resolved"];
  if (event.amountNanos != null && event.amountNanos !== 0n && bold.includes(event.type)) {
    const positive = event.amountNanos > 0n;
    return (
      <RightCell mono>
        <span style={{ color: positive ? "var(--yes)" : "var(--no)" }}>
          {formatDollars(event.amountNanos, { decimals: 2, sign: true })}
        </span>
      </RightCell>
    );
  }
  // placed (reserved) / cancelled / expired / created → muted reserve or —
  if (event.type === "placed" && event.priceNanos != null && event.qty != null) {
    const reserved = event.priceNanos * BigInt(event.qty);
    return (
      <RightCell mono>
        <span style={{ color: "var(--fg-4)" }} title="reserved margin">
          {formatDollars(reserved, { decimals: 2 })}
        </span>
      </RightCell>
    );
  }
  return (
    <RightCell mono>
      <span style={{ color: "var(--fg-4)" }}>—</span>
    </RightCell>
  );
}

function TypeBadge({
  type,
  side,
}: {
  type: HistoryEventType;
  side?: "BUY" | "SELL" | undefined;
}) {
  const { label, tone } = badgeMeta(type, side);
  const palette: Record<string, { fg: string; bg: string }> = {
    yes: { fg: "var(--yes)", bg: "color-mix(in srgb, var(--yes) 14%, transparent)" },
    no: { fg: "var(--no)", bg: "color-mix(in srgb, var(--no) 14%, transparent)" },
    accent: {
      fg: "var(--accent)",
      bg: "color-mix(in srgb, var(--accent) 14%, transparent)",
    },
    muted: { fg: "var(--fg-3)", bg: "rgba(255,255,255,0.04)" },
  };
  const c = palette[tone]!;
  return (
    <span
      style={{
        justifySelf: "start",
        padding: "1px 7px",
        background: c.bg,
        color: c.fg,
        borderRadius: 3,
        fontFamily: "var(--font-mono)",
        fontSize: 9.5,
        fontWeight: 600,
        letterSpacing: "var(--track-wide)",
        whiteSpace: "nowrap",
      }}
    >
      {label}
    </span>
  );
}

function badgeMeta(
  type: HistoryEventType,
  side?: "BUY" | "SELL" | undefined,
): { label: string; tone: "yes" | "no" | "accent" | "muted" } {
  switch (type) {
    case "created":
      return { label: "CREATED", tone: "muted" };
    case "placed":
      return { label: "PLACED", tone: "accent" };
    case "partial_fill":
      return { label: "PARTIAL", tone: side === "SELL" ? "no" : "yes" };
    case "filled":
      return { label: "FILLED", tone: side === "SELL" ? "no" : "yes" };
    case "cancelled":
      return { label: "CANCELLED", tone: "muted" };
    case "expired":
      return { label: "EXPIRED", tone: "muted" };
    case "deposit":
      return { label: "DEPOSIT", tone: "yes" };
    case "withdrawal":
      return { label: "WITHDRAWAL", tone: "no" };
    case "resolved":
      return { label: "RESOLVED", tone: "accent" };
  }
}

function DayDivider({ label }: { label: string }) {
  return (
    <div
      style={{
        padding: "8px 14px",
        background: "var(--bg-2)",
        color: "var(--fg-4)",
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
      }}
    >
      {label}
    </div>
  );
}

function Chip({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        padding: "4px 12px",
        background: active ? "var(--accent)" : "transparent",
        border: active ? 0 : "1px solid var(--border-1)",
        borderRadius: 999,
        color: active ? "var(--bg-1)" : "var(--fg-3)",
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        fontWeight: active ? 600 : 500,
        letterSpacing: "var(--track-wide)",
        cursor: "pointer",
      }}
    >
      {label}
    </button>
  );
}

function rowGrid(): React.CSSProperties {
  return {
    display: "grid",
    gridTemplateColumns: "52px 96px minmax(0, 1fr) 56px 96px 80px",
    gap: 10,
    alignItems: "center",
    padding: "9px 14px",
    color: "var(--fg-2)",
    fontFamily: "var(--font-mono)",
    fontSize: 11,
    letterSpacing: "var(--track-wide)",
  };
}

function RightCell({ children, mono }: { children: React.ReactNode; mono?: boolean }) {
  return (
    <span
      style={{
        textAlign: "right",
        fontFamily: mono ? "var(--font-mono)" : "inherit",
        fontSize: mono ? 12 : undefined,
        whiteSpace: "nowrap",
      }}
    >
      {children}
    </span>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        padding: "32px 16px",
        background: "var(--surface-1)",
        border: "1px dashed var(--border-1)",
        borderRadius: 6,
        color: "var(--fg-4)",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        textAlign: "center",
      }}
    >
      {children}
    </div>
  );
}

// ---- day grouping ---------------------------------------------------------

function groupByDay(
  rows: HistoryEvent[],
): { label: string; rows: HistoryEvent[] }[] {
  const out: { label: string; rows: HistoryEvent[] }[] = [];
  let current: { key: string; label: string; rows: HistoryEvent[] } | null = null;
  for (const e of rows) {
    const d = new Date(e.timestampMs);
    const key = d.toDateString();
    if (!current || current.key !== key) {
      current = { key, label: dayLabel(d), rows: [] };
      out.push({ label: current.label, rows: current.rows });
    }
    current.rows.push(e);
  }
  return out;
}

function dayLabel(d: Date): string {
  const today = new Date();
  const yesterday = new Date();
  yesterday.setDate(today.getDate() - 1);
  if (d.toDateString() === today.toDateString()) return "Today";
  if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
  return d.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    year: d.getFullYear() === today.getFullYear() ? undefined : "numeric",
  });
}
