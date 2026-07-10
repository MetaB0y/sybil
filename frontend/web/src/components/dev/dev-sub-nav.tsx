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
            className="dev-sub-nav-tab"
            data-active={active}
          >
            {section.label}
          </Link>
        );
      })}
    </div>
  );
}
