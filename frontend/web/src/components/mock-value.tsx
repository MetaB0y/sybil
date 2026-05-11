"use client";

import type { CSSProperties, ReactNode } from "react";

type Props = {
  children: ReactNode;
  hint: string;
  style?: CSSProperties;
};

/**
 * Marks a rendered value as a frontend mock. Adds a faint dotted
 * underline + a tooltip so you can hover any suspicious number and see
 * "(mocked — backend field pending)".
 *
 * Remove the wrap (just render `children`) once backend exposes the
 * underlying field.
 */
export function MockValue({ children, hint, style }: Props) {
  return (
    <span
      title={`${hint} — mocked until backend exposes this field`}
      style={{
        borderBottom: "1px dotted color-mix(in srgb, var(--warn) 45%, transparent)",
        cursor: "help",
        ...style,
      }}
    >
      {children}
    </span>
  );
}
