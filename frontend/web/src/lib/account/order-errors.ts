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

export function humanizeOrderError(err: unknown, noun: OrderNoun = "bet"): string {
  const m = err instanceof Error ? err.message : String(err);

  if (/InsufficientBalance/i.test(m)) return `Not enough balance for this ${noun}.`;
  if (/InsufficientPosition/i.test(m)) return "You don't have enough shares to sell.";
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
  if (/market not found/i.test(m)) return "This market isn't available right now.";
  if (/account not found/i.test(m)) return "Account not found — reconnect.";
  // NegRisk self-trade prevention: this order would give the account buy-side
  // coverage of every outcome in the group — a complete set. The rail normally
  // catches it before signing (see `lib/account/complete-set.ts`); this copy is
  // the fallback for a stale open-orders list, so it has to say what to do.
  if (/CompleteSetFormation/i.test(m))
    return `Your open orders in this event already cover the other outcomes — cancel one to place this ${noun}.`;

  return `Couldn't place your ${noun}. Try again.`;
}
