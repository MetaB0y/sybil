import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClientProvider, QueryClient } from "@tanstack/react-query";
import {
  Field,
  SettingsView,
  settingsActionMessage,
  signingPublicKeysEqual,
  signingKeyRevocationPolicy,
  writeShowOnceSecret,
} from "./settings-view";
import { SettingsActionError } from "@/lib/account/settings";
import { settingsQueryKeys } from "@/lib/account/use-settings-data";

describe("Field", () => {
  it("programmatically associates its visible text with the form control", () => {
    const html = renderToStaticMarkup(
      <Field label="Display name">
        <input type="text" />
      </Field>,
    );

    expect(html).toMatch(
      /^<label[^>]*>[\s\S]*Display name[\s\S]*<input[^>]*\/>[\s\S]*<\/label>$/,
    );
  });
});

describe("writeShowOnceSecret", () => {
  it("reports success only after the clipboard write resolves", async () => {
    let written = "";
    const copied = await writeShowOnceSecret("sybk_once", {
      writeText: async (value) => {
        written = value;
      },
    });

    expect(copied).toBe(true);
    expect(written).toBe("sybk_once");
  });

  it("falls back to manual selection when clipboard access is missing or rejects", async () => {
    await expect(writeShowOnceSecret("secret", undefined)).resolves.toBe(false);
    await expect(
      writeShowOnceSecret("secret", {
        writeText: async () => {
          throw new DOMException("clipboard denied", "NotAllowedError");
        },
      }),
    ).resolves.toBe(false);
  });
});

describe("settingsActionMessage", () => {
  it("only labels the actual last-key conflict as last-key protection", () => {
    expect(
      settingsActionMessage(
        new SettingsActionError(
          "revoke_key failed (HTTP 409): Cannot revoke the account's last remaining signing key",
          409,
        ),
      ),
    ).toContain("Cannot revoke the last key");
  });

  it("gives retry guidance for stale bindings and replay nonces", () => {
    expect(
      settingsActionMessage(
        new SettingsActionError(
          "register_agent_key failed (HTTP 409): stale key-operation state binding for account 7",
          409,
        ),
      ),
    ).toContain("key state changed");
    expect(
      settingsActionMessage(
        new SettingsActionError(
          "set_profile failed (HTTP 409): stale replay nonce for account 7: nonce 41 must be greater than 42",
          409,
        ),
      ),
    ).toContain("account changed");
  });

  it("preserves unknown conflicts instead of inventing last-key copy", () => {
    const error = new SettingsActionError(
      "create_api_key failed (HTTP 409): another conflict",
      409,
    );
    expect(settingsActionMessage(error)).toBe(error.message);
  });
});

describe("SettingsView", () => {
  const currentPublicKey = `02${"ab".repeat(32)}`;
  const currentSigningKey = {
    public_key_hex: currentPublicKey,
    auth_scheme: "webauthn",
    scope: "primary",
    label: "current passkey",
    created_at_ms: 0,
  };

  function renderWithClient(
    client: QueryClient,
    authScheme: "raw_p256" | "webauthn",
  ) {
    return renderToStaticMarkup(
      <QueryClientProvider client={client}>
        <SettingsView
          accountId={7}
          publicKeyHex={currentPublicKey}
          authScheme={authScheme}
          {...(authScheme === "webauthn"
            ? { credentialIdB64url: "primary-credential" }
            : {})}
        />
      </QueryClientProvider>,
    );
  }

  function render(
    authScheme: "raw_p256" | "webauthn",
    data: {
      profile?: Record<string, unknown>;
      signingKeys?: Array<Record<string, unknown>>;
      apiKeys?: Array<Record<string, unknown>>;
    } = {},
  ) {
    const client = new QueryClient();
    if (data.profile !== undefined) {
      client.setQueryData(settingsQueryKeys.profile(7), data.profile);
    }
    if (data.signingKeys !== undefined) {
      client.setQueryData(settingsQueryKeys.signingKeys(7), data.signingKeys);
    }
    if (data.apiKeys !== undefined) {
      client.setQueryData(settingsQueryKeys.apiKeys(7), data.apiKeys);
    }
    return renderWithClient(client, authScheme);
  }

  function successfulData(signingKeys = [currentSigningKey]) {
    return {
      profile: { display_name: null, avatar_seed: null },
      signingKeys,
      apiKeys: [],
    };
  }

  async function renderWithPrivateReadErrors() {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false, retryOnMount: false } },
    });
    const fail = async () => {
      throw new Error("private read failed");
    };
    await Promise.all([
      client.prefetchQuery({
        queryKey: settingsQueryKeys.profile(7),
        queryFn: fail,
      }),
      client.prefetchQuery({
        queryKey: settingsQueryKeys.signingKeys(7),
        queryFn: fail,
      }),
      client.prefetchQuery({
        queryKey: settingsQueryKeys.apiKeys(7),
        queryFn: fail,
      }),
    ]);
    return renderWithClient(client, "webauthn");
  }

  it("renders authoritative loading states without mutation controls", () => {
    const html = render("raw_p256");
    expect(html).toContain("Profile");
    expect(html).toContain("Signing keys / agent keys");
    expect(html).toContain("Read API keys");
    expect(html).toContain("Loading your current profile");
    expect(html).toContain("Loading the authoritative signing-key list");
    expect(html).toContain("Loading the authoritative read-key list");
    expect(html).not.toMatch(/>Save<|>Clear<|>Add agent key<|>Create API key</);
    expect(html).not.toContain("Add backup passkey");
  });

  it("offers recovery passkey setup only for passkey sessions", () => {
    const passkeyHtml = render("webauthn", successfulData());
    expect(passkeyHtml).toContain("Recovery passkey");
    expect(passkeyHtml).toContain("Add backup passkey");
    for (const label of ["Save", "Clear", "Add agent key", "Create API key"]) {
      expect(passkeyHtml).toContain(`>${label}<`);
    }
    expect(passkeyHtml).toContain("cannot trade");

    const rawHtml = render("raw_p256", successfulData());
    expect(rawHtml).not.toContain("Recovery passkey");
    expect(rawHtml).not.toContain("Add backup passkey");
  });

  it("explains that the connected key must be rotated from another session", () => {
    const html = render("webauthn", successfulData());
    expect(html).toContain(
      "disconnect and sign in with a different registered key first",
    );
    expect(html).toContain("current session key cannot revoke itself");
  });

  it("renders the prefixed current key as self and disables only its revoke button", () => {
    const sessionPublicKey = `02${"ab".repeat(32)}`;
    const html = render(
      "webauthn",
      successfulData([
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
      ]),
    );

    expect(html).toContain("this key");
    expect(html).toMatch(
      /<button[^>]*disabled=""[^>]*title="Reconnect with another registered key before revoking this one"[^>]*>/,
    );
    expect(html).toMatch(
      /<button[^>]*title="Revoke this key"[^>]*>Revoke<\/button>/,
    );
  });

  it("renders retryable private-read errors without empty data or mutation controls", async () => {
    const html = await renderWithPrivateReadErrors();

    expect(html).toContain("current profile could not be verified");
    expect(html).toContain("signing keys could not be verified");
    expect(html).toContain("read API keys could not be verified");
    expect(html.match(/>Retry<\/button>/g)).toHaveLength(3);
    expect(html).not.toContain("no signing keys");
    expect(html).not.toContain("no read API keys");
    expect(html).not.toMatch(
      />Save<|>Clear<|>Add agent key<|>Add backup passkey<|>Create API key<|>Revoke<\/button>/,
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
