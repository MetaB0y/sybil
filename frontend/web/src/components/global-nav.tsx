"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { Menu, X } from "lucide-react";
import { Suspense, useState } from "react";
import { AccountChip } from "./auth/account-chip";
import { BatchPill } from "./batch-pill";
import { DevZoneNav } from "./dev/dev-zone-nav";
import { NavSearch, NavSearchSkeleton } from "./nav-search";
import { ThemeToggle } from "./theme-toggle";
import { shouldCloseNavSheetOnPathChange } from "@/lib/responsive/nav";

type NavTab = { href: string; label: string; match: (path: string) => boolean };

const TABS: readonly NavTab[] = [
  { href: "/", label: "Markets", match: (p) => p === "/" || p.startsWith("/m/") },
  { href: "/activity", label: "Activity", match: (p) => p.startsWith("/activity") },
  { href: "/arena", label: "Arena", match: (p) => p.startsWith("/arena") },
  {
    href: "/leaderboard",
    label: "Leaderboard",
    match: (p) => p.startsWith("/leaderboard"),
  },
  { href: "/portfolio", label: "Portfolio", match: (p) => p.startsWith("/portfolio") },
  { href: "/settings", label: "Settings", match: (p) => p.startsWith("/settings") },
];

export function GlobalNav() {
  const pathname = usePathname();
  const [menuOpen, setMenuOpen] = useState(false);
  const [lastPathname, setLastPathname] = useState(pathname);

  if (pathname !== lastPathname) {
    setLastPathname(pathname);
    if (shouldCloseNavSheetOnPathChange(menuOpen)) setMenuOpen(false);
  }

  return (
    <header className="global-nav">
      {/* Wordmark */}
      <Link
        href="/"
        style={{
          display: "inline-flex",
          alignItems: "baseline",
          flexShrink: 0,
          textDecoration: "none",
          color: "var(--fg-1)",
        }}
      >
        <span
          style={{
            fontFamily: "var(--font-display)",
            fontWeight: 700,
            fontSize: "var(--fs-18)",
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

      {/* Global search — grows to fill the middle of the bar */}
      <div className="global-nav-search-desktop">
        <Suspense fallback={<NavSearchSkeleton />}>
          <NavSearch />
        </Suspense>
      </div>

      {/* Right side — theme + batch pill + (placeholder) account chip */}
      <div className="global-nav-right">
        <ThemeToggle />
        <BatchPill />
        <AccountChip />
        <button
          type="button"
          className="global-nav-menu-button"
          aria-label={menuOpen ? "Close navigation menu" : "Open navigation menu"}
          aria-expanded={menuOpen}
          aria-controls="global-nav-sheet"
          onClick={() => setMenuOpen((open) => !open)}
        >
          {menuOpen ? <X size={18} aria-hidden /> : <Menu size={18} aria-hidden />}
        </button>
      </div>
      {menuOpen && (
        <>
          <div
            className="global-nav-sheet-backdrop"
            aria-hidden
            onClick={() => setMenuOpen(false)}
          />
          <div id="global-nav-sheet" className="global-nav-sheet">
            <div className="global-nav-sheet-search">
              <Suspense fallback={<NavSearchSkeleton />}>
                <NavSearch />
              </Suspense>
            </div>
            <nav className="global-nav-sheet-tabs" aria-label="Primary mobile">
              {TABS.map((tab) => (
                <NavTabLink key={tab.href} tab={tab} pathname={pathname} />
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
}: {
  tab: NavTab;
  pathname: string;
}) {
  const active = tab.match(pathname);
  return (
    <Link className="global-nav-tab" data-active={active} href={tab.href}>
      {tab.label}
    </Link>
  );
}
