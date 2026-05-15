"use client";

import { useEffect } from "react";

export default function GlobalError({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  useEffect(() => {
    console.error(error);
  }, [error]);

  return (
    <div
      style={{
        minHeight: "100vh",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: "var(--space-3)",
        padding: "var(--space-6)",
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
        {"// error"}
      </div>
      <h1
        style={{
          fontFamily: "var(--font-display)",
          fontSize: "var(--fs-32)",
          margin: 0,
        }}
      >
        Something went wrong
      </h1>
      <p style={{ color: "var(--fg-3)", margin: 0 }}>
        {error.message || "Unexpected error."}
      </p>
      <button
        onClick={() => reset()}
        style={{
          marginTop: "var(--space-4)",
          padding: "var(--space-2) var(--space-4)",
          background: "var(--accent)",
          color: "var(--fg-on-accent)",
          border: 0,
          borderRadius: "var(--radius-md)",
          fontFamily: "var(--font-sans)",
          cursor: "pointer",
        }}
      >
        Try again
      </button>
    </div>
  );
}
