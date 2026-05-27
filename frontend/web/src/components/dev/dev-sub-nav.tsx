"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { DEV_SECTIONS } from "./dev-zone-nav";

export function DevSubNav() {
  const pathname = usePathname();

  return (
    <div
      style={{
        display: "flex",
        gap: 8,
        overflowX: "auto",
        flexWrap: "nowrap",
      }}
    >
      {DEV_SECTIONS.map((section) => {
        const active = pathname === section.href;
        return (
          <Link
            key={section.href}
            href={section.href}
            style={{
              padding: "6px 10px",
              borderRadius: "var(--radius-md)",
              fontSize: 13,
              fontFamily: "var(--font-sans)",
              fontWeight: 500,
              textDecoration: "none",
              whiteSpace: "nowrap",
              color: active ? "var(--fg-1)" : "var(--fg-3)",
              background: active ? "var(--surface-2)" : "transparent",
            }}
          >
            {section.label}
          </Link>
        );
      })}
    </div>
  );
}
