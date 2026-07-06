import type { components } from "@/lib/api/schema";

export type BridgeWithdrawal =
  components["schemas"]["BridgeWithdrawalResponse"];

export type WithdrawalCountdown = {
  label: string;
  expired: boolean;
};

export type WithdrawalCancelState =
  | "not-requested"
  | "cancel-window-open"
  | "executable"
  | "finalized"
  | "cancelled";

export function formatWithdrawalCountdown(
  nowMs: number,
  executableAtUnix: number | null | undefined,
): WithdrawalCountdown {
  if (executableAtUnix == null) return { label: "waiting for L1", expired: false };
  const remainingMs = executableAtUnix * 1000 - nowMs;
  if (remainingMs <= 0) return { label: "executable now", expired: true };

  const totalSeconds = Math.ceil(remainingMs / 1000);
  const days = Math.floor(totalSeconds / 86_400);
  const hours = Math.floor((totalSeconds % 86_400) / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  if (days > 0) return { label: `${days}d ${hours}h`, expired: false };
  if (hours > 0) return { label: `${hours}h ${minutes}m`, expired: false };
  if (minutes > 0) return { label: `${minutes}m ${seconds}s`, expired: false };
  return { label: `${seconds}s`, expired: false };
}

export function withdrawalCancelState(
  withdrawal: Pick<
    BridgeWithdrawal,
    "l1_status" | "l1_executable_at_unix" | "l1_cancelled_at_unix" | "l1_finalized_at_unix"
  >,
  nowMs: number,
): WithdrawalCancelState {
  if (withdrawal.l1_status === "cancelled" || withdrawal.l1_cancelled_at_unix != null) {
    return "cancelled";
  }
  if (withdrawal.l1_status === "finalized" || withdrawal.l1_finalized_at_unix != null) {
    return "finalized";
  }
  if (withdrawal.l1_status === "not_requested" || withdrawal.l1_executable_at_unix == null) {
    return "not-requested";
  }
  return withdrawal.l1_executable_at_unix * 1000 > nowMs
    ? "cancel-window-open"
    : "executable";
}

export function pendingWithdrawals(
  withdrawals: readonly BridgeWithdrawal[],
  nowMs: number,
): BridgeWithdrawal[] {
  return withdrawals.filter((w) => {
    const state = withdrawalCancelState(w, nowMs);
    return (
      state === "not-requested" ||
      state === "cancel-window-open" ||
      state === "executable"
    );
  });
}
