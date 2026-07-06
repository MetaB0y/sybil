import { describe, expect, it } from "vitest";
import {
  NAV_SHEET_BREAKPOINT_PX,
  navCollapseStateForWidth,
  shouldCloseNavSheetOnPathChange,
} from "./nav";

describe("nav responsive helpers", () => {
  it("uses desktop navigation above the sheet breakpoint", () => {
    expect(navCollapseStateForWidth(NAV_SHEET_BREAKPOINT_PX + 1)).toBe(
      "desktop",
    );
  });

  it("uses the sheet at and below the breakpoint", () => {
    expect(navCollapseStateForWidth(NAV_SHEET_BREAKPOINT_PX)).toBe("sheet");
    expect(navCollapseStateForWidth(375)).toBe("sheet");
  });

  it("closes an open sheet when the route changes", () => {
    expect(shouldCloseNavSheetOnPathChange(true)).toBe(true);
    expect(shouldCloseNavSheetOnPathChange(false)).toBe(false);
  });
});
