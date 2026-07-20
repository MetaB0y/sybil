import { afterEach, describe, expect, it, vi } from "vitest";
import {
  clearAllKeyHandles,
  getKeyHandle,
  setKeyHandle,
  useAccountStore,
} from "./store";

const mocks = vi.hoisted(() => ({
  apiGet: vi.fn(async () => ({
    data: { genesis_hash: "ab".repeat(32) },
  })),
  apiPost: vi.fn(),
  clearStoredAccount: vi.fn(),
  clearStoredReadApiKey: vi.fn(),
  discoverPasskeyAccount: vi.fn(),
  importPrivateKey: vi.fn(),
  readStoredAccount: vi.fn(),
  readStoredAccountRevision: vi.fn(),
  signBytes: vi.fn(),
  signWebAuthnBytes: vi.fn(),
  writeStoredAccount: vi.fn(),
}));

vi.mock("@/lib/api/client", () => ({
  api: { GET: mocks.apiGet, POST: mocks.apiPost },
}));

vi.mock("@/lib/auth/webauthn", () => ({
  createPasskeyForAccount: vi.fn(),
  discoverPasskeyAccount: mocks.discoverPasskeyAccount,
  isWebAuthnAvailable: vi.fn(() => true),
  verifyStoredPasskey: vi.fn(),
  signWebAuthnBytes: mocks.signWebAuthnBytes,
}));

vi.mock("@/lib/auth/p256", () => ({
  exportPrivateJwk: vi.fn(),
  exportPublicKeyCompressedHex: vi.fn(),
  generateKeyPair: vi.fn(),
  importPrivateKey: mocks.importPrivateKey,
  signBytes: mocks.signBytes,
}));

vi.mock("./storage", () => ({
  clearStoredAccount: mocks.clearStoredAccount,
  clearStoredReadApiKey: mocks.clearStoredReadApiKey,
  readStoredAccount: mocks.readStoredAccount,
  readStoredAccountRevision: mocks.readStoredAccountRevision,
  writeStoredAccount: mocks.writeStoredAccount,
}));

import {
  AccountError,
  invalidateReadSession,
  rehydrateFromStorage,
  signInWithDiscoverablePasskey,
  signInWithStoredAccount,
} from "./actions";

afterEach(() => {
  vi.clearAllMocks();
  clearAllKeyHandles();
  useAccountStore.setState({
    session: null,
    connectModalOpen: true,
    connectModalRecovery: false,
  });
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

describe("read-session recovery", () => {
  it("invalidates only the read token and opens reconnect", () => {
    const privateKey = {} as CryptoKey;
    setKeyHandle(12, privateKey);
    useAccountStore.setState({
      session: {
        accountId: 12,
        publicKeyHex: "02local",
        authScheme: "raw_p256",
        readApiKey: "sybk_stale",
      },
      connectModalOpen: false,
      connectModalRecovery: false,
    });

    invalidateReadSession();

    expect(mocks.clearStoredReadApiKey).toHaveBeenCalledOnce();
    expect(mocks.clearStoredAccount).not.toHaveBeenCalled();
    expect(getKeyHandle(12)).toBeNull();
    expect(useAccountStore.getState().session).toBeNull();
    expect(useAccountStore.getState().connectModalOpen).toBe(true);
    expect(useAccountStore.getState().connectModalRecovery).toBe(true);
  });

  it("mints a replacement read token with a saved local signing key", async () => {
    const jwk = {
      kty: "EC",
      crv: "P-256",
      x: "saved-x",
      y: "saved-y",
      d: "saved-d",
    } satisfies JsonWebKey;
    const privateKey = {} as CryptoKey;
    mocks.readStoredAccount.mockReturnValue({
      accountId: 12,
      publicKeyHex: "02local",
      authScheme: "raw_p256",
      jwk,
    });
    mocks.importPrivateKey.mockResolvedValue(privateKey);
    mocks.signBytes.mockResolvedValue("signed");
    mocks.apiPost.mockResolvedValue({
      data: {
        id: 9,
        token: "sybk_fresh",
        created_at_ms: 2,
        signer_pubkey_hex: "02local",
      },
    });

    await signInWithStoredAccount();

    expect(mocks.writeStoredAccount).toHaveBeenCalledWith({
      accountId: 12,
      publicKeyHex: "02local",
      authScheme: "raw_p256",
      jwk,
      readApiKey: "sybk_fresh",
    });
    expect(useAccountStore.getState().session).toEqual({
      accountId: 12,
      publicKeyHex: "02local",
      authScheme: "raw_p256",
      readApiKey: "sybk_fresh",
    });
    expect(useAccountStore.getState().connectModalOpen).toBe(false);
  });

  it("keeps a saved passkey when the account belongs to an older genesis", async () => {
    mocks.readStoredAccount.mockReturnValue({
      accountId: 44,
      publicKeyHex: "03passkey",
      authScheme: "webauthn",
      credentialIdB64url: "credential-44",
    });
    mocks.signWebAuthnBytes.mockResolvedValue({
      credential_id_b64url: "credential-44",
    });
    mocks.apiPost.mockResolvedValue({
      error: { message: "account not found" },
      response: { status: 404 },
    });

    await expect(signInWithStoredAccount()).rejects.toMatchObject({
      message:
        "Saved account #44 is not registered on this devnet. Its passkey was kept.",
      kind: "account_not_found",
    } satisfies Partial<AccountError>);
    expect(mocks.clearStoredAccount).not.toHaveBeenCalled();
    expect(mocks.writeStoredAccount).not.toHaveBeenCalled();
  });

  it("does not resurrect a local session after storage changes during reconnect", async () => {
    let revision = "revision-1";
    let stored: ReturnType<typeof mocks.readStoredAccount> = {
      accountId: 12,
      publicKeyHex: "02local",
      authScheme: "raw_p256",
      jwk: { kty: "EC", d: "saved" },
    };
    let finishRequest!: (value: unknown) => void;
    mocks.readStoredAccount.mockImplementation(() => stored);
    mocks.readStoredAccountRevision.mockImplementation(() => revision);
    const olderKey = {} as CryptoKey;
    const newerKey = {} as CryptoKey;
    mocks.importPrivateKey.mockResolvedValue(olderKey);
    mocks.signBytes.mockResolvedValue("signed");
    mocks.apiPost.mockReturnValue(
      new Promise((resolve) => {
        finishRequest = resolve;
      }),
    );

    const reconnect = signInWithStoredAccount();
    await vi.waitFor(() => expect(mocks.apiPost).toHaveBeenCalledOnce());
    revision = "revision-2";
    stored = {
      accountId: 12,
      publicKeyHex: "03replacement",
      authScheme: "raw_p256",
      jwk: { kty: "EC", d: "replacement" },
      readApiKey: "sybk_replacement",
    };
    setKeyHandle(12, newerKey);
    finishRequest({
      data: {
        id: 9,
        token: "sybk_late",
        created_at_ms: 2,
        signer_pubkey_hex: "02local",
      },
    });

    await expect(reconnect).rejects.toMatchObject({ kind: "session_changed" });
    expect(mocks.writeStoredAccount).not.toHaveBeenCalled();
    expect(useAccountStore.getState().session).toBeNull();
    expect(getKeyHandle(12)).toBe(newerKey);
  });

  it("does not resurrect a passkey session after storage changes during its prompt", async () => {
    let revision = "revision-1";
    let stored: ReturnType<typeof mocks.readStoredAccount> = {
      accountId: 44,
      publicKeyHex: "03passkey",
      authScheme: "webauthn",
      credentialIdB64url: "credential-44",
    };
    let finishPrompt!: (value: unknown) => void;
    mocks.readStoredAccount.mockImplementation(() => stored);
    mocks.readStoredAccountRevision.mockImplementation(() => revision);
    mocks.signWebAuthnBytes.mockReturnValue(
      new Promise((resolve) => {
        finishPrompt = resolve;
      }),
    );
    mocks.apiPost.mockResolvedValue({
      data: {
        id: 9,
        token: "sybk_late",
        created_at_ms: 2,
        signer_pubkey_hex: "03passkey",
      },
    });

    const reconnect = signInWithStoredAccount();
    await vi.waitFor(() =>
      expect(mocks.signWebAuthnBytes).toHaveBeenCalledOnce(),
    );
    revision = "revision-2";
    stored = null;
    finishPrompt({ credential_id_b64url: "credential-44" });

    await expect(reconnect).rejects.toMatchObject({ kind: "session_changed" });
    expect(mocks.writeStoredAccount).not.toHaveBeenCalled();
    expect(useAccountStore.getState().session).toBeNull();
  });

  it("ignores a stale hydration and closes recovery after a current one", async () => {
    let revision = "revision-1";
    let stored: ReturnType<typeof mocks.readStoredAccount> = {
      accountId: 12,
      publicKeyHex: "02local",
      authScheme: "raw_p256",
      jwk: { kty: "EC", d: "saved" },
      readApiKey: "sybk_fresh",
    };
    let finishImport!: (value: CryptoKey) => void;
    mocks.readStoredAccount.mockImplementation(() => stored);
    mocks.readStoredAccountRevision.mockImplementation(() => revision);
    mocks.importPrivateKey.mockReturnValue(
      new Promise((resolve) => {
        finishImport = resolve;
      }),
    );
    useAccountStore.setState({
      session: null,
      connectModalOpen: true,
      connectModalRecovery: true,
    });

    const staleHydration = rehydrateFromStorage();
    revision = "revision-2";
    stored = null;
    finishImport({} as CryptoKey);
    await staleHydration;
    expect(useAccountStore.getState().session).toBeNull();
    expect(useAccountStore.getState().connectModalOpen).toBe(true);

    stored = {
      accountId: 12,
      publicKeyHex: "02local",
      authScheme: "raw_p256",
      jwk: { kty: "EC", d: "saved" },
      readApiKey: "sybk_current",
    };
    mocks.importPrivateKey.mockResolvedValue({} as CryptoKey);
    await rehydrateFromStorage();

    expect(useAccountStore.getState().session?.readApiKey).toBe("sybk_current");
    expect(useAccountStore.getState().connectModalOpen).toBe(false);
  });

  it("closes only a forced recovery modal after a cross-tab full disconnect", async () => {
    mocks.readStoredAccount.mockReturnValue(null);
    mocks.readStoredAccountRevision.mockReturnValue("revision-2");
    useAccountStore.setState({
      session: null,
      connectModalOpen: true,
      connectModalRecovery: true,
    });

    await rehydrateFromStorage();

    expect(useAccountStore.getState().connectModalOpen).toBe(false);
    expect(useAccountStore.getState().connectModalRecovery).toBe(false);

    useAccountStore.setState({
      session: null,
      connectModalOpen: true,
      connectModalRecovery: false,
    });
    await rehydrateFromStorage();
    expect(useAccountStore.getState().connectModalOpen).toBe(true);
  });
});
