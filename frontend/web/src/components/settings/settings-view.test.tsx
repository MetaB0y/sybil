import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClientProvider, QueryClient } from "@tanstack/react-query";
import { SettingsView } from "./settings-view";

describe("SettingsView", () => {
  function render(authScheme: "raw_p256" | "webauthn") {
    const client = new QueryClient();
    return renderToStaticMarkup(
      <QueryClientProvider client={client}>
        <SettingsView
          accountId={7}
          publicKeyHex={"02" + "ab".repeat(32)}
          authScheme={authScheme}
          {...(authScheme === "webauthn"
            ? { credentialIdB64url: "primary-credential" }
            : {})}
        />
      </QueryClientProvider>,
    );
  }

  it("renders the three section headings before data loads", () => {
    const html = render("raw_p256");
    expect(html).toContain("Profile");
    expect(html).toContain("Signing keys / agent keys");
    expect(html).toContain("Read API keys");
    // The read-only framing must be visible in copy.
    expect(html).toContain("cannot trade");
  });

  it("offers recovery passkey setup only for passkey sessions", () => {
    const passkeyHtml = render("webauthn");
    expect(passkeyHtml).toContain("Recovery passkey");
    expect(passkeyHtml).toContain("Add backup passkey");

    const rawHtml = render("raw_p256");
    expect(rawHtml).not.toContain("Recovery passkey");
    expect(rawHtml).not.toContain("Add backup passkey");
  });
});
