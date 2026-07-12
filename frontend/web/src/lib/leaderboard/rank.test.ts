import { describe, expect, it } from "vitest";
import { formatRoiBps, formatSignedDollars, signColor } from "./format";
import { toLeaderboardRows } from "./use-leaderboard";

const DOLLAR = 1_000_000_000n;

describe("formatSignedDollars", () => {
  it("signs positive and negative PnL, keeps zero clean", () => {
    expect(formatSignedDollars(12n * DOLLAR + 340_000_000n)).toBe("+$12.34");
    expect(formatSignedDollars(-5n * DOLLAR)).toBe("-$5.00");
    expect(formatSignedDollars(0n)).toBe("$0.00");
  });
});

describe("formatRoiBps", () => {
  it("converts basis points to signed percent", () => {
    expect(formatRoiBps(1230)).toBe("+12.3%");
    expect(formatRoiBps(-400)).toBe("-4.0%");
    expect(formatRoiBps(0)).toBe("0.0%");
  });
});

describe("signColor", () => {
  it("maps sign to a semantic token", () => {
    expect(signColor(5n)).toBe("var(--yes)");
    expect(signColor(-5)).toBe("var(--no)");
    expect(signColor(0n)).toBe("var(--fg-3)");
  });
});

describe("toLeaderboardRows", () => {
  it("returns [] for missing data", () => {
    expect(toLeaderboardRows(undefined)).toEqual([]);
  });

  it("maps wire entries to display rows with bigint money, preserving order", () => {
    const rows = toLeaderboardRows({
      window: "7d",
      entries: [
        {
          rank: 1,
          account_id: 7,
          display_name: "alice",
          pnl_nanos: (3n * DOLLAR).toString(),
          roi_bps: 500,
          markets_traded: 4,
          equity_nanos: (100n * DOLLAR).toString(),
        },
        {
          rank: 2,
          account_id: 3,
          display_name: "bob",
          pnl_nanos: (-1n * DOLLAR).toString(),
          roi_bps: -50,
          markets_traded: 2,
          equity_nanos: (50n * DOLLAR).toString(),
        },
      ],
    });

    expect(rows.map((r) => r.rank)).toEqual([1, 2]);
    expect(rows[0]).toMatchObject({
      accountId: 7,
      label: "alice",
      pnlNanos: 3n * DOLLAR,
      roiBps: 500,
      marketsTraded: 4,
      equityNanos: 100n * DOLLAR,
    });
    expect(rows[1]?.pnlNanos).toBe(-1n * DOLLAR);
  });
});
