"use client";

/**
 * Nav chip showing the current account. Disconnected → "connect" button
 * (opens modal). Connected → the live portfolio "total / cash ▾" with a small
 * dropdown that recaps portfolio / cash / account id and offers disconnect /
 * copy account id / copy JWK.
 */

import { useEffect, useRef, useState } from "react";
import { disconnect } from "@/lib/account/actions";
import { readStoredAccount } from "@/lib/account/storage";
import {
  useAccountHydrated,
  useAccountSession,
  useSetConnectModalOpen,
} from "@/lib/account/use-account";
import { selectBalances } from "@/lib/account/use-available-balance";
import { usePortfolio } from "@/lib/account/use-portfolio";
import { formatDollars, parseNanos } from "@/lib/format/nanos";

export function AccountChip() {
  const session = useAccountSession();
  const hydrated = useAccountHydrated();
  const setOpen = useSetConnectModalOpen();

  // Server render + pre-hydration: render a stable placeholder so React
  // doesn't tear during hydration. Style matches the connect button.
  if (!hydrated) {
    return <ChipShell label="…" disabled />;
  }

  if (!session) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        style={chipButtonStyle(false)}
        title="Create or import an account"
      >
        connect
      </button>
    );
  }

  return <ConnectedMenu accountId={session.accountId} />;
}

function ConnectedMenu({ accountId }: { accountId: number }) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const portfolio = usePortfolio(accountId).data ?? null;

  // Which auth scheme backs this account decides whether a real key backup is
  // possible. raw_p256 stores an extractable JWK (copyable = a real backup);
  // webauthn/passkey has NO exportable secret — the credential id is just a
  // handle, not a restore key — so we must not offer it as "backup". Read
  // lazily from localStorage; this menu only mounts post-hydration (AccountChip
  // gates on `hydrated`), so there's no SSR/first-paint mismatch.
  const [authScheme] = useState<"raw_p256" | "webauthn" | null>(
    () => readStoredAccount()?.authScheme ?? null,
  );

  const total =
    portfolio != null
      ? formatDollars(parseNanos(portfolio.portfolio_value_nanos), { decimals: 2 })
      : "—";
  // "Cash" is the AVAILABLE (spendable) balance — total minus funds reserved by
  // open orders — so the chip never implies more buying power than the engine
  // will accept. Reserved is surfaced separately when non-zero.
  const { availableNanos, reservedNanos } = selectBalances(portfolio);
  const cash =
    availableNanos != null
      ? formatDollars(availableNanos, { decimals: 2 })
      : "—";
  const reserved =
    reservedNanos > 0n ? formatDollars(reservedNanos, { decimals: 2 }) : null;

  useEffect(() => {
    if (!open) return;
    function onClick(e: MouseEvent) {
      if (!rootRef.current) return;
      if (!rootRef.current.contains(e.target as Node)) setOpen(false);
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    document.addEventListener("mousedown", onClick);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onClick);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div ref={rootRef} style={{ position: "relative" }}>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-haspopup="menu"
        aria-expanded={open}
        style={chipButtonStyle(true)}
        title={`Portfolio ${total} · Available ${cash}${reserved ? ` · Reserved ${reserved}` : ""} · Account #${accountId}`}
      >
        <span style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
          <span style={{ color: "var(--fg-1)" }}>{total}</span>
          <span aria-hidden style={{ color: "var(--fg-4)" }}>
            /
          </span>
          <span style={{ color: "var(--fg-3)" }}>{cash}</span>
        </span>
        <span aria-hidden style={{ marginLeft: 6, color: "var(--fg-4)" }}>
          ▾
        </span>
      </button>
      {open && (
        <div
          role="menu"
          style={{
            position: "absolute",
            top: "calc(100% + 6px)",
            right: 0,
            minWidth: 200,
            background: "var(--surface-1)",
            border: "1px solid var(--border-1)",
            borderRadius: 8,
            boxShadow: "0 12px 32px rgba(0,0,0,0.4)",
            padding: 4,
            zIndex: 60,
          }}
        >
          <div style={{ padding: "6px 10px 8px" }}>
            <InfoRow label="Portfolio" value={total} strong />
            <InfoRow label="Available" value={cash} />
            {reserved && <InfoRow label="Reserved" value={reserved} />}
            <InfoRow label="Account" value={`#${accountId}`} />
          </div>
          <div style={{ height: 1, background: "var(--border-1)", margin: "4px 0" }} />
          <MenuItem
            onClick={() => {
              void navigator.clipboard?.writeText(String(accountId));
              setOpen(false);
            }}
          >
            Copy account id
          </MenuItem>
          {authScheme === "webauthn" ? (
            <PasskeyNotice />
          ) : (
            <MenuItem
              onClick={() => {
                const stored = readStoredAccount();
                if (stored?.authScheme === "raw_p256" && stored.jwk) {
                  void navigator.clipboard?.writeText(
                    JSON.stringify(stored.jwk),
                  );
                }
                setOpen(false);
              }}
            >
              Copy private key (backup)
            </MenuItem>
          )}
          <div style={{ height: 1, background: "var(--border-1)", margin: "4px 0" }} />
          <MenuItem
            onClick={() => {
              disconnect();
              setOpen(false);
            }}
            danger
          >
            Disconnect
          </MenuItem>
        </div>
      )}
    </div>
  );
}

/** Labeled value row in the dropdown header (Portfolio / Cash / Account). */
function InfoRow({
  label,
  value,
  strong,
}: {
  label: string;
  value: string;
  strong?: boolean;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "baseline",
        justifyContent: "space-between",
        gap: 16,
        padding: "2px 0",
      }}
    >
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 10,
          letterSpacing: "var(--track-wide)",
          textTransform: "uppercase",
          color: "var(--fg-4)",
        }}
      >
        {label}
      </span>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 12,
          color: strong ? "var(--fg-1)" : "var(--fg-2)",
          fontWeight: strong ? 600 : 400,
        }}
      >
        {value}
      </span>
    </div>
  );
}

/**
 * Read-only warning shown in place of the "copy key" affordance for passkey
 * (WebAuthn) accounts. A passkey has no exportable private key — the credential
 * lives in this browser + authenticator only — so there is nothing to copy as a
 * backup. Saying so plainly avoids false backup confidence (the old "copy key
 * handle" copied the non-restorable credential id).
 */
function PasskeyNotice() {
  return (
    <div
      role="note"
      style={{
        padding: "8px 10px",
        display: "flex",
        flexDirection: "column",
        gap: 3,
      }}
    >
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 10,
          letterSpacing: "var(--track-wide)",
          textTransform: "uppercase",
          color: "var(--warn)",
        }}
      >
        Passkey account
      </span>
      <span
        style={{
          fontFamily: "var(--font-sans)",
          fontSize: 12,
          lineHeight: 1.4,
          color: "var(--fg-3)",
        }}
      >
        Lives in this browser + authenticator. There is no exportable key to
        back up — clearing this browser or losing the authenticator loses
        access.
      </span>
    </div>
  );
}

function MenuItem({
  children,
  onClick,
  danger,
}: {
  children: React.ReactNode;
  onClick: () => void;
  danger?: boolean;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      style={{
        display: "block",
        width: "100%",
        textAlign: "left",
        padding: "8px 10px",
        background: "transparent",
        border: 0,
        borderRadius: 4,
        color: danger ? "var(--no)" : "var(--fg-2)",
        fontFamily: "var(--font-sans)",
        fontSize: 13,
        cursor: "pointer",
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = "var(--surface-2)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = "transparent";
      }}
    >
      {children}
    </button>
  );
}

function ChipShell({ label, disabled }: { label: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      disabled={disabled}
      style={chipButtonStyle(false)}
      aria-hidden={disabled}
    >
      {label}
    </button>
  );
}

function chipButtonStyle(connected: boolean): React.CSSProperties {
  return {
    height: 32,
    padding: "0 var(--space-3)",
    background: connected ? "var(--accent-soft, var(--surface-2))" : "var(--surface-2)",
    border: connected
      ? "1px solid color-mix(in srgb, var(--accent) 40%, transparent)"
      : "1px solid var(--border-2)",
    borderRadius: "var(--radius-md)",
    color: connected ? "var(--fg-1)" : "var(--fg-2)",
    fontFamily: "var(--font-mono)",
    fontSize: "var(--fs-12)",
    letterSpacing: "var(--track-wide)",
    textTransform: "uppercase",
    cursor: "pointer",
    display: "inline-flex",
    alignItems: "center",
  };
}
