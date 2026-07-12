import { describe, expect, it, vi } from "vitest";
import { getFocusWrapTarget, trapTabFocus } from "./focus-trap";

function element(): HTMLElement {
  return {} as HTMLElement;
}

describe("getFocusWrapTarget", () => {
  const first = element();
  const middle = element();
  const last = element();
  const items = [first, middle, last];

  it("wraps forward from the last item to the first", () => {
    expect(getFocusWrapTarget(items, last, false)).toBe(first);
  });

  it("wraps backward from the first item to the last", () => {
    expect(getFocusWrapTarget(items, first, true)).toBe(last);
  });

  it("leaves focus alone inside the normal tab order", () => {
    expect(getFocusWrapTarget(items, middle, false)).toBeNull();
    expect(getFocusWrapTarget(items, middle, true)).toBeNull();
  });

  it("recovers focus that is outside the modal", () => {
    expect(getFocusWrapTarget(items, element(), false)).toBe(first);
    expect(getFocusWrapTarget(items, element(), true)).toBe(last);
  });
});

describe("trapTabFocus", () => {
  it("moves focus from the last control back to the first", () => {
    const focusFirst = vi.fn();
    const first = {
      focus: focusFirst,
      getAttribute: () => null,
    } as unknown as HTMLElement;
    const last = {
      focus: vi.fn(),
      getAttribute: () => null,
    } as unknown as HTMLElement;
    const preventDefault = vi.fn();
    const container = {
      querySelectorAll: () => [first, last],
    } as unknown as HTMLElement;

    expect(
      trapTabFocus(
        { key: "Tab", shiftKey: false, preventDefault },
        container,
        last,
      ),
    ).toBe(true);
    expect(preventDefault).toHaveBeenCalledOnce();
    expect(focusFirst).toHaveBeenCalledWith();
  });

  it("focuses the modal itself when it has no focusable controls", () => {
    const focus = vi.fn();
    const preventDefault = vi.fn();
    const container = {
      focus,
      querySelectorAll: () => [],
    } as unknown as HTMLElement;

    expect(
      trapTabFocus(
        { key: "Tab", shiftKey: false, preventDefault },
        container,
        null,
      ),
    ).toBe(true);
    expect(preventDefault).toHaveBeenCalledOnce();
    expect(focus).toHaveBeenCalledWith();
  });

  it("ignores keys other than Tab", () => {
    const preventDefault = vi.fn();
    const container = {} as HTMLElement;

    expect(
      trapTabFocus(
        { key: "Escape", shiftKey: false, preventDefault },
        container,
        null,
      ),
    ).toBe(false);
    expect(preventDefault).not.toHaveBeenCalled();
  });
});
