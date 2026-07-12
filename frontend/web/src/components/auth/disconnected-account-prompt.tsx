"use client";

export function DisconnectedAccountPrompt({
  title,
  message,
  onConnect,
}: {
  title: string;
  message: React.ReactNode;
  onConnect: () => void;
}) {
  return (
    <section
      aria-labelledby="disconnected-account-title"
      style={{
        padding: "48px 24px",
        background: "var(--surface-1)",
        border: "1px dashed var(--border-1)",
        borderRadius: 10,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: 16,
        textAlign: "center",
      }}
    >
      <h1
        id="disconnected-account-title"
        style={{
          margin: 0,
          fontFamily: "var(--font-display)",
          fontSize: 18,
          fontWeight: 600,
          color: "var(--fg-1)",
        }}
      >
        {title}
      </h1>
      <p
        style={{
          margin: 0,
          color: "var(--fg-3)",
          fontFamily: "var(--font-sans)",
          fontSize: 13,
          maxWidth: 400,
          lineHeight: 1.5,
        }}
      >
        {message}
      </p>
      <button
        type="button"
        onClick={onConnect}
        style={{
          minHeight: 44,
          padding: "10px 18px",
          background: "var(--accent)",
          border: 0,
          borderRadius: 8,
          color: "var(--bg-1)",
          fontFamily: "var(--font-sans)",
          fontWeight: 600,
          fontSize: 14,
          cursor: "pointer",
        }}
      >
        Connect
      </button>
    </section>
  );
}
