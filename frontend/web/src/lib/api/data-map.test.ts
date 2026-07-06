/**
 * Staleness guard for `frontend/DATA_MAP.md` (SYB-156).
 *
 * Two cheap, path-string-only checks keep the data map honest as the API
 * surface moves:
 *
 *   A. Every `/v1/...` endpoint the map NAMES must still exist as a path key in
 *      the generated OpenAPI client types (`schema.d.ts`). Catches renamed /
 *      deleted endpoints and typos.
 *
 *   B. Every `/v1/...` endpoint the frontend API client ACTUALLY calls
 *      (`api.GET("/v1/…")`, `api.POST(…)`, …) must appear somewhere in the map.
 *      Catches new endpoints wired into the UI that nobody documented.
 *
 * Deliberate limits (keep this low-false-positive, not a semantic verifier):
 *   - Pure path-string matching. It does NOT check HTTP method, query params,
 *     response shape, field names, or whether a row's prose is still accurate.
 *   - Path placeholders are normalised to `{}` on both sides, so `{id}` vs
 *     `{event_id}` vs `{height}` never trip it.
 *   - Doc-only glob rows (e.g. `/v1/bridge/*`, `/v1/proofs/state/*`) are skipped
 *     in check A — they intentionally stand in for a family of endpoints.
 *   - The WebSocket (`/v1/blocks/ws`) is reached through `ws/client.ts`, not the
 *     `api` client, so check B does not see it; it is a normal schema key so
 *     check A still validates it when the map names it.
 *   - Test/smoke harness files are excluded from check B — they hit liveness /
 *     fixture endpoints (`/v1/health`, …) that are not frontend-visible data.
 */

import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const HERE = dirname(fileURLToPath(import.meta.url)); // …/src/lib/api
const SRC_ROOT = join(HERE, "..", ".."); // …/src
const DATA_MAP = join(HERE, "..", "..", "..", "..", "DATA_MAP.md"); // frontend/DATA_MAP.md
const SCHEMA = join(HERE, "schema.d.ts");

/** Collapse `{anything}` → `{}` and drop any trailing slash. */
function normalize(path: string): string {
  return path.replace(/\{[^}]+\}/g, "{}").replace(/\/+$/, "");
}

/** Path keys declared in the generated OpenAPI `paths` interface. */
function schemaPaths(): Set<string> {
  const text = readFileSync(SCHEMA, "utf8");
  const out = new Set<string>();
  // Path keys look like:  `    "/v1/markets/{id}": {`
  const re = /^\s*"(\/v1\/[^"]*)":\s*\{/gm;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    if (m[1]) out.add(normalize(m[1]));
  }
  return out;
}

/** Every `/v1/...` token the map mentions, minus doc-only glob rows. */
function dataMapPaths(): Set<string> {
  const text = readFileSync(DATA_MAP, "utf8");
  const out = new Set<string>();
  const re = /\/v1\/[\w{}/*-]+/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    const raw = m[0];
    if (raw.includes("*")) continue; // glob stand-in, not a real key
    out.add(normalize(raw));
  }
  return out;
}

/** Recursively list every .ts/.tsx file under `src`. */
function sourceFiles(dir: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) files.push(...sourceFiles(full));
    else if (/\.tsx?$/.test(entry.name)) files.push(full);
  }
  return files;
}

/** Paths the frontend `api` client actually calls (excluding test/smoke). */
function frontendUsedPaths(): Set<string> {
  const out = new Set<string>();
  const re = /\bapi\.(?:GET|POST|PUT|DELETE|PATCH)\(\s*["'`](\/v1\/[^"'`]+)["'`]/g;
  for (const file of sourceFiles(SRC_ROOT)) {
    const posix = file.replace(/\\/g, "/");
    // Skip vitest files and the smoke *page* — dev liveness/fixture harnesses
    // that hit endpoints (`/v1/health`, …) which are not frontend-visible data.
    if (/\.test\.tsx?$/.test(posix) || posix.includes("/app/smoke/")) continue;
    const text = readFileSync(file, "utf8");
    let m: RegExpExecArray | null;
    while ((m = re.exec(text)) !== null) {
      if (m[1]) out.add(normalize(m[1]));
    }
  }
  return out;
}

describe("DATA_MAP.md staleness guard", () => {
  const schema = schemaPaths();
  const mapped = dataMapPaths();
  const used = frontendUsedPaths();

  it("names only endpoints that still exist in schema.d.ts (check A)", () => {
    const orphans = [...mapped].filter((p) => !schema.has(p)).sort();
    expect(
      orphans,
      `DATA_MAP.md references /v1 paths absent from schema.d.ts (renamed/deleted?):\n  ${orphans.join(
        "\n  ",
      )}`,
    ).toEqual([]);
  });

  it("documents every endpoint the frontend API client calls (check B)", () => {
    const undocumented = [...used].filter((p) => !mapped.has(p)).sort();
    expect(
      undocumented,
      `Frontend calls these /v1 paths but DATA_MAP.md never mentions them:\n  ${undocumented.join(
        "\n  ",
      )}`,
    ).toEqual([]);
  });

  it("sees a sane, non-empty surface (sanity)", () => {
    expect(schema.size).toBeGreaterThan(20);
    expect(mapped.size).toBeGreaterThan(15);
    expect(used.size).toBeGreaterThan(10);
  });
});
