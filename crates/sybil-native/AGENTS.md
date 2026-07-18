# `sybil-native`

Native Sybil market catalog ownership. The package validates checked-in
templates, applies them idempotently through the public service API, writes a
genesis-bound deployment manifest, and runs static-anchor flash liquidity from
that manifest.

## Read first

- [[REST API]]
- [[Order Admission]] and [[MM Budget Constraint]]
- [[Market Resolution]]

## Boundaries

- Catalog application is a one-shot admin command, never a mirror side effect.
- The MM process consumes a completed, genesis-bound deployment manifest.
- The MM actor publishes read-only progress; the owning native process turns
  it into `/healthz`, `/readyz`, and Prometheus metrics. The API does not infer
  native MM health.
- Native static anchors do not consume Polymarket data or learn recursively
  from Sybil clearing prices.
- Native resolution remains an explicit operator workflow. No LLM resolver is
  bundled into provisioning or market making.
