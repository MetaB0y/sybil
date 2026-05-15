import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import { RestartCaveatBadge } from "./restart-caveat-badge";

describe("RestartCaveatBadge", () => {
  it("renders the caveat label", () => {
    const html = renderToStaticMarkup(<RestartCaveatBadge />);
    expect(html).toContain("since last restart");
  });

  it("default tooltip mentions in-memory tracker", () => {
    const html = renderToStaticMarkup(<RestartCaveatBadge />);
    expect(html).toMatch(/in-memory/i);
  });

  it("includes an extra hint when provided", () => {
    const html = renderToStaticMarkup(
      <RestartCaveatBadge hint="all-time trader count" />,
    );
    expect(html).toContain("all-time trader count");
  });

  it("exposes a screen-reader label", () => {
    const html = renderToStaticMarkup(<RestartCaveatBadge />);
    expect(html).toContain('aria-label="since last restart"');
  });
});
