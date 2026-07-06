export const NAV_SHEET_BREAKPOINT_PX = 1100;

export type NavCollapseState = "desktop" | "sheet";

export function navCollapseStateForWidth(widthPx: number): NavCollapseState {
  return widthPx <= NAV_SHEET_BREAKPOINT_PX ? "sheet" : "desktop";
}

export function shouldCloseNavSheetOnPathChange(open: boolean): boolean {
  return open;
}
