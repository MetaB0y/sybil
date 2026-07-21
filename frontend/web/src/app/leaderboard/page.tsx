"use client";

/**
 * Leaderboard page (SYB-59) — ranks traders by windowed PnL, with 7d/30d/all
 * window tabs. Anonymous by default ("Trader #<id>"); the connected user's own
 * row is highlighted. Display-name opt-in awaits profiles (SYB-60).
 */

import { useMemo, useState } from "react";
import { PageHeader } from "@/components/page-header";
import { LeaderboardTable } from "@/components/leaderboard/leaderboard-table";
import { WindowTabs } from "@/components/leaderboard/window-tabs";
import { useAccountSession } from "@/lib/account/use-account";
import { useBotLeaderboard } from "@/lib/leaderboard/use-bot-leaderboard";
import {
  DEFAULT_LEADERBOARD_WINDOW,
  mergeAndRank,
  useLeaderboard,
  type LeaderboardWindow,
} from "@/lib/leaderboard/use-leaderboard";

export default function LeaderboardPage() {
  const [window, setWindow] = useState<LeaderboardWindow>(
    DEFAULT_LEADERBOARD_WINDOW,
  );
  const session = useAccountSession();
  const { rows, isLoading, isRetrying, readState, errorMessage, retry } =
    useLeaderboard(window);
  // Arena bots never publish a profile, so they cannot reach /v1/leaderboard.
  // They are fetched separately and ranked alongside the opt-in human rows.
  const bots = useBotLeaderboard(window);
  const allRows = useMemo(
    () => mergeAndRank(rows, bots.rows),
    [rows, bots.rows],
  );

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
        className="sybil-page-pad"
        style={{
          paddingTop: "calc(var(--space-6) + var(--ticker-offset))",
        }}
      >
        <PageHeader
          title="Leaderboard"
          meta="arena bots and opted-in traders, ranked by net PnL"
          action={<WindowTabs value={window} onChange={setWindow} />}
        />
      </div>

      <LeaderboardTable
        rows={allRows}
        isLoading={isLoading}
        isRetrying={isRetrying}
        readState={readState}
        errorMessage={errorMessage}
        onRetry={retry}
        myAccountId={session?.accountId ?? null}
      />
    </main>
  );
}
