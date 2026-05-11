"use client";

import Link from "next/link";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { Suspense, useCallback, type ChangeEvent } from "react";
import { BatchPill } from "./batch-pill";

type NavTab = { href: string; label: string; match: (path: string) => boolean };

const TABS: readonly NavTab[] = [
  { href: "/", label: "Markets", match: (p) => p === "/" || p.startsWith("/m/") },
  { href: "/activity", label: "Activity", match: (p) => p.startsWith("/activity") },
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
        background: "rgba(10,14,18,0.72)",
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
        <Suspense fallback={<SearchSkeleton />}>
          <NavSearch />
        </Suspense>
        <BatchPill />
        <button
          type="button"
          style={{
            height: 32,
            padding: "0 var(--space-3)",
            background: "var(--surface-2)",
            border: "1px solid var(--border-2)",
            borderRadius: "var(--radius-md)",
            color: "var(--fg-2)",
            fontFamily: "var(--font-mono)",
            fontSize: "var(--fs-12)",
            letterSpacing: "var(--track-wide)",
            textTransform: "uppercase",
            cursor: "not-allowed",
          }}
          disabled
          title="Wallet — coming soon"
        >
          connect
        </button>
      </div>
    </header>
  );
}

/**
 * Search input live-bound to `?q=` on the markets index. If the user is on
 * another route (e.g. /m/123), typing routes them to / with the query
 * applied so the filtered grid is what they see next.
 */
function NavSearch() {
  const searchParams = useSearchParams();
  const router = useRouter();
  const pathname = usePathname();
  const value = searchParams.get("q") ?? "";

  const onChange = useCallback(
    (e: ChangeEvent<HTMLInputElement>) => {
      const next = e.target.value;
      const params = new URLSearchParams(searchParams.toString());
      if (next) params.set("q", next);
      else params.delete("q");
      const qs = params.toString();
      const target = pathname === "/" ? "/" : "/";
      if (pathname === "/") {
        router.replace(qs ? `${target}?${qs}` : target, { scroll: false });
      } else {
        router.push(qs ? `${target}?${qs}` : target, { scroll: false });
      }
    },
    [pathname, router, searchParams]
  );

  return (
    <div style={searchShellStyle}>
      <span
        aria-hidden
        className="text-mono"
        style={searchSlashStyle}
      >
        /
      </span>
      <input
        value={value}
        onChange={onChange}
        placeholder="search events, markets"
        aria-label="search markets"
        style={searchInputStyle}
      />
    </div>
  );
}

function SearchSkeleton() {
  return <div style={searchShellStyle} aria-hidden />;
}

const searchShellStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  height: 32,
  width: 280,
  padding: "0 var(--space-3)",
  background: "var(--surface-1)",
  border: "1px solid var(--border-2)",
  borderRadius: "var(--radius-md)",
};

const searchSlashStyle: React.CSSProperties = {
  color: "var(--fg-4)",
  fontSize: "var(--fs-12)",
  letterSpacing: "var(--track-wide)",
  marginRight: "var(--space-2)",
};

const searchInputStyle: React.CSSProperties = {
  flex: 1,
  background: "transparent",
  border: 0,
  outline: "none",
  color: "var(--fg-1)",
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-12)",
  padding: 0,
};
