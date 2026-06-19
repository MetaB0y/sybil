"use client";

import { createContext, useContext } from "react";
import { useRouter } from "next/navigation";

/**
 * Switch the active outcome WITHOUT a route navigation.
 *
 * On /m/[id], a `router.push` to a sibling outcome remounts the entire dynamic
 * segment — chart, rail, and header all tear down and rebuild (~hundreds of DOM
 * mutations), which reads as a whole-screen blink. The market detail page
 * instead provides a setter that flips in-page state and rewrites the URL via
 * `history.replaceState`, so React reconciles the existing tree in place.
 *
 * Components fall back to a normal navigation when no provider is mounted, so
 * the legend/picker still work anywhere else.
 */
const SelectOutcomeContext = createContext<((marketId: number) => void) | null>(
  null,
);

export const SelectOutcomeProvider = SelectOutcomeContext.Provider;

export function useSelectOutcome(): (marketId: number) => void {
  const ctx = useContext(SelectOutcomeContext);
  const router = useRouter();
  return ctx ?? ((marketId) => router.push(`/m/${marketId}`));
}
