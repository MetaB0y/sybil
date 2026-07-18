# `arena`

Python bots, simulations, generated client bindings, and the live Arena
service. Arena consumes `sybil-api`; it is never an exchange-validity input.

## Read first

- [[Bot Framework]], [[Python SDK]], and [[LLM Trader]]
- [[REST API]] and [[WebSocket Block Stream]] for client changes

## Boundaries

- Do not import sequencer, solver, or storage internals. Exercise the same
  HTTP/WebSocket contracts as external agents.
- `sybil_client/_generated/` is machine-owned; regenerate it with
  `just arena-sdk-regen`. Hand-written behavior belongs in `client.py` and
  `types.py`.
- JSON nanos are decimal strings and become Python integers at the client
  boundary. Floats are display/strategy values, not wire or signing values.
- Replayed block events repair observations only; never invoke strategies or
  submit historical orders during replay.
- `sim/` owns offline/time-compressed experiments. `live/` owns the supervised
  service, durable decision data, health, and metrics; neither may become
  canonical exchange state.
- External news, LLM output, and market configuration are untrusted strategy
  inputs. Preserve provenance and fail explicitly on malformed outputs.

Run `just arena-check`; API surface changes also require
`just arena-sdk-regen` and review of the generated diff.
