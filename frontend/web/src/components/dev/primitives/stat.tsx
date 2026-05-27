import type { CSSProperties, ReactNode } from "react";
import { type Tone, toneColor } from "./color-text";

export function Stat({
  label,
  value,
  sub,
  tone,
}: {
  label: string;
  value: ReactNode;
  sub?: ReactNode;
  tone?: Tone;
}) {
  return (
    <div
      style={{
        background: "var(--surface-1)",
        border: "1px solid var(--border-2)",
        borderRadius: 8,
        padding: "10px 11px",
        minHeight: 78,
      }}
    >
      <div
        style={{
          fontSize: 10,
          textTransform: "uppercase",
          color: "var(--fg-3)",
          letterSpacing: 0.4,
        }}
      >
        {label}
      </div>
      <div
        style={{
          marginTop: 4,
          fontSize: 21,
          fontWeight: 650,
          color: tone ? toneColor(tone) : "var(--fg-1)",
        }}
      >
        {value}
      </div>
      {sub !== undefined ? (
        <div style={{ marginTop: 4, fontSize: 11, color: "var(--fg-4)" }}>
          {sub}
        </div>
      ) : null}
    </div>
  );
}

export function StatGrid({
  columns,
  children,
  style,
}: {
  columns: number;
  children?: ReactNode;
  style?: CSSProperties;
}) {
  return (
    <div
      style={{
        display: "grid",
        gap: 12,
        gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
        ...style,
      }}
    >
      {children}
    </div>
  );
}
