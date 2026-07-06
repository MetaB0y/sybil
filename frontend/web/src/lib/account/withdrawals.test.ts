import { describe, expect, it } from "vitest";
import {
  formatWithdrawalCountdown,
  pendingWithdrawals,
  withdrawalCancelState,
  type BridgeWithdrawal,
} from "./withdrawals";

const NOW = Date.UTC(2026, 6, 6, 12, 0, 0);

function withdrawal(
  patch: Partial<BridgeWithdrawal> = {},
): BridgeWithdrawal {
  return {
    account_id: 7,
    amount_nanos: "1000000000",
    amount_token_units: 1_000_000,
    created_at_height: 10,
    expiry_height: 100,
    nullifier_hex: "00",
    recipient_hex: "11",
    token_hex: "22",
    withdrawal_id: 1,
    withdrawal_leaf_digest_hex: "33",
    withdrawal_leaf_hex: "44",
    ...patch,
  };
}

describe("withdrawal countdown helpers", () => {
  it("formats multi-day, hourly, minute, and second countdowns", () => {
    expect(formatWithdrawalCountdown(NOW, NOW / 1000 + 90_000).label).toBe(
      "1d 1h",
    );
    expect(formatWithdrawalCountdown(NOW, NOW / 1000 + 7_260).label).toBe(
      "2h 1m",
    );
    expect(formatWithdrawalCountdown(NOW, NOW / 1000 + 125).label).toBe(
      "2m 5s",
    );
    expect(formatWithdrawalCountdown(NOW, NOW / 1000 + 7).label).toBe("7s");
  });

  it("rolls over to executable now at or past the timestamp", () => {
    expect(formatWithdrawalCountdown(NOW, NOW / 1000)).toEqual({
      label: "executable now",
      expired: true,
    });
    expect(formatWithdrawalCountdown(NOW, NOW / 1000 - 10).expired).toBe(true);
  });

  it("derives cancel-window states from L1 status and executable time", () => {
    expect(
      withdrawalCancelState(
        withdrawal({ l1_status: "queued", l1_executable_at_unix: NOW / 1000 + 60 }),
        NOW,
      ),
    ).toBe("cancel-window-open");
    expect(
      withdrawalCancelState(
        withdrawal({ l1_status: "queued", l1_executable_at_unix: NOW / 1000 - 1 }),
        NOW,
      ),
    ).toBe("executable");
    expect(withdrawalCancelState(withdrawal({ l1_status: "finalized" }), NOW)).toBe(
      "finalized",
    );
    expect(withdrawalCancelState(withdrawal({ l1_status: "cancelled" }), NOW)).toBe(
      "cancelled",
    );
  });

  it("keeps only pending withdrawals", () => {
    const rows = pendingWithdrawals(
      [
        withdrawal({ withdrawal_id: 1, l1_status: "queued" }),
        withdrawal({ withdrawal_id: 2, l1_status: "finalized" }),
        withdrawal({ withdrawal_id: 3, l1_status: "cancelled" }),
      ],
      NOW,
    );
    expect(rows.map((row) => row.withdrawal_id)).toEqual([1]);
  });
});
