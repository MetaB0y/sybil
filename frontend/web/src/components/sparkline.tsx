"use client";

import { useMemo } from "react";
import { parseNanos } from "@/lib/format/nanos";
import type { PricePoint } from "@/lib/markets/use-card-history";

type Props = {
  points: PricePoint[];
  width?: number;
  height?: number;
  /** "auto" picks --yes when trending up, --no when trending down. */
  tone?: "yes" | "no" | "auto";
};

/**
 * Inline SVG area sparkline driven by yes_price_nanos. Pure presentational —
 * doesn't fetch, doesn't subscribe. Hand it a points array.
 *
 * With fewer than 2 points (no history yet) it draws a neutral flat baseline so
 * the slot reads as "no movement" rather than an empty gap.
 */
export function Sparkline({
  points,
  width = 120,
  height = 36,
  tone = "auto",
}: Props) {
  const path = useMemo(() => {
    if (points.length < 2) return null;
    const values: number[] = points.map((p) =>
      Number(parseNanos(p.yes_price_nanos ?? 0))
    );
    const first = values[0]!;
    const last = values[values.length - 1]!;
    const min = Math.min(...values);
    const max = Math.max(...values);
    const range = max - min || 1;
    const stepX = width / (values.length - 1);
    const pad = 2;
    const h = height - pad * 2;
    const toY = (v: number) => pad + h - ((v - min) / range) * h;

    let line = `M 0 ${toY(first).toFixed(2)}`;
    for (let i = 1; i < values.length; i++) {
      line += ` L ${(i * stepX).toFixed(2)} ${toY(values[i]!).toFixed(2)}`;
    }

    const area = `${line} L ${width} ${height} L 0 ${height} Z`;
    const direction: "yes" | "no" = last >= first ? "yes" : "no";
    return { line, area, direction };
  }, [points, width, height]);

  if (!path) {
    // No history yet → flat baseline at mid-height, neutral tone (there's no
    // up/down direction to color).
    const midY = height / 2;
    return (
      <svg
        width={width}
        height={height}
        viewBox={`0 0 ${width} ${height}`}
        aria-hidden
        style={{ display: "block" }}
      >
        <line
          x1={0}
          y1={midY}
          x2={width}
          y2={midY}
          stroke="var(--fg-4)"
          strokeWidth={1.5}
          strokeLinecap="round"
        />
      </svg>
    );
  }

  const effectiveTone = tone === "auto" ? path.direction : tone;
  const stroke =
    effectiveTone === "yes" ? "var(--yes)" : "var(--no)";
  const fill =
    effectiveTone === "yes" ? "var(--yes-faint)" : "var(--no-faint)";

  return (
    <svg
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      aria-hidden
      style={{ display: "block" }}
    >
      <path d={path.area} fill={fill} opacity={0.6} />
      <path
        d={path.line}
        fill="none"
        stroke={stroke}
        strokeWidth={1.5}
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  );
}
