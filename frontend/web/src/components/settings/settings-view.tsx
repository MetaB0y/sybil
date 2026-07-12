"use client";

/**
 * /settings account-management view (SYB-60).
 *
 * Three sections:
 *   1. Profile — display name + identicon seed (signed set/clear).
 *   2. Signing keys — list + add agent trade key + revoke (signed).
 *   3. Read API keys — list + create (show-once token) + revoke (signed).
 *
 * SECURITY framing surfaced in copy: read API keys are READ-ONLY bearer tokens
 * that cannot trade; trade authority comes only from a registered signing key
 * (add one with scope "agent").
 */

import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { createPortal } from "react-dom";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { PageHeader } from "@/components/page-header";
import { Panel, PanelBody, PanelHead } from "@/components/dev/primitives/panel";
import {
  addAgentKey,
  createApiKey,
  revokeApiKey,
  revokeSigningKey,
  setProfile,
  SettingsActionError,
} from "@/lib/account/settings";
import { addBackupPasskey } from "@/lib/account/backup-passkey";
import type { AccountAuthScheme } from "@/lib/account/storage";
import { isWebAuthnAvailable } from "@/lib/auth/webauthn";
import { trapTabFocus } from "@/lib/accessibility/focus-trap";
import {
  settingsQueryKeys,
  useAccountProfile,
  useReadApiKeys,
  useSigningKeys,
  type ReadApiKey,
  type SigningKey,
} from "@/lib/account/use-settings-data";

export function SettingsView({
  accountId,
  publicKeyHex,
  authScheme,
  credentialIdB64url,
}: {
  accountId: number;
  publicKeyHex: string;
  authScheme: AccountAuthScheme;
  credentialIdB64url?: string;
}) {
  return (
    <>
      <PageHeader
        title="Settings"
        meta={`account #${accountId} · profile, signing keys & API keys`}
      />
      <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
        <ProfileSection accountId={accountId} publicKeyHex={publicKeyHex} />
        <SigningKeysSection
          accountId={accountId}
          publicKeyHex={publicKeyHex}
          authScheme={authScheme}
          {...(credentialIdB64url ? { credentialIdB64url } : {})}
        />
        <ReadApiKeysSection accountId={accountId} publicKeyHex={publicKeyHex} />
      </div>
    </>
  );
}

function SettingsSectionReadState({
  title,
  status,
  loadingMessage,
  errorMessage,
  onRetry,
  retrying,
}: {
  title: string;
  status: "loading" | "error";
  loadingMessage: string;
  errorMessage: string;
  onRetry: () => void;
  retrying: boolean;
}) {
  const failed = status === "error";
  return (
    <Panel>
      <PanelHead title={title} />
      <PanelBody>
        <div
          role={failed ? "alert" : "status"}
          aria-live={failed ? "assertive" : "polite"}
          aria-busy={!failed || retrying}
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "flex-start",
            gap: 10,
          }}
        >
          <p style={{ ...bodyText, margin: 0 }}>
            {failed ? errorMessage : loadingMessage}
          </p>
          {failed && (
            <button
              type="button"
              onClick={onRetry}
              disabled={retrying}
              className="settings-btn settings-btn--secondary"
            >
              {retrying ? "Retrying…" : "Retry"}
            </button>
          )}
        </div>
      </PanelBody>
    </Panel>
  );
}

// --- Section 1: Profile ---------------------------------------------------

function ProfileSection({
  accountId,
  publicKeyHex,
}: {
  accountId: number;
  publicKeyHex: string;
}) {
  const qc = useQueryClient();
  const profile = useAccountProfile(accountId);
  const [displayName, setDisplayName] = useState("");
  const [avatarSeed, setAvatarSeed] = useState("");
  const [error, setError] = useState<string | null>(null);

  // Seed the form from the loaded profile using React's render-time state-reset
  // pattern (https://react.dev/reference/react/useState#storing-information-from-previous-renders)
  // — no effect, so a refetch of unchanged data never clobbers in-flight edits.
  const loadedDisplay = profile.data?.display_name ?? "";
  const loadedSeed = profile.data?.avatar_seed ?? "";
  const loadedKey = profile.isSuccess
    ? `${loadedDisplay}\u0000${loadedSeed}`
    : null;
  const [seededKey, setSeededKey] = useState<string | null>(null);
  if (loadedKey !== null && loadedKey !== seededKey) {
    setSeededKey(loadedKey);
    setDisplayName(loadedDisplay);
    setAvatarSeed(loadedSeed);
  }

  const invalidate = () =>
    qc.invalidateQueries({ queryKey: settingsQueryKeys.profile(accountId) });

  const save = useMutation({
    mutationFn: async () => {
      const name = displayName.trim();
      const seed = avatarSeed.trim();
      await setProfile({
        accountId,
        publicKeyHex,
        displayName: name.length > 0 ? name : null,
        avatarSeed: seed.length > 0 ? seed : null,
      });
    },
    onSuccess: () => {
      setError(null);
      void invalidate();
    },
    onError: (e) => setError(messageOf(e)),
  });

  const clear = useMutation({
    mutationFn: async () => {
      await setProfile({
        accountId,
        publicKeyHex,
        displayName: null,
        avatarSeed: null,
      });
    },
    onSuccess: () => {
      setError(null);
      setDisplayName("");
      setAvatarSeed("");
      void invalidate();
    },
    onError: (e) => setError(messageOf(e)),
  });

  const previewSeed =
    avatarSeed.trim() || displayName.trim() || String(accountId);
  const busy = save.isPending || clear.isPending;

  if (!profile.isSuccess) {
    return (
      <SettingsSectionReadState
        title="Profile"
        status={profile.isError ? "error" : "loading"}
        loadingMessage="Loading your current profile before editing is enabled…"
        errorMessage="Your current profile could not be verified. Editing is disabled until this read succeeds."
        onRetry={() => void profile.refetch()}
        retrying={profile.isFetching}
      />
    );
  }

  return (
    <Panel>
      <PanelHead title="Profile" />
      <PanelBody style={{ display: "flex", flexDirection: "column", gap: 14 }}>
        <p style={{ ...bodyText, margin: 0 }}>
          A display name opts your account into the public leaderboard and
          publishes your account ID, PnL, ROI, open-market count, and equity.
          The name and identicon seed are public and covered by your signature.
          Clear the display name to remove future leaderboard publication.
        </p>

        <div className="settings-profile-grid">
          <Identicon seed={previewSeed} size={64} />
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 12,
              flex: 1,
              minWidth: 0,
            }}
          >
            <Field label="Display name">
              <input
                type="text"
                value={displayName}
                maxLength={32}
                onChange={(e) => setDisplayName(e.target.value)}
                placeholder="e.g. alice"
                className="settings-input"
              />
            </Field>
            <Field label="Identicon seed">
              <input
                type="text"
                value={avatarSeed}
                onChange={(e) => setAvatarSeed(e.target.value)}
                placeholder="any string — drives the avatar above"
                className="settings-input"
              />
            </Field>
          </div>
        </div>

        {error && <ErrorRow>{error}</ErrorRow>}

        <div className="settings-inline-form">
          <button
            type="button"
            onClick={() => save.mutate()}
            disabled={busy}
            className="settings-btn settings-btn--primary"
          >
            {save.isPending ? "Saving…" : "Save"}
          </button>
          <button
            type="button"
            onClick={() => clear.mutate()}
            disabled={busy}
            className="settings-btn settings-btn--secondary"
          >
            {clear.isPending ? "Clearing…" : "Clear"}
          </button>
        </div>
      </PanelBody>
    </Panel>
  );
}

// --- Section 2: Signing keys ----------------------------------------------

function SigningKeysSection({
  accountId,
  publicKeyHex,
  authScheme,
  credentialIdB64url,
}: {
  accountId: number;
  publicKeyHex: string;
  authScheme: AccountAuthScheme;
  credentialIdB64url?: string;
}) {
  const qc = useQueryClient();
  const keys = useSigningKeys(accountId);
  const [label, setLabel] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [newJwk, setNewJwk] = useState<{
    jwk: JsonWebKey;
    pubkey: string;
  } | null>(null);
  const addButtonRef = useRef<HTMLButtonElement>(null);

  const invalidate = () =>
    qc.invalidateQueries({
      queryKey: settingsQueryKeys.signingKeys(accountId),
    });

  const add = useMutation({
    mutationFn: async () => {
      const trimmed = label.trim();
      return addAgentKey({
        accountId,
        publicKeyHex,
        ...(trimmed ? { label: trimmed } : {}),
      });
    },
    onSuccess: (res) => {
      setError(null);
      setLabel("");
      setNewJwk({ jwk: res.jwk, pubkey: res.publicKeyHex });
      void invalidate();
    },
    onError: (e) => setError(messageOf(e)),
  });

  const revoke = useMutation({
    mutationFn: async (target: {
      publicKeyHex: string;
      authScheme: AccountAuthScheme;
    }) =>
      revokeSigningKey({
        accountId,
        publicKeyHex,
        targetPubkeyHex: target.publicKeyHex,
        targetAuthScheme: target.authScheme,
      }),
    onSuccess: () => {
      setError(null);
      void invalidate();
    },
    onError: (e) => setError(messageOf(e)),
  });

  const list = keys.data ?? [];
  const revokingTarget =
    revoke.isPending && revoke.variables ? revoke.variables.publicKeyHex : null;

  if (!keys.isSuccess) {
    return (
      <SettingsSectionReadState
        title="Signing keys / agent keys"
        status={keys.isError ? "error" : "loading"}
        loadingMessage="Loading the authoritative signing-key list before key management is enabled…"
        errorMessage="Your signing keys could not be verified. Key creation, revocation, and recovery setup are disabled until this read succeeds."
        onRetry={() => void keys.refetch()}
        retrying={keys.isFetching}
      />
    );
  }

  return (
    <Panel>
      <PanelHead title="Signing keys / agent keys" />
      <PanelBody style={{ display: "flex", flexDirection: "column", gap: 14 }}>
        <p style={{ ...bodyText, margin: 0 }}>
          Signing keys are P256 keypairs that can <strong>trade</strong> on your
          account. Add an <strong>agent key</strong> to give a bot trade
          authority — this is the only way to delegate trading (read API keys
          below cannot trade). The new key&apos;s private JWK is shown once;
          save it then.
        </p>
        <p style={{ ...bodyText, margin: 0 }}>
          To retire the key connected to this session, disconnect and sign in
          with a different registered key first. The current session key cannot
          revoke itself here.
        </p>

        <div
          style={{
            display: "flex",
            flexDirection: "column",
            border: "1px solid var(--border-2)",
            borderRadius: 6,
            overflow: "hidden",
          }}
        >
          {list.length === 0 ? (
            <EmptyRow>no signing keys</EmptyRow>
          ) : (
            list.map((k) => {
              const isSelf = signingPublicKeysEqual(
                k.public_key_hex,
                publicKeyHex,
              );
              return (
                <SigningKeyRow
                  key={k.public_key_hex}
                  keyItem={k}
                  isSelf={isSelf}
                  revokePolicy={signingKeyRevocationPolicy(list.length, isSelf)}
                  revoking={revokingTarget === k.public_key_hex}
                  onRevoke={() =>
                    revoke.mutate({
                      publicKeyHex: k.public_key_hex,
                      authScheme:
                        k.auth_scheme === "webauthn" ? "webauthn" : "raw_p256",
                    })
                  }
                />
              );
            })
          )}
        </div>

        {error && <ErrorRow>{error}</ErrorRow>}

        <div className="settings-inline-form">
          <Field label="Agent key label (optional)">
            <input
              type="text"
              value={label}
              onChange={(e) => setLabel(e.target.value)}
              placeholder="e.g. agent:pricer"
              className="settings-input"
            />
          </Field>
          <button
            ref={addButtonRef}
            type="button"
            onClick={() => add.mutate()}
            disabled={add.isPending}
            className="settings-btn settings-btn--primary"
          >
            {add.isPending ? "Adding…" : "Add agent key"}
          </button>
        </div>

        {authScheme === "webauthn" && credentialIdB64url && (
          <BackupPasskeyControl
            accountId={accountId}
            publicKeyHex={publicKeyHex}
            credentialIdB64url={credentialIdB64url}
            onAdded={() => void invalidate()}
          />
        )}
      </PanelBody>

      {newJwk && (
        <ShowOnceModal
          title="Agent key created"
          onClose={() => setNewJwk(null)}
          intro="This is the new agent key's private JWK. It signs trades on your account and is shown only once — store it somewhere safe now."
          secretLabel="Private key (JWK)"
          secret={JSON.stringify(newJwk.jwk)}
          restoreFocusRef={addButtonRef}
          extra={
            <InfoLine
              label="Public key"
              value={truncateMiddle(newJwk.pubkey)}
            />
          }
        />
      )}
    </Panel>
  );
}

function BackupPasskeyControl({
  accountId,
  publicKeyHex,
  credentialIdB64url,
  onAdded,
}: {
  accountId: number;
  publicKeyHex: string;
  credentialIdB64url: string;
  onAdded: () => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const passkeysAvailable = useSyncExternalStore(
    subscribeToPasskeyCapability,
    isWebAuthnAvailable,
    passkeysUnavailableOnServer,
  );

  const addBackup = useMutation({
    mutationFn: () =>
      addBackupPasskey({ accountId, publicKeyHex, credentialIdB64url }),
    onSuccess: () => {
      setError(null);
      setSuccess(true);
      onAdded();
    },
    onError: (cause) => {
      setSuccess(false);
      setError(backupPasskeyError(cause));
    },
  });

  const unavailable = !passkeysAvailable;
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 10,
        paddingTop: 14,
        borderTop: "1px solid var(--border-2)",
      }}
    >
      <div style={{ ...bodyText, margin: 0 }}>
        <strong>Recovery passkey.</strong> Add another passkey before losing
        access to every registered authenticator. Sybil never receives or can
        reset either credential. Requires passkey support in this browser.
      </div>
      {error && <ErrorRow>{error}</ErrorRow>}
      {success && (
        <div role="status" style={{ ...bodyText, color: "var(--yes)" }}>
          Backup passkey added. Disconnect and test it before relying on it.
        </div>
      )}
      <div className="settings-inline-form">
        <button
          type="button"
          onClick={() => {
            setError(null);
            setSuccess(false);
            addBackup.mutate();
          }}
          disabled={unavailable || addBackup.isPending}
          className="settings-btn settings-btn--primary"
        >
          {addBackup.isPending ? "Adding passkey…" : "Add backup passkey"}
        </button>
      </div>
    </div>
  );
}

function subscribeToPasskeyCapability(): () => void {
  return () => {};
}

function passkeysUnavailableOnServer(): boolean {
  return false;
}

function backupPasskeyError(cause: unknown): string {
  if (
    cause instanceof DOMException &&
    (cause.name === "NotAllowedError" || cause.name === "AbortError")
  ) {
    return "Passkey creation was cancelled.";
  }
  if (cause instanceof SettingsActionError) return cause.message;
  return messageOf(cause);
}

function SigningKeyRow({
  keyItem,
  isSelf,
  revokePolicy,
  revoking,
  onRevoke,
}: {
  keyItem: SigningKey;
  isSelf: boolean;
  revokePolicy: SigningKeyRevocationPolicy;
  revoking: boolean;
  onRevoke: () => void;
}) {
  return (
    <div className="settings-row">
      <ScopeBadge scope={keyItem.scope} />
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: 2,
          minWidth: 0,
        }}
      >
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 12,
            color: "var(--fg-1)",
          }}
        >
          {truncateMiddle(keyItem.public_key_hex)}
          {isSelf && <SelfTag />}
        </span>
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            color: "var(--fg-4)",
          }}
        >
          {keyItem.label ? `${keyItem.label} · ` : ""}
          {keyItem.auth_scheme}
          {keyItem.created_at_ms > 0
            ? ` · ${formatMs(keyItem.created_at_ms)}`
            : ""}
        </span>
      </div>
      <div className="settings-row-actions" style={{ marginLeft: "auto" }}>
        <button
          type="button"
          onClick={onRevoke}
          disabled={!revokePolicy.canRevoke || revoking}
          title={revokePolicy.title}
          className="settings-btn settings-btn--danger"
        >
          {revoking ? "Revoking…" : "Revoke"}
        </button>
      </div>
    </div>
  );
}

export type SigningKeyRevocationPolicy = {
  canRevoke: boolean;
  title: string;
};

export function signingPublicKeysEqual(left: string, right: string): boolean {
  const normalize = (value: string) => value.replace(/^0x/i, "").toLowerCase();
  return normalize(left) === normalize(right);
}

/**
 * Keep the browser session usable while rotating signing keys. The backend
 * still permits a key to revoke itself when another key remains, but the web
 * UI requires users to prove the replacement works by reconnecting with it
 * first. The backend's last-key protection remains a separate final guard.
 */
export function signingKeyRevocationPolicy(
  activeKeyCount: number,
  isCurrentSessionKey: boolean,
): SigningKeyRevocationPolicy {
  if (activeKeyCount <= 1) {
    return {
      canRevoke: false,
      title: "Cannot revoke the last remaining key",
    };
  }
  if (isCurrentSessionKey) {
    return {
      canRevoke: false,
      title: "Reconnect with another registered key before revoking this one",
    };
  }
  return { canRevoke: true, title: "Revoke this key" };
}

// --- Section 3: Read API keys ---------------------------------------------

function ReadApiKeysSection({
  accountId,
  publicKeyHex,
}: {
  accountId: number;
  publicKeyHex: string;
}) {
  const qc = useQueryClient();
  const apiKeys = useReadApiKeys(accountId);
  const [label, setLabel] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [newToken, setNewToken] = useState<{
    token: string;
    id: number;
  } | null>(null);
  const createButtonRef = useRef<HTMLButtonElement>(null);

  const invalidate = () =>
    qc.invalidateQueries({ queryKey: settingsQueryKeys.apiKeys(accountId) });

  const create = useMutation({
    mutationFn: async () => {
      const trimmed = label.trim();
      return createApiKey({
        accountId,
        publicKeyHex,
        ...(trimmed ? { label: trimmed } : {}),
      });
    },
    onSuccess: (res) => {
      setError(null);
      setLabel("");
      setNewToken({ token: res.token, id: res.id });
      void invalidate();
    },
    onError: (e) => setError(messageOf(e)),
  });

  const revoke = useMutation({
    mutationFn: async (apiKeyId: number) =>
      revokeApiKey({ accountId, publicKeyHex, apiKeyId }),
    onSuccess: () => {
      setError(null);
      void invalidate();
    },
    onError: (e) => setError(messageOf(e)),
  });

  const list = apiKeys.data ?? [];
  const revokingId =
    revoke.isPending && typeof revoke.variables === "number"
      ? revoke.variables
      : null;

  if (!apiKeys.isSuccess) {
    return (
      <SettingsSectionReadState
        title="Read API keys"
        status={apiKeys.isError ? "error" : "loading"}
        loadingMessage="Loading the authoritative read-key list before API-key management is enabled…"
        errorMessage="Your read API keys could not be verified. Key creation and revocation are disabled until this read succeeds."
        onRetry={() => void apiKeys.refetch()}
        retrying={apiKeys.isFetching}
      />
    );
  }

  return (
    <Panel>
      <PanelHead title="Read API keys" />
      <PanelBody style={{ display: "flex", flexDirection: "column", gap: 14 }}>
        <p style={{ ...bodyText, margin: 0 }}>
          Read API keys are <strong>read-only bearer tokens</strong> (
          <code style={codeStyle}>sybk_…</code>) for dashboards and scripts.
          They can read your account but <strong>cannot trade</strong> — to give
          an agent trade authority, register an agent signing key above. Tokens
          are shown once at creation.
        </p>

        <div
          style={{
            display: "flex",
            flexDirection: "column",
            border: "1px solid var(--border-2)",
            borderRadius: 6,
            overflow: "hidden",
          }}
        >
          {list.length === 0 ? (
            <EmptyRow>no read API keys</EmptyRow>
          ) : (
            list.map((k) => (
              <ApiKeyRow
                key={k.id}
                keyItem={k}
                revoking={revokingId === k.id}
                onRevoke={() => revoke.mutate(k.id)}
              />
            ))
          )}
        </div>

        {error && <ErrorRow>{error}</ErrorRow>}

        <div className="settings-inline-form">
          <Field label="API key label (optional)">
            <input
              type="text"
              value={label}
              onChange={(e) => setLabel(e.target.value)}
              placeholder="e.g. grafana"
              className="settings-input"
            />
          </Field>
          <button
            ref={createButtonRef}
            type="button"
            onClick={() => create.mutate()}
            disabled={create.isPending}
            className="settings-btn settings-btn--primary"
          >
            {create.isPending ? "Creating…" : "Create API key"}
          </button>
        </div>
      </PanelBody>

      {newToken && (
        <ShowOnceModal
          title="Read API key created"
          onClose={() => setNewToken(null)}
          intro="This is a READ-ONLY bearer token. It can read your account but cannot trade. It is shown only once — copy it now; the server keeps only a hash."
          secretLabel="Bearer token"
          secret={newToken.token}
          restoreFocusRef={createButtonRef}
          extra={<InfoLine label="Key id" value={`#${newToken.id}`} />}
        />
      )}
    </Panel>
  );
}

function ApiKeyRow({
  keyItem,
  revoking,
  onRevoke,
}: {
  keyItem: ReadApiKey;
  revoking: boolean;
  onRevoke: () => void;
}) {
  const revoked = keyItem.revoked_at_ms != null;
  return (
    <div className="settings-row" style={{ opacity: revoked ? 0.55 : 1 }}>
      <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 12,
            color: "var(--fg-1)",
          }}
        >
          {keyItem.label || `key #${keyItem.id}`}
          {revoked && (
            <span
              style={{
                marginLeft: 8,
                fontSize: 10,
                color: "var(--fg-4)",
                textTransform: "uppercase",
                letterSpacing: "var(--track-wide)",
              }}
            >
              revoked
            </span>
          )}
        </span>
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            color: "var(--fg-4)",
          }}
        >
          #{keyItem.id} · created {formatMs(keyItem.created_at_ms)}
          {revoked ? ` · revoked ${formatMs(keyItem.revoked_at_ms!)}` : ""}
        </span>
      </div>
      <div className="settings-row-actions" style={{ marginLeft: "auto" }}>
        {!revoked && (
          <button
            type="button"
            onClick={onRevoke}
            disabled={revoking}
            className="settings-btn settings-btn--danger"
          >
            {revoking ? "Revoking…" : "Revoke"}
          </button>
        )}
      </div>
    </div>
  );
}

// --- Show-once modal (portal, mirrors connect-modal) ----------------------

function ShowOnceModal({
  title,
  intro,
  secretLabel,
  secret,
  extra,
  restoreFocusRef,
  onClose,
}: {
  title: string;
  intro: string;
  secretLabel: string;
  secret: string;
  extra?: React.ReactNode;
  restoreFocusRef?: React.RefObject<HTMLElement | null>;
  onClose: () => void;
}) {
  const [copyState, setCopyState] = useState<
    "idle" | "copying" | "copied" | "failed"
  >("idle");
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const secretRef = useRef<HTMLTextAreaElement>(null);
  const copyResetTimerRef = useRef<number | null>(null);
  const mountedRef = useRef(false);
  const copyRequestRef = useRef(0);

  useEffect(() => {
    mountedRef.current = true;
    const previouslyFocused =
      restoreFocusRef?.current ??
      (document.activeElement instanceof HTMLElement &&
      document.activeElement !== document.body
        ? document.activeElement
        : null);
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const focusFrame = window.requestAnimationFrame(() => {
      closeButtonRef.current?.focus();
    });
    return () => {
      mountedRef.current = false;
      copyRequestRef.current += 1;
      window.cancelAnimationFrame(focusFrame);
      if (copyResetTimerRef.current !== null) {
        window.clearTimeout(copyResetTimerRef.current);
      }
      document.body.style.overflow = previousOverflow;
      if (previouslyFocused?.isConnected) previouslyFocused.focus();
    };
  }, [restoreFocusRef]);

  async function onCopy() {
    if (copyState === "copying") return;
    if (copyResetTimerRef.current !== null) {
      window.clearTimeout(copyResetTimerRef.current);
      copyResetTimerRef.current = null;
    }
    const request = copyRequestRef.current + 1;
    copyRequestRef.current = request;
    setCopyState("copying");
    const clipboard =
      typeof navigator === "undefined" ? undefined : navigator.clipboard;
    const copied = await writeShowOnceSecret(secret, clipboard);
    if (!mountedRef.current || copyRequestRef.current !== request) return;
    if (copied) {
      setCopyState("copied");
      copyResetTimerRef.current = window.setTimeout(() => {
        setCopyState("idle");
        copyResetTimerRef.current = null;
      }, 1500);
      return;
    }
    setCopyState("failed");
    secretRef.current?.focus();
    secretRef.current?.select();
  }

  if (typeof document === "undefined") return null;

  return createPortal(
    <div
      role="dialog"
      aria-modal="true"
      aria-label={title}
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          onClose();
          return;
        }
        trapTabFocus(event.nativeEvent, event.currentTarget);
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      style={{
        position: "fixed",
        inset: 0,
        background: "var(--overlay)",
        backdropFilter: "blur(6px)",
        WebkitBackdropFilter: "blur(6px)",
        zIndex: 100,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        padding: "var(--space-5)",
        boxSizing: "border-box",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          width: "100%",
          maxWidth: 480,
          maxHeight: "100%",
          display: "flex",
          flexDirection: "column",
          background: "var(--surface-1)",
          border: "1px solid var(--border-1)",
          borderRadius: 12,
          boxShadow: "0 20px 60px rgba(0,0,0,0.4)",
          overflow: "hidden",
          fontFamily: "var(--font-sans)",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            padding: "14px 18px",
            borderBottom: "1px solid var(--border-1)",
          }}
        >
          <div
            style={{
              fontFamily: "var(--font-display)",
              fontWeight: 700,
              fontSize: 16,
              color: "var(--fg-1)",
              letterSpacing: "var(--track-tight)",
              textTransform: "uppercase",
            }}
          >
            {title}
          </div>
          <button
            ref={closeButtonRef}
            type="button"
            onClick={onClose}
            aria-label="Close"
            style={{
              background: "transparent",
              border: 0,
              color: "var(--fg-3)",
              fontSize: 20,
              cursor: "pointer",
              padding: 0,
              lineHeight: 1,
            }}
          >
            ×
          </button>
        </div>
        <div
          style={{
            padding: "16px 18px 18px",
            display: "flex",
            flexDirection: "column",
            gap: 12,
            overflowY: "auto",
            overscrollBehavior: "contain",
          }}
        >
          <p style={{ ...bodyText, margin: 0 }}>{intro}</p>
          {extra}
          <Field label={secretLabel}>
            <textarea
              ref={secretRef}
              readOnly
              value={secret}
              rows={3}
              onFocus={(e) => e.currentTarget.select()}
              style={{
                ...inputStyle,
                fontFamily: "var(--font-mono)",
                fontSize: 12,
                resize: "vertical",
                wordBreak: "break-all",
              }}
            />
          </Field>
          {copyState === "failed" && (
            <div role="alert" style={copyFailureStyle}>
              Copy failed. The credential above is selected—copy it manually
              before closing this one-time view.
            </div>
          )}
          <div style={{ display: "flex", gap: 8 }}>
            <button
              type="button"
              onClick={() => void onCopy()}
              aria-disabled={copyState === "copying"}
              aria-busy={copyState === "copying"}
              className="settings-btn settings-btn--primary"
            >
              {copyState === "copied"
                ? "Copied ✓"
                : copyState === "copying"
                  ? "Copying…"
                  : copyState === "failed"
                    ? "Try copy again"
                    : "Copy"}
            </button>
            <button
              type="button"
              onClick={onClose}
              className="settings-btn settings-btn--secondary"
            >
              Done
            </button>
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}

export async function writeShowOnceSecret(
  secret: string,
  clipboard: Pick<Clipboard, "writeText"> | undefined,
): Promise<boolean> {
  if (!clipboard) return false;
  try {
    await clipboard.writeText(secret);
    return true;
  } catch {
    return false;
  }
}

// --- Identicon (deterministic inline SVG) ---------------------------------

/**
 * GitHub-style 5×5 mirrored identicon derived from a string seed. Pure hash →
 * no assets, no network. Same seed always renders the same avatar.
 */
function Identicon({ seed, size }: { seed: string; size: number }) {
  const { cells, color } = useMemo(() => buildIdenticon(seed), [seed]);
  const cell = size / 5;
  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      role="img"
      aria-label={`identicon for ${seed}`}
      style={{
        borderRadius: 8,
        background: "var(--bg-2)",
        border: "1px solid var(--border-2)",
        flexShrink: 0,
      }}
    >
      {cells.map((on, i) =>
        on ? (
          <rect
            key={i}
            x={(i % 5) * cell}
            y={Math.floor(i / 5) * cell}
            width={cell}
            height={cell}
            fill={color}
          />
        ) : null,
      )}
    </svg>
  );
}

function buildIdenticon(seed: string): { cells: boolean[]; color: string } {
  // FNV-1a 32-bit hash — deterministic across runtimes.
  let h = 0x811c9dc5;
  for (let i = 0; i < seed.length; i++) {
    h ^= seed.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  const hash = h >>> 0;
  const hue = hash % 360;
  const color = `hsl(${hue} 62% 52%)`;
  // Fill a 5×5 grid mirrored across the vertical axis: decide the left 3 cols
  // (15 bits) and reflect columns 3,4 from cols 1,0.
  const cells: boolean[] = new Array(25).fill(false);
  let bits = hash;
  for (let row = 0; row < 5; row++) {
    for (let col = 0; col < 3; col++) {
      const on = (bits & 1) === 1;
      bits = bits >>> 1;
      if (bits === 0) bits = hash | 1; // reseed so we never run dry
      cells[row * 5 + col] = on;
      cells[row * 5 + (4 - col)] = on;
    }
  }
  return { cells, color };
}

// --- small shared UI ------------------------------------------------------

function ScopeBadge({ scope }: { scope: string }) {
  const tone =
    scope === "agent"
      ? { fg: "var(--accent)", bg: "var(--accent-soft, var(--surface-2))" }
      : scope === "primary"
        ? { fg: "var(--fg-2)", bg: "var(--surface-2)" }
        : { fg: "var(--fg-3)", bg: "var(--surface-2)" };
  return (
    <span
      style={{
        padding: "2px 7px",
        borderRadius: 999,
        background: tone.bg,
        color: tone.fg,
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
        flexShrink: 0,
      }}
    >
      {scope}
    </span>
  );
}

function SelfTag() {
  return (
    <span
      style={{
        marginLeft: 8,
        fontSize: 10,
        color: "var(--fg-4)",
        textTransform: "uppercase",
        letterSpacing: "var(--track-wide)",
      }}
    >
      this session
    </span>
  );
}

function InfoLine({ label, value }: { label: string; value: string }) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "space-between",
        gap: 12,
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        color: "var(--fg-3)",
      }}
    >
      <span style={{ color: "var(--fg-4)" }}>{label}</span>
      <span style={{ color: "var(--fg-2)" }}>{value}</span>
    </div>
  );
}

export function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <label
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 6,
        flex: 1,
        minWidth: 0,
      }}
    >
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 10,
          color: "var(--fg-3)",
          letterSpacing: "var(--track-wide)",
          textTransform: "uppercase",
        }}
      >
        {label}
      </span>
      {children}
    </label>
  );
}

function EmptyRow({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        padding: "12px",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        color: "var(--fg-4)",
      }}
    >
      {children}
    </div>
  );
}

function ErrorRow({ children }: { children: React.ReactNode }) {
  return (
    <div
      role="alert"
      style={{
        padding: "8px 10px",
        background:
          "var(--no-soft, color-mix(in srgb, var(--no) 12%, transparent))",
        border: "1px solid color-mix(in srgb, var(--no) 32%, transparent)",
        borderRadius: 6,
        color: "var(--no)",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
      }}
    >
      {children}
    </div>
  );
}

// --- helpers --------------------------------------------------------------

export function settingsActionMessage(e: unknown): string {
  if (e instanceof SettingsActionError) {
    if (e.status === 409) {
      const detail = e.message.toLowerCase();
      if (detail.includes("last remaining signing key")) {
        return "Cannot revoke the last key on this account — add another key first.";
      }
      if (detail.includes("stale key-operation state binding")) {
        return "The account key state changed before this action landed. Review the current key list and try again.";
      }
      if (
        detail.includes("replay nonce") &&
        (detail.includes("stale") || detail.includes("duplicate"))
      ) {
        return "The account changed before this action landed. Review the latest state and try again.";
      }
    }
    return e.message;
  }
  return e instanceof Error ? e.message : "Action failed";
}

const messageOf = settingsActionMessage;

function truncateMiddle(s: string, head = 8, tail = 6): string {
  if (s.length <= head + tail + 1) return s;
  return `${s.slice(0, head)}…${s.slice(-tail)}`;
}

function formatMs(ms: number): string {
  if (!ms) return "—";
  try {
    return new Date(ms).toISOString().slice(0, 10);
  } catch {
    return "—";
  }
}

const bodyText: React.CSSProperties = {
  fontFamily: "var(--font-sans)",
  fontSize: 13,
  color: "var(--fg-3)",
  lineHeight: 1.5,
};

const codeStyle: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 12,
  color: "var(--fg-2)",
  background: "var(--surface-2)",
  padding: "1px 5px",
  borderRadius: 4,
};

const inputStyle: React.CSSProperties = {
  background: "var(--bg-2)",
  border: "1px solid var(--border-1)",
  borderRadius: 6,
  minHeight: 40,
  padding: "8px 10px",
  color: "var(--fg-1)",
  fontFamily: "var(--font-sans)",
  fontSize: 13,
  outline: "none",
  width: "100%",
  boxSizing: "border-box",
};

const copyFailureStyle: React.CSSProperties = {
  padding: "8px 10px",
  background: "color-mix(in srgb, var(--warn) 10%, transparent)",
  border: "1px solid color-mix(in srgb, var(--warn) 35%, transparent)",
  borderRadius: 6,
  color: "var(--warn)",
  fontFamily: "var(--font-mono)",
  fontSize: 12,
  lineHeight: 1.45,
};
