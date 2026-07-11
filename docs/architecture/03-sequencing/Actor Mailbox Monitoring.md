---
tags: [infrastructure, observability]
layer: sequencer
crate: matching-sequencer
status: current
last_verified: 2026-04-26
---

The API communicates with the exchange through the `SequencerActor` mailbox. Ractor 0.15 does not expose an exact public mailbox length, so Sybil tracks a conservative queue-depth gauge at the enqueue/dequeue boundary we control:

- `SequencerHandle` increments `sybil_actor_queue_depth{actor="sequencer"}` before each RPC message is sent.
- The block-tick loop increments before sending `Tick`.
- `SequencerActor::handle` decrements as soon as it starts processing a message.
- The supervisor resets the gauge when replacing a failed child actor.

This measures backlog waiting behind the actor, excluding the message currently executing. It preserves actor semantics: no bounding, dropping, priority, timeout, or reorder behavior is introduced by the monitor.

Two thresholds are configurable on `sybil-api`:

- `SYBIL_ACTOR_QUEUE_WARN_DEPTH` / `--actor-queue-warn-depth`
- `SYBIL_ACTOR_QUEUE_ERROR_DEPTH` / `--actor-queue-error-depth`

At the warning threshold the actor logs WARN; at the error threshold it logs ERROR and the deploy vmalert rules page on sustained depth. The Grafana dashboard includes an "Actor Queue Depth" panel.

The same metric name is intentionally generic. Future bounded-channel actors such as the Polymarket feed/MM tasks or a dedicated WebSocket fanout actor should use the same label shape, e.g. `sybil_actor_queue_depth{actor="polymarket_mm"}`, once those processes expose a metrics endpoint.

## Where This Lives
> `crates/matching-sequencer/src/actor.rs` — `MailboxMonitor` and sequencer enqueue/dequeue instrumentation
> `crates/sybil-api/src/config.rs` — threshold configuration
> `deploy/vmalert/rules.yml` — sustained high/critical queue alerts
> `deploy/grafana/dashboards/sybil.json` — queue-depth dashboard panel

## See Also
- [[REST API]] — API-to-sequencer message flow
- [[Order Admission]] — admission backpressure before mailbox pressure
- [[Block Lifecycle]] — actor tick and block production
