import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearAllKeyHandles,
  getKeyHandle,
  setKeyHandle,
  useAccountStore,
} from "./store";

const mocks = vi.hoisted(() => {
  class MockSettingsActionError extends Error {
    constructor(
      message: string,
      public readonly status?: number,
    ) {
      super(message);
      this.name = "SettingsActionError";
    }
  }
  return {
    apiPost: vi.fn(),
    createApiKey: vi.fn(),
    createPasskeyForAccount: vi.fn(),
    exportPrivateJwk: vi.fn(),
    exportPublicKeyCompressedHex: vi.fn(),
    generateKeyPair: vi.fn(),
    importPrivateKey: vi.fn(),
    readStoredAccount: vi.fn(),
    readStoredAccountRevision: vi.fn(),
    registerPasskey: vi.fn(),
    revokeSigningKey: vi.fn(),
    SettingsActionError: MockSettingsActionError,
    writeStoredAccount: vi.fn(),
  };
});

vi.mock("@/lib/api/client", () => ({
  api: { GET: vi.fn(), POST: mocks.apiPost },
}));

vi.mock("@/lib/auth/p256", () => ({
  exportPrivateJwk: mocks.exportPrivateJwk,
  exportPublicKeyCompressedHex: mocks.exportPublicKeyCompressedHex,
  generateKeyPair: mocks.generateKeyPair,
  importPrivateKey: mocks.importPrivateKey,
}));

vi.mock("@/lib/auth/webauthn", () => ({
  createPasskeyForAccount: mocks.createPasskeyForAccount,
  discoverPasskeyAccount: vi.fn(),
  isWebAuthnAvailable: vi.fn(() => true),
}));

vi.mock("./settings", () => ({
  createApiKey: mocks.createApiKey,
  registerPasskey: mocks.registerPasskey,
  revokeSigningKey: mocks.revokeSigningKey,
  SettingsActionError: mocks.SettingsActionError,
}));

vi.mock("./storage", () => ({
  clearStoredAccount: vi.fn(),
  clearStoredReadApiKey: vi.fn(),
  readStoredAccount: mocks.readStoredAccount,
  readStoredAccountRevision: mocks.readStoredAccountRevision,
  writeStoredAccount: mocks.writeStoredAccount,
}));

import {
  AccountError,
  createDemoAccount,
  recoverCreatedAccount,
} from "./actions";

const privateKey = {} as CryptoKey;
const publicKey = {} as CryptoKey;
const bootstrapJwk = {
  kty: "EC",
  crv: "P-256",
  x: "bootstrap-x",
  y: "bootstrap-y",
  d: "bootstrap-d",
} satisfies JsonWebKey;
const passkey = {
  publicKeyHex: "03passkey",
  credentialIdB64url: "credential-73",
  attestationObjectB64url: "attestation",
  clientDataJSONB64url: "client-data",
};

let storedAccount: Record<string, unknown> | null;
let storageRevision: string;
let revisionCounter: number;

function persistAccount(account: Record<string, unknown>) {
  storedAccount = account;
  storageRevision = `revision-${++revisionCounter}`;
}

beforeEach(() => {
  vi.clearAllMocks();
  clearAllKeyHandles();
  storedAccount = null;
  storageRevision = "revision-0";
  revisionCounter = 0;
  useAccountStore.setState({
    session: null,
    connectModalOpen: true,
    connectModalRecovery: false,
  });
  mocks.generateKeyPair.mockResolvedValue({ privateKey, publicKey });
  mocks.exportPublicKeyCompressedHex.mockResolvedValue("02bootstrap");
  mocks.exportPrivateJwk.mockResolvedValue(bootstrapJwk);
  mocks.importPrivateKey.mockResolvedValue(privateKey);
  mocks.apiPost.mockResolvedValue({ data: { account_id: 73 } });
  mocks.createPasskeyForAccount.mockResolvedValue(passkey);
  mocks.registerPasskey.mockResolvedValue(undefined);
  mocks.revokeSigningKey.mockResolvedValue(undefined);
  mocks.writeStoredAccount.mockImplementation(persistAccount);
  mocks.readStoredAccount.mockImplementation(() => storedAccount);
  mocks.readStoredAccountRevision.mockImplementation(() => storageRevision);
});

function readKey(token: string) {
  return {
    id: 1,
    token,
    createdAtMs: 1,
    signerPublicKeyHex: "signer",
  };
}

describe("createDemoAccount recovery checkpoints", () => {
  it("uses the key-only public route and surfaces lifetime-cap exhaustion", async () => {
    mocks.apiPost.mockResolvedValue({
      error: { code: "PUBLIC_ACCOUNT_CAPACITY_EXHAUSTED" },
      response: { status: 409 },
    });

    await expect(createDemoAccount("local_key")).rejects.toMatchObject({
      kind: "capacity_exhausted",
      message: expect.stringContaining("allocated"),
    } satisfies Partial<AccountError>);
    expect(mocks.apiPost).toHaveBeenCalledWith("/v1/onboarding/accounts", {
      body: {
        initial_key: {
          public_key_hex: "02bootstrap",
          auth_scheme: "raw_p256",
        },
      },
    });
    expect(mocks.writeStoredAccount).not.toHaveBeenCalled();
  });

  it("does not create an account when browser storage cannot be read", async () => {
    mocks.readStoredAccountRevision.mockImplementationOnce(() => {
      throw new DOMException("storage blocked", "SecurityError");
    });

    await expect(
      createDemoAccount("local_key"),
    ).rejects.toMatchObject({
      kind: "unknown",
      message: expect.stringContaining("Browser storage is unavailable"),
    } satisfies Partial<AccountError>);
    expect(mocks.apiPost).not.toHaveBeenCalled();
    expect(mocks.writeStoredAccount).not.toHaveBeenCalled();
  });

  it("requires explicit recovery when storage changes during account creation", async () => {
    let finishCreate!: (value: unknown) => void;
    mocks.apiPost.mockReturnValue(
      new Promise((resolve) => {
        finishCreate = resolve;
      }),
    );

    const creation = createDemoAccount("local_key");
    await vi.waitFor(() => expect(mocks.apiPost).toHaveBeenCalledOnce());
    storageRevision = "external-revision";
    storedAccount = {
      accountId: 91,
      publicKeyHex: "03other",
      authScheme: "raw_p256",
      jwk: { kty: "EC", d: "other" },
    };
    finishCreate({ data: { account_id: 73 } });

    await expect(creation).rejects.toMatchObject({
      kind: "account_created_recovery",
      createdAccountId: 73,
      createdPublicKeyHex: "02bootstrap",
    } satisfies Partial<AccountError>);
    expect(mocks.writeStoredAccount).not.toHaveBeenCalled();
    expect(storedAccount.accountId).toBe(91);

    mocks.createApiKey.mockResolvedValue(readKey("sybk_recovered"));
    await recoverCreatedAccount(73, "02bootstrap", "raw_p256");
    expect(storedAccount.accountId).toBe(73);
    expect(storedAccount.publicKeyHex).toBe("02bootstrap");
    expect(mocks.apiPost).toHaveBeenCalledOnce();
  });

  it("keeps a raw checkpoint when the passkey ceremony is cancelled", async () => {
    mocks.createPasskeyForAccount.mockRejectedValue(
      new Error("Passkey creation was cancelled"),
    );

    await expect(
      createDemoAccount("passkey"),
    ).rejects.toMatchObject({
      kind: "account_created_recovery",
      message: expect.stringContaining("Account #73 was created and saved"),
    } satisfies Partial<AccountError>);

    expect(mocks.writeStoredAccount).toHaveBeenNthCalledWith(1, {
      accountId: 73,
      publicKeyHex: "02bootstrap",
      authScheme: "raw_p256",
      jwk: bootstrapJwk,
    });
    expect(storedAccount).toEqual({
      accountId: 73,
      publicKeyHex: "02bootstrap",
      authScheme: "raw_p256",
      jwk: bootstrapJwk,
    });
    expect(useAccountStore.getState().session).toBeNull();
    expect(mocks.createApiKey).not.toHaveBeenCalled();
    expect(mocks.apiPost).toHaveBeenCalledOnce();
  });

  it("keeps the no-token raw checkpoint when initial session minting fails", async () => {
    mocks.createApiKey.mockRejectedValue(new Error("network unavailable"));

    await expect(
      createDemoAccount("local_key"),
    ).rejects.toMatchObject({
      kind: "account_created_recovery",
    } satisfies Partial<AccountError>);

    expect(storedAccount).toEqual({
      accountId: 73,
      publicKeyHex: "02bootstrap",
      authScheme: "raw_p256",
      jwk: bootstrapJwk,
    });
    expect(useAccountStore.getState().session).toBeNull();
    expect(getKeyHandle(73)).toBe(privateKey);
  });

  it("retries an in-memory checkpoint without creating another account", async () => {
    mocks.writeStoredAccount.mockImplementationOnce(() => {
      throw new Error("localStorage blocked");
    });

    await expect(
      createDemoAccount("local_key"),
    ).rejects.toMatchObject({
      kind: "account_created_recovery",
      createdAccountId: 73,
      createdPublicKeyHex: "02bootstrap",
      createdAuthScheme: "raw_p256",
    } satisfies Partial<AccountError>);
    expect(mocks.createApiKey).not.toHaveBeenCalled();

    mocks.writeStoredAccount.mockImplementation(persistAccount);
    mocks.createApiKey.mockResolvedValue(readKey("sybk_recovered"));
    await recoverCreatedAccount(73, "02bootstrap", "raw_p256");

    expect(mocks.apiPost).toHaveBeenCalledOnce();
    expect(storedAccount).toEqual({
      accountId: 73,
      publicKeyHex: "02bootstrap",
      authScheme: "raw_p256",
      jwk: bootstrapJwk,
      readApiKey: "sybk_recovered",
    });
    expect(useAccountStore.getState().session?.accountId).toBe(73);
    expect(useAccountStore.getState().connectModalOpen).toBe(false);
  });

  it("repairs a parseable partial first checkpoint with the pending bootstrap", async () => {
    mocks.writeStoredAccount.mockImplementationOnce(() => {
      storedAccount = {
        accountId: 73,
        publicKeyHex: "03old",
        authScheme: "raw_p256",
        jwk: { kty: "EC", d: "old" },
      };
      throw new Error("quota failed mid-write");
    });

    await expect(
      createDemoAccount("local_key"),
    ).rejects.toMatchObject({
      kind: "account_created_recovery",
      createdAccountId: 73,
      createdPublicKeyHex: "02bootstrap",
    } satisfies Partial<AccountError>);

    mocks.writeStoredAccount.mockImplementation(persistAccount);
    mocks.createApiKey.mockResolvedValue(readKey("sybk_repaired"));
    await recoverCreatedAccount(73, "02bootstrap", "raw_p256");

    expect(storedAccount).toEqual({
      accountId: 73,
      publicKeyHex: "02bootstrap",
      authScheme: "raw_p256",
      jwk: bootstrapJwk,
      readApiKey: "sybk_repaired",
    });
    expect(mocks.apiPost).toHaveBeenCalledOnce();
  });

  it("keeps the raw checkpoint when passkey registration fails", async () => {
    mocks.registerPasskey.mockRejectedValue(new Error("registration rejected"));

    await expect(
      createDemoAccount("passkey"),
    ).rejects.toMatchObject({
      kind: "account_created_recovery",
    } satisfies Partial<AccountError>);

    expect(storedAccount).toEqual({
      accountId: 73,
      publicKeyHex: "02bootstrap",
      authScheme: "raw_p256",
      jwk: bootstrapJwk,
    });
    expect(mocks.revokeSigningKey).not.toHaveBeenCalled();
    expect(mocks.createApiKey).not.toHaveBeenCalled();
    expect(useAccountStore.getState().session).toBeNull();
  });

  it("checkpoints the registered passkey before minting its read session", async () => {
    mocks.createApiKey.mockRejectedValueOnce(
      new Error("Passkey signing was cancelled"),
    );

    await expect(
      createDemoAccount("passkey"),
    ).rejects.toMatchObject({
      kind: "account_created_recovery",
    } satisfies Partial<AccountError>);

    expect(storedAccount).toEqual({
      accountId: 73,
      publicKeyHex: "03passkey",
      authScheme: "webauthn",
      credentialIdB64url: "credential-73",
    });
    expect(mocks.writeStoredAccount.mock.invocationCallOrder[1]).toBeLessThan(
      mocks.createApiKey.mock.invocationCallOrder[0]!,
    );
    expect(mocks.revokeSigningKey).not.toHaveBeenCalled();
    expect(getKeyHandle(73)).toBeNull();
    expect(useAccountStore.getState().session).toBeNull();
  });

  it("does not overwrite a newer same-id identity after a passkey prompt", async () => {
    let finishPasskey!: (value: typeof passkey) => void;
    mocks.createPasskeyForAccount.mockReturnValue(
      new Promise((resolve) => {
        finishPasskey = resolve;
      }),
    );

    const creation = createDemoAccount("passkey");
    await vi.waitFor(() =>
      expect(mocks.createPasskeyForAccount).toHaveBeenCalledOnce(),
    );
    const replacementKey = {} as CryptoKey;
    storageRevision = "external-revision";
    storedAccount = {
      accountId: 73,
      publicKeyHex: "03replacement",
      authScheme: "raw_p256",
      jwk: { kty: "EC", d: "replacement" },
      readApiKey: "sybk_replacement",
    };
    setKeyHandle(73, replacementKey);
    finishPasskey(passkey);

    await expect(creation).rejects.toMatchObject({
      kind: "session_changed",
    } satisfies Partial<AccountError>);
    expect(mocks.registerPasskey).not.toHaveBeenCalled();
    expect(mocks.writeStoredAccount).toHaveBeenCalledOnce();
    expect(getKeyHandle(73)).toBe(replacementKey);
    expect(storedAccount.publicKeyHex).toBe("03replacement");
  });

  it("does not misclassify a newer same-id identity after registration rejects", async () => {
    let rejectRegistration!: (error: Error) => void;
    mocks.registerPasskey.mockReturnValue(
      new Promise((_, reject) => {
        rejectRegistration = reject;
      }),
    );

    const creation = createDemoAccount("passkey");
    await vi.waitFor(() =>
      expect(mocks.registerPasskey).toHaveBeenCalledOnce(),
    );
    const replacementKey = {} as CryptoKey;
    storageRevision = "external-revision";
    storedAccount = {
      accountId: 73,
      publicKeyHex: "03replacement",
      authScheme: "raw_p256",
      jwk: { kty: "EC", d: "replacement" },
      readApiKey: "sybk_replacement",
    };
    setKeyHandle(73, replacementKey);
    rejectRegistration(new Error("registration response failed"));

    await expect(creation).rejects.toMatchObject({
      kind: "session_changed",
      createdAccountId: undefined,
    } satisfies Partial<AccountError>);
    expect(mocks.writeStoredAccount).toHaveBeenCalledOnce();
    expect(getKeyHandle(73)).toBe(replacementKey);
    expect(storedAccount.publicKeyHex).toBe("03replacement");
  });

  it("does not leave its passkey session active after a disconnect during cleanup", async () => {
    let finishRevoke!: () => void;
    mocks.createApiKey.mockResolvedValueOnce(readKey("sybk_passkey"));
    mocks.revokeSigningKey.mockReturnValue(
      new Promise<void>((resolve) => {
        finishRevoke = resolve;
      }),
    );

    const creation = createDemoAccount("passkey");
    await vi.waitFor(() =>
      expect(mocks.revokeSigningKey).toHaveBeenCalledOnce(),
    );
    storageRevision = "disconnect-revision";
    storedAccount = null;
    finishRevoke();

    await expect(creation).rejects.toMatchObject({
      kind: "session_changed",
    } satisfies Partial<AccountError>);
    expect(useAccountStore.getState().session).toBeNull();
    expect(storedAccount).toBeNull();
    expect(getKeyHandle(73)).toBeNull();
  });

  it("never reconnects a different account through a stale recovery action", async () => {
    mocks.createPasskeyForAccount.mockRejectedValue(
      new Error("Passkey creation was cancelled"),
    );
    await expect(
      createDemoAccount("passkey"),
    ).rejects.toMatchObject({
      kind: "account_created_recovery",
    } satisfies Partial<AccountError>);
    storedAccount = {
      accountId: 73,
      publicKeyHex: "03other",
      authScheme: "raw_p256",
      jwk: { kty: "EC", d: "other" },
    };

    await expect(
      recoverCreatedAccount(73, "02bootstrap", "raw_p256"),
    ).rejects.toMatchObject({
      kind: "session_changed",
      message: expect.stringContaining(
        "saved identity no longer matches newly-created account #73",
      ),
    } satisfies Partial<AccountError>);
    expect(mocks.createApiKey).not.toHaveBeenCalled();
    expect(storedAccount.accountId).toBe(73);
    expect(storedAccount.publicKeyHex).toBe("03other");
  });

  it("keeps the usable passkey session when bootstrap cleanup fails", async () => {
    mocks.createApiKey.mockResolvedValueOnce(readKey("sybk_passkey"));
    mocks.revokeSigningKey.mockRejectedValue(
      new mocks.SettingsActionError(
        "revoke_key failed (HTTP 409): cannot revoke the last signing key",
        409,
      ),
    );

    await expect(
      createDemoAccount("passkey"),
    ).rejects.toMatchObject({
      kind: "account_created_recovery",
    } satisfies Partial<AccountError>);

    expect(storedAccount).toEqual({
      accountId: 73,
      publicKeyHex: "03passkey",
      authScheme: "webauthn",
      credentialIdB64url: "credential-73",
      readApiKey: "sybk_passkey",
    });
    expect(useAccountStore.getState().session).toEqual(storedAccount);
    expect(getKeyHandle(73)).toBeNull();
    expect(useAccountStore.getState().connectModalOpen).toBe(true);
    expect(mocks.revokeSigningKey).toHaveBeenCalledOnce();
  });

  it("migrates a successful account only after both credentials are durable", async () => {
    mocks.createApiKey.mockResolvedValueOnce(readKey("sybk_passkey"));

    await createDemoAccount("passkey");

    expect(mocks.writeStoredAccount).toHaveBeenCalledTimes(3);
    expect(mocks.writeStoredAccount).toHaveBeenNthCalledWith(2, {
      accountId: 73,
      publicKeyHex: "03passkey",
      authScheme: "webauthn",
      credentialIdB64url: "credential-73",
    });
    expect(mocks.writeStoredAccount.mock.invocationCallOrder[1]).toBeLessThan(
      mocks.revokeSigningKey.mock.invocationCallOrder[0]!,
    );
    expect(useAccountStore.getState().session).toEqual({
      accountId: 73,
      publicKeyHex: "03passkey",
      authScheme: "webauthn",
      credentialIdB64url: "credential-73",
      readApiKey: "sybk_passkey",
    });
    expect(getKeyHandle(73)).toBeNull();
    expect(useAccountStore.getState().connectModalOpen).toBe(false);
  });

  it("refreshes bootstrap cleanup when the first revoke binding crosses a block", async () => {
    mocks.createApiKey.mockResolvedValueOnce(readKey("sybk_passkey"));
    mocks.revokeSigningKey
      .mockRejectedValueOnce(
        new mocks.SettingsActionError(
          "revoke_key failed (HTTP 409): stale key-operation state binding for account 73",
          409,
        ),
      )
      .mockResolvedValueOnce(undefined);

    await createDemoAccount("passkey");

    expect(mocks.revokeSigningKey).toHaveBeenCalledTimes(2);
    expect(mocks.revokeSigningKey).toHaveBeenNthCalledWith(1, {
      accountId: 73,
      publicKeyHex: "02bootstrap",
      authScheme: "raw_p256",
      targetPubkeyHex: "02bootstrap",
      targetAuthScheme: "raw_p256",
    });
    expect(mocks.revokeSigningKey).toHaveBeenNthCalledWith(2, {
      accountId: 73,
      publicKeyHex: "02bootstrap",
      authScheme: "raw_p256",
      targetPubkeyHex: "02bootstrap",
      targetAuthScheme: "raw_p256",
    });
    expect(useAccountStore.getState().session).toEqual({
      accountId: 73,
      publicKeyHex: "03passkey",
      authScheme: "webauthn",
      credentialIdB64url: "credential-73",
      readApiKey: "sybk_passkey",
    });
    expect(getKeyHandle(73)).toBeNull();
    expect(useAccountStore.getState().connectModalOpen).toBe(false);
  });

  it("activates a successful local account from the same raw checkpoint", async () => {
    mocks.createApiKey.mockResolvedValue(readKey("sybk_local"));

    await createDemoAccount("local_key");

    expect(storedAccount).toEqual({
      accountId: 73,
      publicKeyHex: "02bootstrap",
      authScheme: "raw_p256",
      jwk: bootstrapJwk,
      readApiKey: "sybk_local",
    });
    expect(useAccountStore.getState().session).toEqual({
      accountId: 73,
      publicKeyHex: "02bootstrap",
      authScheme: "raw_p256",
      readApiKey: "sybk_local",
    });
    expect(getKeyHandle(73)).toBe(privateKey);
    expect(mocks.createPasskeyForAccount).not.toHaveBeenCalled();
    expect(useAccountStore.getState().connectModalOpen).toBe(false);
  });
});
