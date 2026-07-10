import { afterEach, describe, expect, it, vi } from "vitest";
import { useAccountStore } from "./store";

const mocks = vi.hoisted(() => ({
  apiGet: vi.fn(),
  discoverPasskeyAccount: vi.fn(),
  writeStoredAccount: vi.fn(),
}));

vi.mock("@/lib/api/client", () => ({
  api: { GET: mocks.apiGet, POST: vi.fn() },
}));

vi.mock("@/lib/auth/webauthn", () => ({
  createPasskeyForAccount: vi.fn(),
  discoverPasskeyAccount: mocks.discoverPasskeyAccount,
  isWebAuthnAvailable: vi.fn(() => true),
  verifyStoredPasskey: vi.fn(),
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
    mocks.apiGet.mockResolvedValue({
      data: [
        { auth_scheme: "raw_p256", public_key_hex: "02raw" },
        { auth_scheme: "webauthn", public_key_hex: "03passkey" },
      ],
    });

    await signInWithDiscoverablePasskey();

    const expected = {
      accountId: 109,
      publicKeyHex: "03passkey",
      authScheme: "webauthn",
      credentialIdB64url: "credential-109",
    };
    expect(mocks.apiGet).toHaveBeenCalledWith("/v1/accounts/{id}/keys", {
      params: { path: { id: 109 } },
    });
    expect(mocks.writeStoredAccount).toHaveBeenCalledWith(expected);
    expect(useAccountStore.getState().session).toEqual(expected);
    expect(useAccountStore.getState().connectModalOpen).toBe(false);
  });

  it("surfaces a friendly error when the account has no passkey", async () => {
    mocks.discoverPasskeyAccount.mockResolvedValue({
      accountId: 109,
      credentialIdB64url: "credential-109",
    });
    mocks.apiGet.mockResolvedValue({ data: [] });

    await expect(signInWithDiscoverablePasskey()).rejects.toMatchObject({
      message: "No passkey is registered for account #109",
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
    mocks.apiGet.mockResolvedValue({
      error: { message: "not found" },
      response: { status: 404 },
    });

    await expect(signInWithDiscoverablePasskey()).rejects.toMatchObject({
      message: "Account #109 was not found",
      kind: "account_not_found",
    } satisfies Partial<AccountError>);
  });
});
