import type { Metadata } from "next";
import localFont from "next/font/local";
import { ConnectModal } from "@/components/auth/connect-modal";
import { DevnetNotice } from "@/components/devnet-notice";
import { GlobalNav } from "@/components/global-nav";
import { DEVNET_DISMISSED_KEY } from "@/lib/devnet";
import { Providers } from "./providers";
import "./globals.css";

// Fonts are vendored (next/font/local) rather than fetched from
// fonts.googleapis.com at build time. This makes `next build` hermetic and
// network-free — the recurring "fonts fetch failed during build" breakage
// can no longer happen. The .woff2 files are the latin-subset variable fonts
// (same subset as the previous `subsets: ["latin"]`). Private vendor variables
// feed the public design tokens without colliding with them. See
// src/app/fonts/README.md.
const display = localFont({
  variable: "--font-display-vendor",
  display: "swap",
  src: [
    { path: "./fonts/Syne-Variable.woff2", weight: "400 800", style: "normal" },
  ],
});

const sans = localFont({
  variable: "--font-sans-vendor",
  display: "swap",
  src: [
    {
      path: "./fonts/Inter-Variable.woff2",
      weight: "100 900",
      style: "normal",
    },
  ],
});

// Exposed as `--font-mono-vendor` (not `--font-mono`) so it doesn't collide
// with the design token in sybil-tokens.css. The token references this var
// first, so the bundled variable font (every weight 100–800 real) is actually
// used instead of a system fallback whose weight faces are hit-or-miss.
const mono = localFont({
  variable: "--font-mono-vendor",
  display: "swap",
  src: [
    {
      path: "./fonts/JetBrainsMono-Variable.woff2",
      weight: "100 800",
      style: "normal",
    },
  ],
});

export const metadata: Metadata = {
  title: "Sybil",
  description: "Prediction market on frequent batch auctions.",
};

// Runs before first paint: applies the persisted light theme so there's no
// flash of dark before React hydrates (dark is the default, no attribute), and
// the same for a dismissed devnet notice — which also reserves layout space, so
// restoring it after hydration would shove the whole page down a line.
const CHROME_INIT = `
(function () {
  try {
    var t = localStorage.getItem('sybil-theme');
    if (t === 'light') document.documentElement.setAttribute('data-theme', 'light');
    if (localStorage.getItem('${DEVNET_DISMISSED_KEY}') === '1') {
      document.documentElement.setAttribute('data-devnet', 'dismissed');
    }
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
        <script dangerouslySetInnerHTML={{ __html: CHROME_INIT }} />
      </head>
      <body className="min-h-full flex flex-col">
        <Providers>
          <DevnetNotice />
          <GlobalNav />
          {/* Both strips are fixed, so the page reserves their combined height
              (`--chrome-height`) rather than the nav bar's alone. */}
          <div style={{ paddingTop: "var(--chrome-height)" }}>{children}</div>
          <ConnectModal />
        </Providers>
      </body>
    </html>
  );
}
