"use client";

/**
 * Dev-mode demo funding. Calls `POST /v1/accounts/{id}/fund`. Returns 403
 * when `dev_mode=false` on the server — caller surfaces that as
 * "bridge deposits coming soon" once we have a real onramp.
 */

import { api } from "@/lib/api/client";

export async function addDemoFunds(
  accountId: number,
  amountNanos: bigint,
): Promise<void> {
  const res = await api.POST("/v1/accounts/{id}/fund", {
    params: { path: { id: accountId } },
    body: { amount_nanos: Number(amountNanos) as unknown as string },
  });
  if (res.error) {
    const status = res.response?.status;
    if (status === 403) {
      throw new Error("Demo funding is disabled on this server");
    }
    throw new Error(`fund_account failed (HTTP ${status ?? "?"})`);
  }
}
