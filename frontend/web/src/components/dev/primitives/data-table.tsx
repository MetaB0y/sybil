import type { CSSProperties, ReactNode } from "react";
import { type Tone, toneColor } from "./color-text";

export function DataTable({
  children,
  maxHeight,
  minWidth = 760,
}: {
  children?: ReactNode;
  maxHeight?: number | string;
  minWidth?: number;
}) {
  return (
    <div
      style={{
        overflow: "auto",
        border: "1px solid var(--border-2)",
        borderRadius: 8,
        ...(maxHeight !== undefined ? { maxHeight } : {}),
      }}
    >
      <table
        className="dev-data-table"
        style={{
          width: "100%",
          borderCollapse: "collapse",
          minWidth,
        }}
      >
        {children}
      </table>
    </div>
  );
}

export function Th({
  children,
  align = "left",
}: {
  children?: ReactNode;
  align?: "left" | "right";
}) {
  return (
    <th
      style={{
        position: "sticky",
        top: 0,
        zIndex: 1,
        background: "var(--surface-2)",
        color: "var(--fg-3)",
        fontWeight: 600,
        fontSize: 10,
        textTransform: "uppercase",
        letterSpacing: 0.4,
        padding: "7px 9px",
        borderBottom: "1px solid var(--border-2)",
        textAlign: align,
      }}
    >
      {children}
    </th>
  );
}

export function Td({
  children,
  tone,
  align = "left",
  mono = false,
}: {
  children?: ReactNode;
  tone?: Tone;
  align?: "left" | "right";
  mono?: boolean;
}) {
  const style: CSSProperties = {
    padding: "7px 9px",
    borderBottom: "1px solid var(--border-2)",
    verticalAlign: "top",
    textAlign: align,
  };
  if (tone) style.color = toneColor(tone);
  if (mono) {
    style.fontFamily = "var(--font-mono)";
    style.fontVariantNumeric = "tabular-nums";
    style.whiteSpace = "nowrap";
  }
  return <td style={style}>{children}</td>;
}
