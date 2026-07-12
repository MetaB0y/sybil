"use client";

import Link from "next/link";
import { useEffect } from "react";

export default function GlobalError({
  error,
  unstable_retry,
}: {
  error: Error & { digest?: string };
  unstable_retry: () => void;
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
      <p
        style={{
          color: "var(--fg-3)",
          margin: 0,
          maxWidth: 460,
          textAlign: "center",
          lineHeight: 1.5,
        }}
      >
        We couldn&apos;t load this screen. Retry the request, or return to the
        market list while the service recovers.
      </p>
      {error.digest && (
        <span
          style={{
            color: "var(--fg-4)",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
          }}
        >
          reference {error.digest}
        </span>
      )}
      <div
        style={{
          marginTop: "var(--space-4)",
          display: "flex",
          alignItems: "center",
          gap: "var(--space-3)",
        }}
      >
        <button
          type="button"
          onClick={() => unstable_retry()}
          style={{
            minHeight: 40,
            padding: "0 var(--space-4)",
            background: "var(--accent)",
            color: "var(--fg-on-accent)",
            border: 0,
            borderRadius: "var(--radius-md)",
            fontFamily: "var(--font-sans)",
            fontWeight: 600,
            cursor: "pointer",
          }}
        >
          Try again
        </button>
        <Link
          href="/"
          style={{
            minHeight: 40,
            display: "inline-flex",
            alignItems: "center",
            padding: "0 var(--space-4)",
            border: "1px solid var(--border-2)",
            borderRadius: "var(--radius-md)",
            color: "var(--fg-2)",
            fontFamily: "var(--font-sans)",
            fontSize: 13,
            fontWeight: 600,
            textDecoration: "none",
          }}
        >
          Back to markets
        </Link>
      </div>
    </div>
  );
}
