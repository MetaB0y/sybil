#!/usr/bin/env node
// Sync handoff design tokens into the web app, stripping the Google Fonts @import
// (next/font owns font loading; the @import would cause a duplicate request + FOUT).
//
// Source: ../handoff/tokens/colors_and_type.css  (design source of truth — DO NOT EDIT)
// Dest:   src/styles/sybil-tokens.css            (generated; safe to overwrite)

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const src = resolve(root, "../handoff/tokens/colors_and_type.css");
const dest = resolve(root, "src/styles/sybil-tokens.css");

const raw = readFileSync(src, "utf8");

// Strip lines that @import from fonts.googleapis.com (Step 6 owns fonts).
const cleaned = raw
  .split("\n")
  .filter((line) => !/@import\s+url\(['"]https:\/\/fonts\.googleapis\.com/.test(line))
  .join("\n");

const banner = `/*
 * GENERATED — do not edit directly.
 * Synced from frontend/handoff/tokens/colors_and_type.css via \`pnpm tokens:sync\`.
 * The Google Fonts @import line is stripped; fonts are loaded by next/font in layout.tsx.
 */\n\n`;

mkdirSync(dirname(dest), { recursive: true });
writeFileSync(dest, banner + cleaned);

console.log(`✓ Synced tokens → ${dest.replace(root + "/", "")}`);
