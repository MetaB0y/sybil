# `sybil-loadtest`

Black-box HTTP load tests for architectural isolation and capacity discovery.
These binaries target an already-running stack and never mutate exchange
state.

## Read first

- [[Testing Strategy]]
- [[Historical Data Serving]]
- [[Deployment Profiles]]

## Boundaries

- Load generation is a development/operations tool, never a dependency of a
  production service.
- Prefer public API contracts so a run exercises routing, authorization, API
  scheduling, service boundaries, and storage together.
- Keep a separately named control request in every isolation test. Throughput
  alone cannot show whether the sequencer stayed responsive.
- Historical account reads should use an owner read bearer token in
  production posture. A service token skips the owner-auth read model and
  weakens the regression signal.
- Tests are read-only. Do not create accounts, place orders, or advance blocks
  unless the runbook explicitly defines an isolated disposable environment.
- Run the generator off the target host for capacity measurements; same-host
  runs measure load-generator contention too.

Run `cargo test -p sybil-loadtest` and `cargo clippy -p sybil-loadtest
--all-targets` after changes.
