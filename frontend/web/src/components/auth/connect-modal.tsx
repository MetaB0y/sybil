"use client";

/**
 * Connect modal — wired into the layout so any "connect to trade" CTA can
 * just call `setConnectModalOpen(true)`. Two tabs:
 *  - **Create demo**: balance chip selector → POST /v1/accounts (dev-mode)
 *  - **Import existing**: paste account_id + JWK
 *
 * Render-gated: only mounted when `connectModalOpen` is true. Uses a portal
 * to `document.body` so z-index is independent of nav layout.
 */

import { useEffect, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import {
  AccountError,
  type CreateAccountKeyMode,
  createDemoAccount,
  importExistingAccount,
  signInWithDiscoverablePasskey,
  signInWithStoredPasskey,
} from "@/lib/account/actions";
import { readStoredAccount } from "@/lib/account/storage";
import {
  useConnectModalOpen,
  useSetConnectModalOpen,
} from "@/lib/account/use-account";
import { isWebAuthnAvailable } from "@/lib/auth/webauthn";
import { trapTabFocus } from "@/lib/accessibility/focus-trap";

const BALANCE_OPTIONS: Array<{ label: string; nanos: bigint }> = [
  { label: "$100", nanos: 100_000_000_000n },
  { label: "$500", nanos: 500_000_000_000n },
  { label: "$1,000", nanos: 1_000_000_000_000n },
  { label: "$5,000", nanos: 5_000_000_000_000n },
];

type Tab = "create" | "passkey" | "import";

export function ConnectModal() {
  const open = useConnectModalOpen();
  const setOpen = useSetConnectModalOpen();
  const closeButtonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    const previouslyFocused =
      document.activeElement instanceof HTMLElement &&
      document.activeElement !== document.body
        ? document.activeElement
        : null;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const focusFrame = window.requestAnimationFrame(() => {
      closeButtonRef.current?.focus();
    });

    return () => {
      window.cancelAnimationFrame(focusFrame);
      document.body.style.overflow = previousOverflow;
      if (previouslyFocused?.isConnected) previouslyFocused.focus();
    };
  }, [open, setOpen]);

  // Modal only opens after a client-side interaction, so `document` is
  // always available by the time we reach createPortal. The redundant guard
  // also protects against accidental SSR rendering of the open state.
  if (!open || typeof document === "undefined") return null;

  return createPortal(
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="connect-modal-title"
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          setOpen(false);
          return;
        }
        trapTabFocus(event.nativeEvent, event.currentTarget);
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget) setOpen(false);
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
      }}
    >
      <ConnectModalBody
        closeButtonRef={closeButtonRef}
        onClose={() => setOpen(false)}
      />
    </div>,
    document.body,
  );
}

function ConnectModalBody({
  closeButtonRef,
  onClose,
}: {
  closeButtonRef: RefObject<HTMLButtonElement | null>;
  onClose: () => void;
}) {
  const [tab, setTab] = useState<Tab>("create");

  return (
    <div
      onClick={(e) => e.stopPropagation()}
      style={{
        width: "100%",
        maxWidth: 440,
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
          id="connect-modal-title"
          style={{
            fontFamily: "var(--font-display)",
            fontWeight: 700,
            fontSize: 16,
            color: "var(--fg-1)",
            letterSpacing: "var(--track-tight)",
            textTransform: "uppercase",
          }}
        >
          Connect
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
          display: "flex",
          gap: 4,
          padding: "10px 18px 0",
        }}
      >
        <TabButton active={tab === "create"} onClick={() => setTab("create")}>
          Create demo
        </TabButton>
        <TabButton active={tab === "passkey"} onClick={() => setTab("passkey")}>
          Passkey
        </TabButton>
        <TabButton active={tab === "import"} onClick={() => setTab("import")}>
          Import existing
        </TabButton>
      </div>

      <div style={{ padding: "16px 18px 18px" }}>
        {tab === "create" ? (
          <CreateTab />
        ) : tab === "passkey" ? (
          <PasskeyTab />
        ) : (
          <ImportTab />
        )}
      </div>
    </div>
  );
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        padding: "8px 12px",
        background: active ? "var(--surface-2)" : "transparent",
        border: 0,
        borderBottom: active
          ? "2px solid var(--accent)"
          : "2px solid transparent",
        color: active ? "var(--fg-1)" : "var(--fg-3)",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
        cursor: "pointer",
      }}
    >
      {children}
    </button>
  );
}

function CreateTab() {
  const [balanceNanos, setBalanceNanos] = useState<bigint>(
    BALANCE_OPTIONS[2]!.nanos,
  );
  const [mode, setMode] = useState<CreateAccountKeyMode>(
    isWebAuthnAvailable() ? "passkey" : "local_key",
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit() {
    setError(null);
    setBusy(true);
    try {
      await createDemoAccount(balanceNanos, mode);
    } catch (e) {
      const msg =
        e instanceof AccountError && e.kind === "dev_mode_off"
          ? "Demo accounts are disabled on this server. Import an existing account instead."
          : e instanceof AccountError && e.kind === "webauthn_unavailable"
            ? "Passkeys are not available in this browser."
            : e instanceof Error
              ? e.message
              : "Failed to create account";
      setError(msg);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <p style={{ ...bodyText, margin: 0 }}>
        Create a demo account with a starting balance. Use a passkey for
        device-backed signing, or a local key for browser-only testing.
      </p>

      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <Label>Key</Label>
        <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
          <button
            type="button"
            onClick={() => setMode("passkey")}
            disabled={!isWebAuthnAvailable()}
            style={chipStyle(mode === "passkey")}
          >
            Passkey
          </button>
          <button
            type="button"
            onClick={() => setMode("local_key")}
            style={chipStyle(mode === "local_key")}
          >
            Local browser key
          </button>
        </div>
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <Label>Starting balance</Label>
        <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
          {BALANCE_OPTIONS.map((opt) => {
            const active = opt.nanos === balanceNanos;
            return (
              <button
                key={opt.label}
                type="button"
                onClick={() => setBalanceNanos(opt.nanos)}
                style={chipStyle(active)}
              >
                {opt.label}
              </button>
            );
          })}
        </div>
      </div>

      {error && <ErrorRow>{error}</ErrorRow>}

      <button
        type="button"
        onClick={onSubmit}
        disabled={busy}
        style={primaryButtonStyle(busy)}
      >
        {busy
          ? "Creating…"
          : mode === "passkey"
            ? "Create with passkey"
            : "Create local account"}
      </button>
    </div>
  );
}

function PasskeyTab() {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const saved = typeof window === "undefined" ? null : readStoredAccount();
  const hasSavedPasskey = saved?.authScheme === "webauthn";

  async function onSubmit() {
    setError(null);
    setBusy(true);
    try {
      if (hasSavedPasskey) {
        await signInWithStoredPasskey();
      } else {
        await signInWithDiscoverablePasskey();
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "Passkey sign-in failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <p style={{ ...bodyText, margin: 0 }}>
        Choose a passkey from this device. Saved accounts reconnect directly;
        otherwise Sybil recovers the account from the passkey.
      </p>

      {error && <ErrorRow>{error}</ErrorRow>}

      <button
        type="button"
        onClick={onSubmit}
        disabled={busy}
        style={primaryButtonStyle(busy)}
      >
        {busy ? "Checking…" : "Sign in with passkey"}
      </button>
    </div>
  );
}

function ImportTab() {
  const [accountIdRaw, setAccountIdRaw] = useState("");
  const [jwkRaw, setJwkRaw] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit() {
    setError(null);
    const id = Number.parseInt(accountIdRaw.trim(), 10);
    if (!Number.isFinite(id) || id < 0) {
      setError("Account id must be a non-negative integer");
      return;
    }
    let jwk: JsonWebKey;
    try {
      jwk = JSON.parse(jwkRaw) as JsonWebKey;
    } catch {
      setError("JWK is not valid JSON");
      return;
    }
    setBusy(true);
    try {
      await importExistingAccount(id, jwk);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Import failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <p style={{ ...bodyText, margin: 0 }}>
        Paste an account id and the JWK from a previous browser session. The
        public key is derived from the JWK. We don&apos;t verify the key matches
        what&apos;s registered server-side until you place an order.
      </p>

      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <Label>Account id</Label>
        <input
          type="text"
          value={accountIdRaw}
          onChange={(e) => setAccountIdRaw(e.target.value)}
          placeholder="e.g. 13"
          style={inputStyle}
        />
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <Label>Private key (JWK JSON)</Label>
        <textarea
          value={jwkRaw}
          onChange={(e) => setJwkRaw(e.target.value)}
          placeholder='{"kty":"EC","crv":"P-256","x":"…","y":"…","d":"…"}'
          rows={6}
          style={{
            ...inputStyle,
            fontFamily: "var(--font-mono)",
            resize: "vertical",
          }}
        />
      </div>

      {error && <ErrorRow>{error}</ErrorRow>}

      <button
        type="button"
        onClick={onSubmit}
        disabled={busy}
        style={primaryButtonStyle(busy)}
      >
        {busy ? "Importing…" : "Import"}
      </button>
    </div>
  );
}

// --- styles ---------------------------------------------------------------

function Label({ children }: { children: React.ReactNode }) {
  return (
    <label
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        color: "var(--fg-3)",
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
      }}
    >
      {children}
    </label>
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

const bodyText: React.CSSProperties = {
  fontFamily: "var(--font-sans)",
  fontSize: 13,
  color: "var(--fg-3)",
  lineHeight: 1.5,
};

const inputStyle: React.CSSProperties = {
  background: "var(--bg-2)",
  border: "1px solid var(--border-1)",
  borderRadius: 6,
  padding: "8px 10px",
  color: "var(--fg-1)",
  fontFamily: "var(--font-sans)",
  fontSize: 13,
  outline: "none",
};

function chipStyle(active: boolean): React.CSSProperties {
  return {
    padding: "6px 12px",
    background: active ? "var(--surface-2)" : "var(--bg-2)",
    border: active ? "1px solid var(--accent)" : "1px solid var(--border-1)",
    borderRadius: 999,
    color: active ? "var(--fg-1)" : "var(--fg-2)",
    fontFamily: "var(--font-mono)",
    fontSize: 12,
    cursor: "pointer",
  };
}

function primaryButtonStyle(busy: boolean): React.CSSProperties {
  return {
    padding: "10px 14px",
    background: busy ? "var(--surface-2)" : "var(--accent)",
    border: 0,
    borderRadius: 8,
    color: busy ? "var(--fg-3)" : "var(--bg-1)",
    fontFamily: "var(--font-sans)",
    fontWeight: 600,
    fontSize: 14,
    cursor: busy ? "not-allowed" : "pointer",
  };
}
