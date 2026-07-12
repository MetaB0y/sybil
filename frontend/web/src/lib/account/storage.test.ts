import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearStoredReadApiKey,
  readStoredAccount,
  STORAGE_KEYS,
  writeStoredAccount,
} from "./storage";

describe("stored account identity", () => {
  let values: Map<string, string>;
  let sessionValues: Map<string, string>;

  beforeEach(() => {
    values = new Map();
    sessionValues = new Map();
    vi.stubGlobal("window", {
      localStorage: {
        getItem: (key: string) => values.get(key) ?? null,
        setItem: (key: string, value: string) => values.set(key, value),
        removeItem: (key: string) => values.delete(key),
      },
      sessionStorage: {
        getItem: (key: string) => sessionValues.get(key) ?? null,
        setItem: (key: string, value: string) =>
          sessionValues.set(key, value),
        removeItem: (key: string) => sessionValues.delete(key),
      },
    });
  });

  afterEach(() => vi.unstubAllGlobals());

  it("keeps the signing identity when its read token is invalidated", () => {
    const jwk = {
      kty: "EC",
      crv: "P-256",
      x: "saved-x",
      y: "saved-y",
      d: "saved-d",
    } satisfies JsonWebKey;
    writeStoredAccount({
      accountId: 12,
      publicKeyHex: "02local",
      authScheme: "raw_p256",
      jwk,
      readApiKey: "sybk_stale",
    });

    clearStoredReadApiKey();

    expect(values.has(STORAGE_KEYS.READ_API_KEY)).toBe(false);
    expect(sessionValues.has(STORAGE_KEYS.READ_API_KEY)).toBe(false);
    expect(values.has(STORAGE_KEYS.REVISION)).toBe(true);
    expect(readStoredAccount()).toEqual({
      accountId: 12,
      publicKeyHex: "02local",
      authScheme: "raw_p256",
      jwk,
    });
  });

  it("keeps read bearer tokens out of persistent storage", () => {
    writeStoredAccount({
      accountId: 13,
      publicKeyHex: "02passkey",
      authScheme: "webauthn",
      credentialIdB64url: "credential",
      readApiKey: "sybk_session_only",
    });

    expect(values.has(STORAGE_KEYS.READ_API_KEY)).toBe(false);
    expect(sessionValues.get(STORAGE_KEYS.READ_API_KEY)).toBe(
      "sybk_session_only",
    );
    expect(readStoredAccount()?.readApiKey).toBe("sybk_session_only");
  });
});
