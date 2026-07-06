import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClientProvider, QueryClient } from "@tanstack/react-query";
import { SettingsView } from "./settings-view";

describe("SettingsView", () => {
  it("renders the three section headings before data loads", () => {
    const client = new QueryClient();
    const html = renderToStaticMarkup(
      <QueryClientProvider client={client}>
        <SettingsView accountId={7} publicKeyHex={"02" + "ab".repeat(32)} />
      </QueryClientProvider>,
    );
    expect(html).toContain("Profile");
    expect(html).toContain("Signing keys / agent keys");
    expect(html).toContain("Read API keys");
    // The read-only framing must be visible in copy.
    expect(html).toContain("cannot trade");
  });
});
