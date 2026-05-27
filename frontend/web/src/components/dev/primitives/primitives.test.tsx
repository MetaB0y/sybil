import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Panel, PanelHead } from "./panel";
import { Stat } from "./stat";
import { Pill } from "./pill";
import { toneColor } from "./color-text";

describe("dev primitives", () => {
  it("PanelHead renders its title", () => {
    const html = renderToStaticMarkup(
      <Panel><PanelHead title="Block Activity" /></Panel>
    );
    expect(html).toContain("Block Activity");
  });
  it("Stat renders label, value, and sub", () => {
    const html = renderToStaticMarkup(
      <Stat label="Markets" value="12" sub="3 cleared" />
    );
    expect(html).toContain("Markets");
    expect(html).toContain("12");
    expect(html).toContain("3 cleared");
  });
  it("Pill renders its children", () => {
    const html = renderToStaticMarkup(<Pill tone="yes">cleared</Pill>);
    expect(html).toContain("cleared");
  });
  it("toneColor maps tone names to css vars", () => {
    expect(toneColor("yes")).toBe("var(--yes)");
    expect(toneColor("no")).toBe("var(--no)");
    expect(toneColor("accent")).toBe("var(--accent)");
    expect(toneColor("dim")).toBe("var(--fg-4)");
  });
});
