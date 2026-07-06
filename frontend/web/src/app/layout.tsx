import type { Metadata } from "next";
import localFont from "next/font/local";
import { ConnectModal } from "@/components/auth/connect-modal";
import { GlobalNav } from "@/components/global-nav";
import { Providers } from "./providers";
import "./globals.css";

// Fonts are vendored (next/font/local) rather than fetched from
// fonts.googleapis.com at build time. This makes `next build` hermetic and
// network-free — the recurring "fonts fetch failed during build" breakage
// can no longer happen. The .woff2 files are the latin-subset variable fonts
// (same subset as the previous `subsets: ["latin"]`). CSS variable names are
// unchanged so nothing downstream needs to change. See src/app/fonts/README.md.
const display = localFont({
  variable: "--font-display",
  display: "swap",
  src: [{ path: "./fonts/Syne-Variable.woff2", weight: "400 800", style: "normal" }],
});

const sans = localFont({
  variable: "--font-sans",
  display: "swap",
  src: [{ path: "./fonts/Inter-Variable.woff2", weight: "100 900", style: "normal" }],
});

const mono = localFont({
  variable: "--font-mono",
  display: "swap",
  src: [{ path: "./fonts/JetBrainsMono-Variable.woff2", weight: "100 800", style: "normal" }],
});

export const metadata: Metadata = {
  title: "Sybil",
  description: "Prediction market on frequent batch auctions.",
};

// Runs before first paint: applies the persisted light theme so there's no
// flash of dark before React hydrates. Dark is the default (no attribute).
const THEME_INIT = `
(function () {
  try {
    var t = localStorage.getItem('sybil-theme');
    if (t === 'light') document.documentElement.setAttribute('data-theme', 'light');
  } catch (e) {}
})();
`;

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html
      lang="en"
      className={`${display.variable} ${sans.variable} ${mono.variable} h-full antialiased`}
      suppressHydrationWarning
    >
      <head>
        <script dangerouslySetInnerHTML={{ __html: THEME_INIT }} />
      </head>
      <body className="min-h-full flex flex-col">
        <Providers>
          <GlobalNav />
          <div style={{ paddingTop: "var(--nav-height)" }}>{children}</div>
          <ConnectModal />
        </Providers>
      </body>
    </html>
  );
}
