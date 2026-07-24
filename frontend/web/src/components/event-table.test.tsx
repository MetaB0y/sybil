import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  CancelButton,
  eventRowGrid,
  ROW_GAP,
  ROW_MIN_HEIGHT,
} from "./event-table";
import { valueChipStyle } from "./portfolio/side-pill";

describe("eventRowGrid", () => {
  // The three lists under the chart drifted to 10 / 14 / 18px gutters, so
  // switching tabs shifted every column sideways.
  it("gives every list the same gutter and padding", () => {
    const a = eventRowGrid("minmax(0, 1fr) 48px");
    const b = eventRowGrid("minmax(0, 1fr) 60px 60px");
    expect(a.gap).toBe(ROW_GAP);
    expect(b.gap).toBe(ROW_GAP);
    expect(a.padding).toBe(b.padding);
  });

  it("floors body rows so a chip-less row matches one with chips", () => {
    expect(eventRowGrid("1fr").minHeight).toBe(ROW_MIN_HEIGHT);
  });

  it("leaves the header unfloored and undivided", () => {
    const header = eventRowGrid("1fr", true);
    expect(header.minHeight).toBeUndefined();
    expect(header.borderTop).toBeUndefined();
    expect(eventRowGrid("1fr").borderTop).toBe("1px solid var(--border-1)");
  });

  it("passes each list's own columns through untouched", () => {
    expect(eventRowGrid("minmax(0, 1fr) 48px 62px").gridTemplateColumns).toBe(
      "minmax(0, 1fr) 48px 62px",
    );
  });
});

describe("CancelButton", () => {
  // The reported symptom: Open-orders rows sat taller than Holdings rows,
  // because Cancel was built at its own scale instead of the chips' scale.
  it("matches the tinted chips' box so it cannot inflate its row", () => {
    const chip = valueChipStyle({ color: "var(--no)", bg: "transparent" });
    const html = renderToStaticMarkup(
      <CancelButton cancelling={false} onClick={() => {}} />,
    );
    expect(html).toContain(`font-size:${chip.fontSize}px`);
    expect(html).toContain(`padding:${chip.padding}`);
    expect(html).toContain(`min-width:${chip.minWidth}px`);
    // A border would add 2px the chips don't have.
    expect(html).toContain("border:0");
  });

  it("disables itself and reads as busy while cancelling", () => {
    const html = renderToStaticMarkup(
      <CancelButton cancelling onClick={() => {}} />,
    );
    expect(html).toContain("disabled");
    expect(html).not.toContain("Cancel");
  });
});
