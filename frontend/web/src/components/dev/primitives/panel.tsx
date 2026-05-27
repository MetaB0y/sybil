import type { CSSProperties, ReactNode } from "react";

export function Panel({
  children,
  style,
}: {
  children?: ReactNode;
  style?: CSSProperties;
}) {
  return (
    <div
      style={{
        background: "var(--surface-1)",
        border: "1px solid var(--border-2)",
        borderRadius: 8,
        overflow: "hidden",
        ...style,
      }}
    >
      {children}
    </div>
  );
}

export function PanelHead({
  title,
  actions,
}: {
  title: string;
  actions?: ReactNode;
}) {
  return (
    <div
      style={{
        padding: "10px 12px",
        borderBottom: "1px solid var(--border-2)",
        display: "flex",
        justifyContent: "space-between",
        alignItems: "center",
        gap: 12,
      }}
    >
      <span
        style={{
          color: "var(--fg-3)",
          fontSize: 12,
          fontWeight: 650,
          letterSpacing: 0.4,
          textTransform: "uppercase",
        }}
      >
        {title}
      </span>
      {actions ? <div>{actions}</div> : null}
    </div>
  );
}

export function PanelBody({
  children,
  style,
}: {
  children?: ReactNode;
  style?: CSSProperties;
}) {
  return <div style={{ padding: 12, ...style }}>{children}</div>;
}
