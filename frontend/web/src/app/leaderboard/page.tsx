"use client";

/**
 * Leaderboard page (SYB-59) — ranks traders by windowed PnL, with 7d/30d/all
 * window tabs. Anonymous by default ("Trader #<id>"); the connected user's own
 * row is highlighted. Display-name opt-in awaits profiles (SYB-60).
 */

import { useState } from "react";
import { PageHeader } from "@/components/page-header";
import { LeaderboardTable } from "@/components/leaderboard/leaderboard-table";
import { WindowTabs } from "@/components/leaderboard/window-tabs";
import { useAccountSession } from "@/lib/account/use-account";
import {
  useLeaderboard,
  type LeaderboardWindow,
} from "@/lib/leaderboard/use-leaderboard";

export default function LeaderboardPage() {
  const [window, setWindow] = useState<LeaderboardWindow>("7D");
  const session = useAccountSession();
  const { rows, isLoading } = useLeaderboard(window);

  return (
    <main
      style={{
        minHeight: "100vh",
        background: "var(--bg-1)",
        color: "var(--fg-1)",
        fontFamily: "var(--font-sans)",
        display: "flex",
        flexDirection: "column",
      }}
    >
      <div
        style={{
          padding: "calc(var(--space-6) + 36px) var(--space-5) 0",
        }}
      >
        <PageHeader
          title="Leaderboard"
          meta="top traders on Sybil, ranked by net PnL"
          action={<WindowTabs value={window} onChange={setWindow} />}
        />
      </div>

      <LeaderboardTable
        rows={rows}
        isLoading={isLoading}
        myAccountId={session?.accountId ?? null}
      />
    </main>
  );
}
