import type { CSSProperties } from "react";

/**
 * Canonical dev-zone form control — one source of truth for the `<select>` and
 * `<input>` chrome across the dev views.
 *
 * Each view previously kept its own near-identical `controlStyle`, and they had
 * drifted: the markets copy omitted `fontSize`, so its selects fell back to the
 * browser's native control size and rendered 14px/36px tall against 12px/33px
 * everywhere else. Import this instead of redeclaring it.
 *
 * Width is deliberately not set here — callers stretch to fill a filter row
 * (`fullWidthControl`) or pin a minimum for long option labels.
 */
export const controlStyle: CSSProperties = {
  border: "1px solid var(--border-2)",
  background: "var(--surface-1)",
  color: "var(--fg-1)",
  borderRadius: 6,
  padding: "7px 9px",
  fontFamily: "inherit",
  fontSize: 12,
  // Explicit height rather than padding alone: inputs and selects have
  // different intrinsic heights, so a filter row laid out on a stretch grid
  // took its height from whichever control happened to be tallest.
  height: 33,
  boxSizing: "border-box",
};

/** `controlStyle` for controls that fill their column. */
export const fullWidthControl: CSSProperties = {
  ...controlStyle,
  width: "100%",
};
