"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useState } from "react";
import { DropdownMenu } from "radix-ui";
import { ChevronDown } from "lucide-react";

export const DEV_SECTIONS = [
  { href: "/dev/overview", label: "Overview" },
  { href: "/dev/markets", label: "Markets" },
  { href: "/dev/blocks", label: "Blocks" },
  { href: "/dev/aggregates", label: "Aggregates" },
  { href: "/dev/accounts", label: "MM & Accounts" },
  { href: "/dev/bots", label: "Bot Decisions" },
] as const;

export function DevZoneNav() {
  const pathname = usePathname();
  const inDevZone = pathname.startsWith("/dev");
  const [hovered, setHovered] = useState<string | null>(null);

  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button
          type="button"
          className="nav-devzone"
          data-active={inDevZone}
        >
          Dev Zone
          <ChevronDown size={14} aria-hidden />
        </button>
      </DropdownMenu.Trigger>

      <DropdownMenu.Portal>
        <DropdownMenu.Content
          sideOffset={8}
          align="start"
          style={{
            background: "var(--surface-3)",
            border: "1px solid var(--border-2)",
            borderRadius: "var(--radius-md)",
            padding: "var(--space-2)",
            minWidth: 200,
            zIndex: 60,
            boxShadow: "0 8px 24px rgba(0,0,0,0.32)",
          }}
        >
          {DEV_SECTIONS.map((section) => {
            const active = pathname === section.href;
            const highlighted = hovered === section.href;
            return (
              <DropdownMenu.Item key={section.href} asChild>
                <Link
                  href={section.href}
                  onMouseEnter={() => setHovered(section.href)}
                  onMouseLeave={() => setHovered(null)}
                  style={{
                    display: "block",
                    padding: "6px 10px",
                    borderRadius: "var(--radius-md)",
                    fontSize: "var(--fs-13)",
                    fontFamily: "var(--font-sans)",
                    textDecoration: "none",
                    outline: "none",
                    color: active ? "var(--fg-1)" : "var(--fg-3)",
                    background:
                      active || highlighted ? "var(--surface-2)" : "transparent",
                    transition: "color var(--dur-fast) var(--ease-standard)",
                  }}
                >
                  {section.label}
                </Link>
              </DropdownMenu.Item>
            );
          })}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}
