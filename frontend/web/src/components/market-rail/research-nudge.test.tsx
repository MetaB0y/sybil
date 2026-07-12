import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ResearchNudge } from "./research-nudge";

describe("ResearchNudge", () => {
  it("marks its standalone research link as a coarse-pointer action", () => {
    const html = renderToStaticMarkup(<ResearchNudge />);

    expect(html).toContain('class="mobile-action-link"');
    expect(html).toContain('target="_blank"');
  });
});
