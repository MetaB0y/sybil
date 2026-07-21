"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { Menu, X } from "lucide-react";
import { Suspense, useCallback, useEffect, useRef, useState } from "react";
import { AccountChip } from "./auth/account-chip";
import { BatchPill } from "./batch-pill";
import { DevZoneNav } from "./dev/dev-zone-nav";
import { NavSearch, NavSearchSkeleton } from "./nav-search";
import { ThemeToggle } from "./theme-toggle";
import { shouldCloseNavSheetOnPathChange } from "@/lib/responsive/nav";

type NavTab = { href: string; label: string; match: (path: string) => boolean };

const TABS: readonly NavTab[] = [
  {
    href: "/",
    label: "Markets",
    match: (p) => p === "/" || p.startsWith("/m/"),
  },
  {
    href: "/activity",
    label: "Activity",
    match: (p) => p.startsWith("/activity"),
  },
  { href: "/arena", label: "Arena", match: (p) => p.startsWith("/arena") },
  {
    href: "/leaderboard",
    label: "Leaderboard",
    match: (p) => p.startsWith("/leaderboard"),
  },
  {
    href: "/portfolio",
    label: "Portfolio",
    match: (p) => p.startsWith("/portfolio"),
  },
];

export function GlobalNav() {
  const pathname = usePathname();
  const [menuOpen, setMenuOpen] = useState(false);
  const [lastPathname, setLastPathname] = useState(pathname);
  const menuButtonRef = useRef<HTMLButtonElement>(null);
  const sheetRef = useRef<HTMLDivElement>(null);
  const restoreMenuButtonFocusRef = useRef(true);

  const closeMenu = useCallback((restoreFocus = true) => {
    restoreMenuButtonFocusRef.current = restoreFocus;
    setMenuOpen(false);
  }, []);

  const openMenu = useCallback(() => {
    restoreMenuButtonFocusRef.current = true;
    setMenuOpen(true);
  }, []);

  if (pathname !== lastPathname) {
    setLastPathname(pathname);
    if (shouldCloseNavSheetOnPathChange(menuOpen)) setMenuOpen(false);
  }

  useEffect(() => {
    if (!menuOpen) return;

    const sheet = sheetRef.current;
    const menuButton = menuButtonRef.current;
    const previousBodyOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";

    const focusable = getFocusableElements(sheet);
    (focusable[0] ?? sheet)?.focus({ preventScroll: true });

    function onKeyDown(event: KeyboardEvent) {
      if (event.defaultPrevented) return;
      if (event.key === "Escape") {
        event.preventDefault();
        closeMenu();
        return;
      }
      if (event.key !== "Tab" || !sheet) return;

      const items = getFocusableElements(sheet);
      if (items.length === 0) {
        event.preventDefault();
        sheet.focus({ preventScroll: true });
        return;
      }

      const first = items[0]!;
      const last = items[items.length - 1]!;
      const active = document.activeElement;
      if (event.shiftKey && (active === first || !sheet.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (
        !event.shiftKey &&
        (active === last || !sheet.contains(active))
      ) {
        event.preventDefault();
        first.focus();
      }
    }

    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      document.body.style.overflow = previousBodyOverflow;
      if (restoreMenuButtonFocusRef.current && menuButton?.isConnected) {
        menuButton.focus({ preventScroll: true });
      }
    };
  }, [menuOpen, closeMenu]);

  return (
    <header className="global-nav">
      {/* Wordmark */}
      <Link
        className="global-nav-brand"
        href="/"
        style={{
          display: "inline-flex",
          alignItems: "baseline",
          flexShrink: 0,
          textDecoration: "none",
          color: "var(--fg-1)",
        }}
      >
        {/* Size lives in globals.css (`.global-nav-wordmark`), not here: the
            phone header scales it up, and an inline font-size would outrank
            the media query. */}
        <span
          className="global-nav-wordmark"
          style={{
            fontFamily: "var(--font-display)",
            fontWeight: 700,
            letterSpacing: "var(--track-tight)",
            textTransform: "uppercase",
          }}
        >
          Sybil
        </span>
      </Link>

      {/* Route tabs */}
      <nav className="global-nav-tabs" aria-label="Primary">
        {TABS.map((tab) => (
          <NavTabLink key={tab.href} tab={tab} pathname={pathname} />
        ))}
        <DevZoneNav />
      </nav>

      <div className="global-nav-search-desktop">
        <Suspense fallback={<NavSearchSkeleton />}>
          <NavSearch />
        </Suspense>
      </div>

      {/* Right side — batch status + account controls. */}
      <div className="global-nav-right">
        <ThemeToggle />
        <BatchPill />
        <AccountChip />
        <button
          ref={menuButtonRef}
          type="button"
          className="global-nav-menu-button"
          aria-label={
            menuOpen ? "Close navigation menu" : "Open navigation menu"
          }
          aria-expanded={menuOpen}
          aria-controls="global-nav-sheet"
          onClick={() => (menuOpen ? closeMenu() : openMenu())}
        >
          {menuOpen ? (
            <X size={18} aria-hidden />
          ) : (
            <Menu size={18} aria-hidden />
          )}
        </button>
      </div>
      {menuOpen && (
        <>
          <div
            className="global-nav-sheet-backdrop"
            aria-hidden
            onClick={() => closeMenu()}
          />
          <div
            ref={sheetRef}
            id="global-nav-sheet"
            className="global-nav-sheet"
            role="dialog"
            aria-modal="true"
            aria-label="Navigation menu"
            tabIndex={-1}
          >
            <div className="global-nav-sheet-search">
              <Suspense fallback={<NavSearchSkeleton />}>
                <NavSearch />
              </Suspense>
            </div>
            <nav className="global-nav-sheet-tabs" aria-label="Primary mobile">
              {TABS.map((tab) => (
                <NavTabLink
                  key={tab.href}
                  tab={tab}
                  pathname={pathname}
                  onNavigate={() => closeMenu(false)}
                />
              ))}
              <DevZoneNav />
            </nav>
          </div>
        </>
      )}
    </header>
  );
}

function NavTabLink({
  tab,
  pathname,
  onNavigate,
}: {
  tab: NavTab;
  pathname: string;
  onNavigate?: () => void;
}) {
  const active = tab.match(pathname);
  return (
    <Link
      className="global-nav-tab"
      data-active={active}
      href={tab.href}
      {...(onNavigate ? { onClick: onNavigate } : {})}
    >
      {tab.label}
    </Link>
  );
}

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

function getFocusableElements(container: HTMLElement | null): HTMLElement[] {
  if (!container) return [];
  return Array.from(
    container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
  ).filter((element) => element.getAttribute("aria-hidden") !== "true");
}
