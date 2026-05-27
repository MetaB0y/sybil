export type Tone = "yes" | "no" | "accent" | "warn" | "cyan" | "muted" | "dim" | "fg";

export function toneColor(tone: Tone): string {
  switch (tone) {
    case "yes": return "var(--yes)";
    case "no": return "var(--no)";
    case "accent": return "var(--accent)";
    case "warn": return "var(--warn)";
    case "cyan": return "var(--accent-hover)";
    case "muted": return "var(--fg-3)";
    case "dim": return "var(--fg-4)";
    case "fg": return "var(--fg-1)";
  }
}
