"use client";

import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";

type Theme = "dark" | "light";
type ThemeCtx = { theme: Theme; setTheme: (t: Theme) => void; toggle: () => void };

const Ctx = createContext<ThemeCtx | null>(null);

export function ThemeProvider({ children }: { children: ReactNode }) {
  // Mirror the attribute the no-flash script already set; default dark.
  const [theme, setThemeState] = useState<Theme>("dark");
  // Pending timer that strips the transient `.theme-transition` class after the
  // flip settles; tracked so rapid toggles don't cut a later crossfade short.
  const flipTimer = useRef<number | null>(null);

  /* eslint-disable react-hooks/set-state-in-effect -- mirror the attribute the no-flash script set, post-mount, to avoid SSR mismatch */
  useEffect(() => {
    const attr = document.documentElement.getAttribute("data-theme");
    setThemeState(attr === "light" ? "light" : "dark");
  }, []);
  /* eslint-enable react-hooks/set-state-in-effect */

  const setTheme = useCallback((t: Theme) => {
    setThemeState(t);
    const el = document.documentElement;

    // Crossfade the whole surface on the flip, unless the user prefers less
    // motion. The class only adds the `transition` — the color change comes
    // from `data-theme` — so we must commit that rule with the *current* colors
    // (a forced reflow) before flipping, or the browser paints the new theme
    // instantly with no animation.
    const animate =
      typeof window !== "undefined" &&
      !window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    if (animate) {
      if (flipTimer.current != null) window.clearTimeout(flipTimer.current);
      el.classList.add("theme-transition");
      void el.offsetWidth;
    }

    if (t === "light") el.setAttribute("data-theme", "light");
    else el.removeAttribute("data-theme");

    if (animate) {
      // Slightly longer than --dur-slow (320ms) so the crossfade finishes
      // before the transition rule is removed.
      flipTimer.current = window.setTimeout(() => {
        el.classList.remove("theme-transition");
        flipTimer.current = null;
      }, 360);
    }

    try {
      localStorage.setItem("sybil-theme", t);
    } catch {}
  }, []);

  const toggle = useCallback(
    () => setTheme(theme === "light" ? "dark" : "light"),
    [theme, setTheme],
  );

  return <Ctx.Provider value={{ theme, setTheme, toggle }}>{children}</Ctx.Provider>;
}

export function useTheme() {
  const v = useContext(Ctx);
  if (!v) throw new Error("useTheme must be used within ThemeProvider");
  return v;
}
