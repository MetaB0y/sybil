const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

type TabKeyEvent = Pick<KeyboardEvent, "key" | "shiftKey" | "preventDefault">;

export function getFocusWrapTarget(
  items: readonly HTMLElement[],
  activeElement: Element | null,
  backwards: boolean,
): HTMLElement | null {
  if (items.length === 0) return null;

  const first = items[0]!;
  const last = items[items.length - 1]!;
  const activeIsInside = items.includes(activeElement as HTMLElement);

  if (backwards && (activeElement === first || !activeIsInside)) return last;
  if (!backwards && (activeElement === last || !activeIsInside)) return first;
  return null;
}

/** Keep keyboard focus inside an open modal while preserving normal Tab order. */
export function trapTabFocus(
  event: TabKeyEvent,
  container: HTMLElement,
  activeElement: Element | null = document.activeElement,
): boolean {
  if (event.key !== "Tab") return false;

  const items = Array.from(
    container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
  ).filter((element) => element.getAttribute("aria-hidden") !== "true");

  const target =
    getFocusWrapTarget(items, activeElement, event.shiftKey) ??
    (items.length === 0 ? container : null);
  if (!target) return false;

  event.preventDefault();
  // Wrapping can cross an internally scrollable modal (for example from a
  // sticky Close button to the final form action). Let the browser reveal the
  // new target in its nearest scroll container instead of leaving keyboard
  // focus on an off-screen control.
  target.focus();
  return true;
}
