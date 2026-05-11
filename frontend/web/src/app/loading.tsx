export default function Loading() {
  return (
    <div
      style={{
        minHeight: "60vh",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        color: "var(--fg-3)",
        fontFamily: "var(--font-mono)",
        fontSize: "var(--fs-12)",
        textTransform: "uppercase",
        letterSpacing: "var(--track-wide)",
      }}
    >
      {"// loading"}
    </div>
  );
}
