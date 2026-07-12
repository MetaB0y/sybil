/**
 * Turn a raw order-submit error into friendly, user-facing copy.
 *
 * `submitSignedOrder` throws messages like
 *   "submit_signed failed (HTTP 400): order 18293722 rejected:
 *    InsufficientBalance { required: 1009762112570, available: 1000000000000 }"
 * — the Rust `{:?}` debug of `RejectionReason` / a `SequencerError` Display
 * string (see `crates/matching-sequencer/src/error.rs`). We never want that in
 * front of a bettor, so map the known reasons here and fall back to a generic
 * line. Callers should still `console.error` the raw error for debugging.
 *
 * Pure + string-only so it's trivially testable.
 */

/** Noun used in the copy — "bet" in Degen mode, "order" in Pro mode. */
export type OrderNoun = "bet" | "order";

export function humanizeOrderError(
  err: unknown,
  noun: OrderNoun = "bet",
): string {
  const m = err instanceof Error ? err.message : String(err);

  if (/InsufficientBalance/i.test(m))
    return `Not enough balance for this ${noun}.`;
  if (/InsufficientPosition/i.test(m))
    return "You don't have enough shares to sell.";
  // HTTP 409 from the signed endpoints — the replay nonce was stale or already
  // used (see `submit_signed_order` in crates/sybil-api). Retrying mints a fresh
  // nonce, so a plain "try again" is the right nudge.
  if (/HTTP 409|replay|nonce|stale or duplicate/i.test(m))
    return `This ${noun} was already submitted — try again.`;
  if (/\bExpired\b/i.test(m))
    return `Your ${noun} didn't make it into a batch in time. Try again.`;
  if (/rate limited/i.test(m))
    return "Too many orders — wait a moment and try again.";
  if (/mempool full/i.test(m))
    return "The network is busy — try again in a moment.";
  if (/signature|signer|does not match/i.test(m))
    return `Couldn't verify your ${noun} — reconnect and try again.`;
  if (/market not found/i.test(m))
    return "This market isn't available right now.";
  if (/account not found/i.test(m)) return "Account not found — reconnect.";
  if (/CompleteSetFormation/i.test(m))
    return `That ${noun} can't be placed right now.`;

  return `Couldn't place your ${noun}. Try again.`;
}

/** Friendly copy for signed cancellation failures; the order may still be live. */
export function humanizeCancelError(
  err: unknown,
  noun: OrderNoun = "bet",
): string {
  const message = err instanceof Error ? err.message : String(err);
  const name = err instanceof Error ? err.name : "";
  const clientFailure = `${name} ${message}`;

  if (
    /passkey signing was cancelled|notallowederror|aborterror|not allowed|timed out|operation was aborted/i.test(
      clientFailure,
    )
  ) {
    return `Passkey approval was cancelled. Your ${noun} may still be active.`;
  }
  if (
    /pending order not found|already (?:filled|expired|cancelled)/i.test(
      message,
    )
  ) {
    return `This ${noun} already filled, expired, or was cancelled.`;
  }
  if (/HTTP 409|replay|nonce|stale or duplicate/i.test(message)) {
    return `The cancel request conflicted with a newer update. Check the ${noun} status and try again.`;
  }
  if (
    /invalid webauthn assertion|signature|signer|does not match/i.test(message)
  ) {
    return `Couldn't verify the cancellation. Reconnect and try again.`;
  }

  return `Couldn't cancel this ${noun}. It may still be active — try again.`;
}
