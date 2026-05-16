import type { ReactNode } from "react";
import { DevSubNav } from "@/components/dev/dev-sub-nav";
import { DevStatusLine } from "@/components/dev/dev-status-line";

export default function DevLayout({ children }: { children: ReactNode }) {
  return (
    <main
      style={{
        minHeight: "100vh",
        background: "var(--bg-1)",
        color: "var(--fg-1)",
        fontFamily: "var(--font-sans)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 16,
          padding: "12px 24px 0",
          flexWrap: "wrap",
        }}
      >
        <DevSubNav />
        <DevStatusLine />
      </div>
      <div style={{ padding: "16px 24px 28px" }}>{children}</div>
    </main>
  );
}
