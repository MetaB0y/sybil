import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { BridgeWithdrawal } from "@/lib/account/withdrawals";
import { WithdrawalStatusPanel } from "./withdrawal-etas";

const NOW = Date.UTC(2026, 6, 12, 12, 0, 0);

function withdrawal(patch: Partial<BridgeWithdrawal> = {}): BridgeWithdrawal {
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

describe("WithdrawalStatusPanel", () => {
  it("keeps the unavailable creation path explicit when there are no rows", () => {
    const html = renderToStaticMarkup(
      <WithdrawalStatusPanel rows={[]} nowMs={NOW} />,
    );

    expect(html).toContain("Normal withdrawals");
    expect(html).toContain("No active withdrawal leaves");
    expect(html).toContain("New withdrawal requests are not enabled");
    expect(html).toContain("owner-signed API can create");
    expect(html).toContain("relay, delayed L1 finalization");
    expect(html).toContain(
      "accept-all mock relay is not real-funds proof security",
    );
    expect(html).not.toContain("signed API is service-gated");
    expect(html).not.toContain(">Withdraw<");
  });

  it("shows active status without overstating L1 release", () => {
    const html = renderToStaticMarkup(
      <WithdrawalStatusPanel
        rows={[
          withdrawal({
            l1_status: "queued",
            l1_executable_at_unix: NOW / 1000 + 60,
          }),
        ]}
        nowMs={NOW}
      />,
    );

    expect(html).toContain("1 active");
    expect(html).toContain("#1 · $1.00");
    expect(html).toContain("cancel window");
    expect(html).toContain(
      "accept-all mock relay is not real-funds proof security",
    );
  });
});
