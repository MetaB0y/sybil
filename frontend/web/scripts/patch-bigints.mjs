#!/usr/bin/env node
// Frontend-only workaround for the backend emitting u64 (`*_nanos`) fields
// as JSON numbers. JS `number` corrupts above 2^53; we treat these as bigint
// at the TS boundary. Backend fix tracked — see frontend/KNOWN_ISSUES.md.
//
// This script rewrites the `_nanos` value type from `number` to `string`
// (and `number[]` to `string[]` inside nested maps) in the generated schema.
// Runs after `openapi-typescript`.

import { readFileSync, writeFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const target = process.argv[2]
  ? resolve(process.cwd(), process.argv[2])
  : resolve(here, "../src/lib/api/schema.d.ts");

const lines = readFileSync(target, "utf8").split("\n");
const out = [];
let braceDepth = 0;
let inside = false;
let patched = 0;

const swapNumberForString = (line) => {
  const next = line.replace(/\bnumber\b/g, "string");
  if (next !== line) patched++;
  return next;
};

for (const line of lines) {
  if (inside) {
    out.push(swapNumberForString(line));
    for (const ch of line) {
      if (ch === "{") braceDepth++;
      else if (ch === "}") braceDepth--;
    }
    if (braceDepth <= 0) inside = false;
  } else {
    const m = line.match(/_nanos\??:\s*(.*)$/);
    if (m) {
      const rest = m[1];
      const opens = (rest.match(/\{/g) || []).length;
      const closes = (rest.match(/\}/g) || []).length;
      out.push(swapNumberForString(line));
      braceDepth = opens - closes;
      inside = braceDepth > 0;
    } else {
      out.push(line);
    }
  }
}

writeFileSync(target, out.join("\n"));
console.log(`✓ patch-bigints: rewrote ${patched} line(s) in schema.d.ts`);
