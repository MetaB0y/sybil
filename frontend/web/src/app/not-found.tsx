import Link from "next/link";

export default function NotFound() {
  return (
    <div
      style={{
        minHeight: "100vh",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: "var(--space-3)",
        background: "var(--bg-1)",
        color: "var(--fg-1)",
        fontFamily: "var(--font-sans)",
      }}
    >
      <div
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: "var(--fs-12)",
          color: "var(--fg-3)",
          textTransform: "uppercase",
          letterSpacing: "var(--track-wide)",
        }}
      >
        {"// 404"}
      </div>
      <h1
        style={{
          fontFamily: "var(--font-display)",
          fontSize: "var(--fs-32)",
          margin: 0,
        }}
      >
        Page not found
      </h1>
      <Link href="/" style={{ color: "var(--accent)" }}>
        Back to markets →
      </Link>
    </div>
  );
}
