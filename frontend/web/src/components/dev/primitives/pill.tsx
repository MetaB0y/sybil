import type { CSSProperties, ReactNode } from "react";
import { type Tone, toneColor } from "./color-text";

export function Pill({
  children,
  tone,
}: {
  children?: ReactNode;
  tone?: Tone;
}) {
  const style: CSSProperties = {
    display: "inline-flex",
    alignItems: "center",
    padding: "2px 6px",
    borderRadius: 999,
    fontSize: 10,
    whiteSpace: "nowrap",
    border: "1px solid var(--border-2)",
    color: "var(--fg-3)",
  };
  if (tone) {
    const c = toneColor(tone);
    style.color = c;
    style.borderColor = `color-mix(in srgb, ${c} 35%, transparent)`;
  }
  return <span style={style}>{children}</span>;
}
