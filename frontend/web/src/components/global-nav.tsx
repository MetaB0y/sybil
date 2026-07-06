"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { Suspense } from "react";
import { AccountChip } from "./auth/account-chip";
import { BatchPill } from "./batch-pill";
import { DevZoneNav } from "./dev/dev-zone-nav";
import { NavSearch, NavSearchSkeleton } from "./nav-search";
import { ThemeToggle } from "./theme-toggle";

type NavTab = { href: string; label: string; match: (path: string) => boolean };

const TABS: readonly NavTab[] = [
  { href: "/", label: "Markets", match: (p) => p === "/" || p.startsWith("/m/") },
  { href: "/activity", label: "Activity", match: (p) => p.startsWith("/activity") },
  { href: "/arena", label: "Arena", match: (p) => p.startsWith("/arena") },
  { href: "/portfolio", label: "Portfolio", match: (p) => p.startsWith("/portfolio") },
];

export function GlobalNav() {
  const pathname = usePathname();

  return (
    <header
      style={{
        position: "fixed",
        top: 0,
        left: 0,
        right: 0,
        height: "var(--nav-height)",
        zIndex: 50,
        background: "var(--nav-bg)",
        backdropFilter: "var(--blur-nav)",
        WebkitBackdropFilter: "var(--blur-nav)",
        borderBottom: "1px solid var(--border-1)",
        display: "flex",
        alignItems: "center",
        gap: "var(--space-5)",
        padding: "0 var(--space-5)",
      }}
    >
      {/* Wordmark + status pill */}
      <Link
        href="/"
        style={{
          display: "inline-flex",
          alignItems: "baseline",
          gap: "var(--space-2)",
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
        <span
          style={{
            display: "inline-flex",
            alignItems: "center",
            height: 18,
            padding: "0 var(--space-2)",
            background: "var(--warn-soft)",
            color: "var(--warn)",
            border: "1px solid color-mix(in srgb, var(--warn) 24%, transparent)",
            borderRadius: "var(--radius-pill)",
            fontFamily: "var(--font-mono)",
            fontSize: "10px",
            letterSpacing: "var(--track-wide)",
            textTransform: "uppercase",
          }}
        >
          testnet
        </span>
      </Link>

      {/* Route tabs */}
      <nav style={{ display: "flex", alignItems: "center", gap: "var(--space-2)" }}>
        {TABS.map((tab) => {
          const active = tab.match(pathname);
          return (
            <Link
              key={tab.href}
              href={tab.href}
              style={{
                position: "relative",
                padding: "0 var(--space-3)",
                height: 32,
                display: "inline-flex",
                alignItems: "center",
                color: active ? "var(--fg-1)" : "var(--fg-3)",
                fontFamily: "var(--font-sans)",
                fontSize: "var(--fs-13)",
                fontWeight: 500,
                textDecoration: "none",
                borderRadius: "var(--radius-md)",
                background: active ? "var(--surface-2)" : "transparent",
                transition: "color var(--dur-fast) var(--ease-standard)",
              }}
            >
              {tab.label}
            </Link>
          );
        })}
        <DevZoneNav />
      </nav>

      {/* Right side — search + batch pill + (placeholder) account chip */}
      <div
        style={{
          marginLeft: "auto",
          display: "flex",
          alignItems: "center",
          gap: "var(--space-3)",
        }}
      >
        <Suspense fallback={<NavSearchSkeleton />}>
          <NavSearch />
        </Suspense>
        <ThemeToggle />
        <BatchPill />
        <AccountChip />
      </div>
    </header>
  );
}
