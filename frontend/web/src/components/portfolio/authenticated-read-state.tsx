"use client";

export type AuthenticatedReadSnapshot = {
  isPending: boolean;
  error: unknown;
};

export type AuthenticatedReadStatus = "loading" | "error" | "ready";

/**
 * Collapse several private-account reads into one display state. Errors win so
 * one failed read cannot be masked indefinitely by another request still being
 * pending. A successful empty array is `ready`, not `loading` or `error`.
 */
export function authenticatedReadStatus(
  reads: readonly AuthenticatedReadSnapshot[],
): AuthenticatedReadStatus {
  if (reads.some((read) => read.error != null)) return "error";
  if (reads.some((read) => read.isPending)) return "loading";
  return "ready";
}

export function AuthenticatedReadState({
  status,
  title,
  message,
  onRetry,
  retrying = false,
}: {
  status: Exclude<AuthenticatedReadStatus, "ready">;
  title: string;
  message: string;
  onRetry?: () => void;
  retrying?: boolean;
}) {
  const failed = status === "error";
  return (
    <section
      role={failed ? "alert" : "status"}
      aria-live={failed ? "assertive" : "polite"}
      aria-busy={!failed || retrying}
      style={{
        padding: "24px",
        background: "var(--surface-1)",
        border: `1px solid ${failed ? "var(--no)" : "var(--border-1)"}`,
        borderRadius: 8,
        display: "flex",
        flexDirection: "column",
        alignItems: "flex-start",
        gap: 10,
      }}
    >
      <div
        style={{
          color: "var(--fg-1)",
          fontFamily: "var(--font-display)",
          fontSize: 17,
          fontWeight: 600,
        }}
      >
        {title}
      </div>
      <p
        style={{
          margin: 0,
          color: "var(--fg-3)",
          fontFamily: "var(--font-sans)",
          fontSize: 13,
          lineHeight: 1.5,
        }}
      >
        {message}
      </p>
      {failed && onRetry && (
        <button
          type="button"
          onClick={onRetry}
          disabled={retrying}
          style={{
            padding: "8px 14px",
            background: "var(--accent)",
            border: 0,
            borderRadius: 6,
            color: "var(--bg-1)",
            fontFamily: "var(--font-sans)",
            fontSize: 13,
            fontWeight: 600,
            cursor: retrying ? "wait" : "pointer",
            opacity: retrying ? 0.7 : 1,
          }}
        >
          {retrying ? "Retrying…" : "Retry"}
        </button>
      )}
    </section>
  );
}
