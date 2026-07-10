import { afterEach, describe, expect, it, vi } from "vitest";
import { useAccountStore } from "./store";

const mocks = vi.hoisted(() => ({
  apiPost: vi.fn(),
  discoverPasskeyAccount: vi.fn(),
  signWebAuthnBytes: vi.fn(),
  writeStoredAccount: vi.fn(),
}));

vi.mock("@/lib/api/client", () => ({
  api: { GET: vi.fn(), POST: mocks.apiPost },
}));

vi.mock("@/lib/auth/webauthn", () => ({
  createPasskeyForAccount: vi.fn(),
  discoverPasskeyAccount: mocks.discoverPasskeyAccount,
  isWebAuthnAvailable: vi.fn(() => true),
  verifyStoredPasskey: vi.fn(),
  signWebAuthnBytes: mocks.signWebAuthnBytes,
}));

vi.mock("./storage", () => ({
  clearStoredAccount: vi.fn(),
  readStoredAccount: vi.fn(),
  writeStoredAccount: mocks.writeStoredAccount,
}));

import { AccountError, signInWithDiscoverablePasskey } from "./actions";

afterEach(() => {
  vi.clearAllMocks();
  useAccountStore.setState({ session: null, connectModalOpen: true });
});

describe("signInWithDiscoverablePasskey", () => {
  it("restores storage and the session from the user handle and server key", async () => {
    mocks.discoverPasskeyAccount.mockResolvedValue({
      accountId: 109,
      credentialIdB64url: "credential-109",
    });
    mocks.signWebAuthnBytes.mockResolvedValue({
      credential_id_b64url: "credential-109",
    });
    mocks.apiPost.mockResolvedValue({
      data: {
        id: 7,
        token: "sybk_read",
        created_at_ms: 1,
        signer_pubkey_hex: "03passkey",
      },
    });

    await signInWithDiscoverablePasskey();

    const expected = {
      accountId: 109,
      publicKeyHex: "03passkey",
      authScheme: "webauthn",
      credentialIdB64url: "credential-109",
      readApiKey: "sybk_read",
    };
    expect(mocks.apiPost).toHaveBeenCalledWith(
      "/v1/accounts/{id}/api-keys",
      expect.objectContaining({ params: { path: { id: 109 } } }),
    );
    expect(mocks.writeStoredAccount).toHaveBeenCalledWith(expected);
    expect(useAccountStore.getState().session).toEqual(expected);
    expect(useAccountStore.getState().connectModalOpen).toBe(false);
  });

  it("surfaces a friendly error when the passkey is not registered", async () => {
    mocks.discoverPasskeyAccount.mockResolvedValue({
      accountId: 109,
      credentialIdB64url: "credential-109",
    });
    mocks.signWebAuthnBytes.mockResolvedValue({});
    mocks.apiPost.mockResolvedValue({
      error: { message: "invalid passkey" },
      response: { status: 401 },
    });

    await expect(signInWithDiscoverablePasskey()).rejects.toMatchObject({
      message: "This passkey is not registered for account #109",
      kind: "account_not_found",
    } satisfies Partial<AccountError>);
    expect(mocks.writeStoredAccount).not.toHaveBeenCalled();
    expect(useAccountStore.getState().session).toBeNull();
  });

  it("surfaces a friendly error when the account no longer exists", async () => {
    mocks.discoverPasskeyAccount.mockResolvedValue({
      accountId: 109,
      credentialIdB64url: "credential-109",
    });
    mocks.signWebAuthnBytes.mockResolvedValue({});
    mocks.apiPost.mockResolvedValue({
      error: { message: "not found" },
      response: { status: 404 },
    });

    await expect(signInWithDiscoverablePasskey()).rejects.toMatchObject({
      message: "This passkey is not registered for account #109",
      kind: "account_not_found",
    } satisfies Partial<AccountError>);
  });
});
