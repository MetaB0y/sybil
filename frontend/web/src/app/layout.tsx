import type { Metadata } from "next";
import { Syne, Inter, JetBrains_Mono } from "next/font/google";
import { ConnectModal } from "@/components/auth/connect-modal";
import { GlobalNav } from "@/components/global-nav";
import { Providers } from "./providers";
import "./globals.css";

const display = Syne({
  variable: "--font-display",
  subsets: ["latin"],
  display: "swap",
});

const sans = Inter({
  variable: "--font-sans",
  subsets: ["latin"],
  display: "swap",
});

const mono = JetBrains_Mono({
  variable: "--font-mono",
  subsets: ["latin"],
  display: "swap",
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
