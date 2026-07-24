import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  createPasskeyForAccount: vi.fn(),
  registerPasskey: vi.fn(),
}));

vi.mock("@/lib/auth/webauthn", () => ({
  createPasskeyForAccount: mocks.createPasskeyForAccount,
}));

vi.mock("./settings", () => ({
  registerPasskey: mocks.registerPasskey,
  SettingsActionError: class SettingsActionError extends Error {
    constructor(
      message: string,
      public readonly status?: number,
    ) {
      super(message);
    }
  },
}));

import { addBackupPasskey } from "./backup-passkey";

describe("addBackupPasskey", () => {
  beforeEach(() => vi.clearAllMocks());

  it("creates a credential and registers it with the active passkey", async () => {
    const passkey = {
      publicKeyHex: "03backup",
      credentialIdB64url: "backup-credential",
      attestationObjectB64url: "attestation",
      clientDataJSONB64url: "client-data",
    };
    mocks.createPasskeyForAccount.mockResolvedValue(passkey);
    mocks.registerPasskey.mockResolvedValue(undefined);

    await expect(
      addBackupPasskey({
        accountId: 17,
        publicKeyHex: "02primary",
        credentialIdB64url: "primary-credential",
      }),
    ).resolves.toEqual({ publicKeyHex: "03backup" });

    expect(mocks.createPasskeyForAccount).toHaveBeenCalledWith(17, {
      authenticatorAttachment: "any",
    });
    expect(mocks.registerPasskey).toHaveBeenCalledWith({
      accountId: 17,
      publicKeyHex: "02primary",
      authScheme: "webauthn",
      credentialIdB64url: "primary-credential",
      passkey,
      label: "backup passkey",
    });
  });

  it("does not attempt registration when credential creation fails", async () => {
    mocks.createPasskeyForAccount.mockRejectedValue(
      new DOMException("cancelled", "NotAllowedError"),
    );

    await expect(
      addBackupPasskey({
        accountId: 17,
        publicKeyHex: "02primary",
        credentialIdB64url: "primary-credential",
      }),
    ).rejects.toMatchObject({ name: "NotAllowedError" });
    expect(mocks.registerPasskey).not.toHaveBeenCalled();
  });

  it("refreshes and re-signs an exact stale key-state conflict", async () => {
    const passkey = {
      publicKeyHex: "03backup",
      credentialIdB64url: "backup-credential",
      attestationObjectB64url: "attestation",
      clientDataJSONB64url: "client-data",
    };
    mocks.createPasskeyForAccount.mockResolvedValue(passkey);
    const { SettingsActionError } = await import("./settings");
    mocks.registerPasskey
      .mockRejectedValueOnce(
        new SettingsActionError(
          "register_passkey failed (HTTP 409): stale key-operation state binding for account 17",
          409,
        ),
      )
      .mockResolvedValueOnce(undefined);

    await expect(
      addBackupPasskey({
        accountId: 17,
        publicKeyHex: "02primary",
        credentialIdB64url: "primary-credential",
      }),
    ).resolves.toEqual({ publicKeyHex: "03backup" });

    expect(mocks.createPasskeyForAccount).toHaveBeenCalledOnce();
    expect(mocks.registerPasskey).toHaveBeenCalledTimes(2);
  });

  it("does not retry unrelated registration conflicts", async () => {
    mocks.createPasskeyForAccount.mockResolvedValue({
      publicKeyHex: "03backup",
      credentialIdB64url: "backup-credential",
      attestationObjectB64url: "attestation",
      clientDataJSONB64url: "client-data",
    });
    const { SettingsActionError } = await import("./settings");
    mocks.registerPasskey.mockRejectedValue(
      new SettingsActionError(
        "register_passkey failed (HTTP 409): key already exists",
        409,
      ),
    );

    await expect(
      addBackupPasskey({
        accountId: 17,
        publicKeyHex: "02primary",
        credentialIdB64url: "primary-credential",
      }),
    ).rejects.toThrow("key already exists");
    expect(mocks.registerPasskey).toHaveBeenCalledOnce();
  });
});
