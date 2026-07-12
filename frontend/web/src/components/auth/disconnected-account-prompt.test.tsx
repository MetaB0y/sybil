import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { DisconnectedAccountPrompt } from "./disconnected-account-prompt";

describe("DisconnectedAccountPrompt", () => {
  it("exposes the page message as its labelled level-one heading", () => {
    const html = renderToStaticMarkup(
      <DisconnectedAccountPrompt
        title="Connect to view your portfolio"
        message="Your key material stays on this device."
        onConnect={vi.fn()}
      />,
    );

    expect(html).toContain(
      '<section aria-labelledby="disconnected-account-title"',
    );
    expect(html).toContain('<h1 id="disconnected-account-title"');
    expect(html).toContain("Connect to view your portfolio</h1>");
    expect(html).toContain("min-height:44px");
  });
});
