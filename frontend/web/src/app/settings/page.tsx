"use client";

/**
 * /settings — account management (SYB-60): profile, signing/agent keys, and
 * read-only API keys. Thin wrapper mirroring /portfolio's hydrated /
 * disconnected / connected states; real logic lives in <SettingsView />.
 */

import { SettingsView } from "@/components/settings/settings-view";
import { DisconnectedAccountPrompt } from "@/components/auth/disconnected-account-prompt";
import {
  useAccountHydrated,
  useAccountSession,
  useSetConnectModalOpen,
} from "@/lib/account/use-account";

export default function SettingsPage() {
  const session = useAccountSession();
  const hydrated = useAccountHydrated();

  if (!hydrated) {
    return (
      <Shell>
        <Hint>loading…</Hint>
      </Shell>
    );
  }
  if (!session) {
    return (
      <Shell>
        <Disconnected />
      </Shell>
    );
  }
  return (
    <Shell>
      <SettingsView
        accountId={session.accountId}
        publicKeyHex={session.publicKeyHex}
        authScheme={session.authScheme}
        {...(session.credentialIdB64url
          ? { credentialIdB64url: session.credentialIdB64url }
          : {})}
      />
    </Shell>
  );
}

function Disconnected() {
  const openModal = useSetConnectModalOpen();
  return (
    <DisconnectedAccountPrompt
      title="Connect to manage your account"
      message={
        <>
          Profile, signing keys, and API keys live behind your account. Connect
          a demo account in your browser to manage them.
        </>
      }
      onConnect={() => openModal(true)}
    />
  );
}

function Shell({ children }: { children: React.ReactNode }) {
  return (
    <main
      className="sybil-page-pad"
      style={{
        width: "100%",
        paddingTop: "calc(var(--space-6) + 36px)",
        paddingBottom: "var(--space-9)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-4)",
      }}
    >
      {children}
    </main>
  );
}

function Hint({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        color: "var(--fg-4)",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
      }}
    >
      {children}
    </div>
  );
}
