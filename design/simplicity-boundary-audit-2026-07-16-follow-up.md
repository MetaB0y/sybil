---
status: unreviewed-codex-generated
tags: [audit, architecture, simplicity, operations]
last_verified: 2026-07-16
---

# Simplicity and Boundary Audit Follow-up — 2026-07-16

This follow-up audits the live product/validity/operations path after the
broader [repository simplification audit](repository-simplification-audit-2026-07-16.html).
It focuses on whether ownership is explicit, optional subsystems disappear
cleanly, and deployment choices have one source of truth.

## Immediate findings implemented

### 1. Product was retaining validity artifacts without a validity consumer

**Finding:** The product devnet omitted the prover but the sequencer still
serialized and retained a portable proof job and DA witness payload for every
block. Exact-byte source ownership correctly prevented deletion because no
prover could acknowledge the rows; the deployment topology made that safety
invariant an unbounded product leak.

**Decision:** Make artifact retention explicit chain identity
(`SYBIL_RETAIN_VALIDITY_ARTIFACTS`). Product mode keeps native verification,
state roots, qMDB recovery state, the latest recovery witness, canonical replay,
and product history, but creates neither proof-job nor DA archive rows. The
validity overlay enables both streams from block 1. Persistent stores reject
mode changes and unbound legacy chains, requiring fresh genesis.

**Issue:** #154.

### 2. Monitoring configuration did not belong to optional service profiles

**Finding:** Static targets created false failures when optional services were
absent; DNS discovery avoided false series but emitted continual lookup errors.
A Docker-socket discovery dependency would have widened the operations trust
boundary solely to answer a configuration question Compose already knows.

**Decision:** Keep one scrape configuration and give each optional job a
profile-selected file-discovery input. Product mounts an empty JSON target set;
the validity and L1 overlays replace exactly their own file mount. No optional
profile means no target, no error, and no `up=0` series.

**Issue:** #146.

### 3. Starting a prover was incorrectly shaped as an in-place service toggle

**Finding:** Once product mode stops retaining proof jobs, adding a prover later
cannot reconstruct a contiguous validity history. The old deploy recipe could
start the process against a chain that had never produced its input.

**Decision:** The explicit prover deploy now requires `CONFIRM`, clears all
coupled state, selects the validity chain/monitoring overlays, and starts from
fresh genesis. The store-level binding independently fails closed if an
operator bypasses the recipe.

## Findings retained as separate work

- **Prover retained-memory ownership (#137):** product isolation removes the
  immediate 2 GB host hazard, but the prover still needs a real policy for
  compacting completed jobs/epochs and bounding resident indexes before it can
  soak indefinitely. A larger cgroup is not that policy.
- **Sequencer resident memory and recovery (#140):** the original live RSS
  measurement was confounded by the accidentally retained proof-job stream.
  Measure the fresh product chain after #154 before designing canonical
  snapshots or WAL changes. Implement only the remaining demonstrated growth;
  do not add a second recovery mechanism to solve a leak that no longer exists.

## Deliberately retained complexity

- The latest block witness remains in product mode because it is useful for
  native inspection and disaster recovery and is latest-only, not an unbounded
  historical stream.
- DA routes and proof-job source routes remain in the common API binary. Their
  empty result in product mode is honest and keeps one typed API/schema. Removing
  routes per profile would multiply router/OpenAPI variants for little runtime
  benefit.
- `save_block`, witnessed test/witgen commits, full actor commits, and imported
  checkpoints still have distinct persistence entry points. Collapsing them is
  attractive, but the callers intentionally exercise different crash/recovery
  boundaries; a smaller API is not clearly a net simplification without first
  redesigning those test and witgen workflows.

## Verification required before closing the findings

1. Full sequencer/API tests, monitoring config/rule tests, Compose contract
   checks, formatting/lint, and architecture-doc validation.
2. Fresh-genesis product deployment with the retention gauge at zero and no
   prover container/target.
3. Repeated samples of API RSS, database size, proof-job outbox, DA rows, block
   progress, history delivery, fills, rejections, and all health/alert gates.
4. Restart-resilience verification on the same new chain.
