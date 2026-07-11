import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClientProvider, QueryClient } from "@tanstack/react-query";
import {
  SettingsView,
  signingPublicKeysEqual,
  signingKeyRevocationPolicy,
} from "./settings-view";
import { settingsQueryKeys } from "@/lib/account/use-settings-data";

describe("SettingsView", () => {
  function render(
    authScheme: "raw_p256" | "webauthn",
    signingKeys?: Array<Record<string, unknown>>,
  ) {
    const client = new QueryClient();
    if (signingKeys) {
      client.setQueryData(settingsQueryKeys.signingKeys(7), signingKeys);
    }
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

  it("explains that the connected key must be rotated from another session", () => {
    const html = render("webauthn");
    expect(html).toContain(
      "disconnect and sign in with a different registered key first",
    );
    expect(html).toContain("current session key cannot revoke itself");
  });

  it("renders the prefixed current key as self and disables only its revoke button", () => {
    const sessionPublicKey = `02${"ab".repeat(32)}`;
    const html = render("webauthn", [
      {
        public_key_hex: `0X${sessionPublicKey.toUpperCase()}`,
        auth_scheme: "webauthn",
        scope: "primary",
        label: "current passkey",
        created_at_ms: 0,
      },
      {
        public_key_hex: `03${"cd".repeat(32)}`,
        auth_scheme: "webauthn",
        scope: "primary",
        label: "old passkey",
        created_at_ms: 0,
      },
    ]);

    expect(html).toContain("this key");
    expect(html).toMatch(
      /<button[^>]*disabled=""[^>]*title="Reconnect with another registered key before revoking this one"[^>]*>/,
    );
    expect(html).toMatch(
      /<button[^>]*title="Revoke this key"[^>]*>Revoke<\/button>/,
    );
  });
});

describe("signingKeyRevocationPolicy", () => {
  it("preserves last-key protection", () => {
    expect(signingKeyRevocationPolicy(1, true)).toEqual({
      canRevoke: false,
      title: "Cannot revoke the last remaining key",
    });
  });

  it("blocks the current session key even when a backup exists", () => {
    expect(signingKeyRevocationPolicy(2, true)).toEqual({
      canRevoke: false,
      title: "Reconnect with another registered key before revoking this one",
    });
  });

  it("allows another key to be revoked after rotation", () => {
    expect(signingKeyRevocationPolicy(2, false)).toEqual({
      canRevoke: true,
      title: "Revoke this key",
    });
  });
});

describe("signingPublicKeysEqual", () => {
  it("normalizes optional hex prefixes and case", () => {
    const publicKey = `02${"ab".repeat(32)}`;
    expect(
      signingPublicKeysEqual(publicKey, `0x${publicKey.toUpperCase()}`),
    ).toBe(true);
  });

  it("does not conflate different public keys", () => {
    expect(
      signingPublicKeysEqual(`02${"ab".repeat(32)}`, `03${"ab".repeat(32)}`),
    ).toBe(false);
  });
});
