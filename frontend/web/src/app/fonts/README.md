# Vendored fonts

These `.woff2` files are checked in so `next build` is **hermetic** — it never
contacts `fonts.googleapis.com` / `fonts.gstatic.com` at build time. This
permanently fixes the recurring "font fetch failed during build" breakage.

They are loaded via `next/font/local` in `src/app/layout.tsx` under private
`--font-*-vendor` variables. The public design tokens (`--font-display`,
`--font-sans`, `--font-mono`) reference those variables first, followed by
system fallbacks. Keeping the two layers distinct prevents the token sheet from
overwriting the font-family value emitted by Next.js.

Each file is the **latin-subset variable** font (all weights in one file),
matching the previous `subsets: ["latin"]` config exactly.

| File | Family | Weight range | Source URL (Google Fonts, `v` may bump) |
| --- | --- | --- | --- |
| `Syne-Variable.woff2` | Syne | 400–800 | https://fonts.gstatic.com/s/syne/v24/8vIH7w4qzmVxm2BL9A.woff2 |
| `Inter-Variable.woff2` | Inter | 100–900 | https://fonts.gstatic.com/s/inter/v20/UcC73FwrK3iLTeHuS_nVMrMxCp50SjIa1ZL7.woff2 |
| `JetBrainsMono-Variable.woff2` | JetBrains Mono | 100–800 | https://fonts.gstatic.com/s/jetbrainsmono/v24/tDbV2o-flEEny0FZhsfKu5WU4xD7OwE.woff2 |

## Re-vendoring (only if a font needs updating)

The URLs above are the `latin` `@font-face` `src` from the Google Fonts CSS API.
To refresh, request the CSS with a modern browser User-Agent and copy the
`/* latin */` block's URL:

```bash
UA="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120 Safari/537.36"
curl -s -H "User-Agent: $UA" \
  "https://fonts.googleapis.com/css2?family=Syne:wght@400..800&display=swap"
# then: curl -s "<latin src url>" -o Syne-Variable.woff2
```
