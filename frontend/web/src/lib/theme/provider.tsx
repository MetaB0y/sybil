"use client";

import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";

type Theme = "dark" | "light";
type ThemeCtx = { theme: Theme; setTheme: (t: Theme) => void; toggle: () => void };

const Ctx = createContext<ThemeCtx | null>(null);

export function ThemeProvider({ children }: { children: ReactNode }) {
  // Mirror the attribute the no-flash script already set; default dark.
  const [theme, setThemeState] = useState<Theme>("dark");

  /* eslint-disable react-hooks/set-state-in-effect -- mirror the attribute the no-flash script set, post-mount, to avoid SSR mismatch */
  useEffect(() => {
    const attr = document.documentElement.getAttribute("data-theme");
    setThemeState(attr === "light" ? "light" : "dark");
  }, []);
  /* eslint-enable react-hooks/set-state-in-effect */

  const setTheme = useCallback((t: Theme) => {
    setThemeState(t);
    const el = document.documentElement;
    if (t === "light") el.setAttribute("data-theme", "light");
    else el.removeAttribute("data-theme");
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
