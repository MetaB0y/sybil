# Frontend known issues

Verified against `main` on 2026-07-11. Keep this list limited to active
workarounds; issue tracking and dated implementation plans belong elsewhere.

## Large integer fields cross JSON as numbers

**Status:** frontend workaround active; a wire-format fix is still needed.

Sybil represents money in nanodollars. JavaScript cannot exactly represent
integers above `2^53 - 1`, so sufficiently large `u64` values can lose precision
during `JSON.parse` before application code can convert them to `bigint`.

Current containment:

1. `web/scripts/patch-bigints.mjs` changes generated TypeScript declarations for
   `*_nanos` fields to strings.
2. `web/src/lib/format/nanos.ts` accepts string, number, or bigint and keeps all
   subsequent money arithmetic in `bigint`.
3. UI code must use the shared formatting/parsing helpers rather than direct
   JavaScript-number arithmetic.

The declaration patch prevents accidental frontend arithmetic but cannot repair
digits already rounded on the JSON wire. The durable fix is for the API to
serialize protocol-sized integer fields as decimal strings and advertise that
shape in OpenAPI. When that lands, regenerate the schema, verify the wire with
values above `2^53`, and remove the patch only after it becomes redundant.

The former Polymarket display-metadata item was removed: images, end dates, and
categories now flow through `MarketRefData` and are consumed by the web app.
