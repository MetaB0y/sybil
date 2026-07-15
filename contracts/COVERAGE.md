# Contract money-path coverage gate

`just contracts-coverage` runs Foundry coverage, filters out scripts, tests,
mocks, and the explicitly unsafe development adapter, then enforces branch
floors on the four production contracts that accept proofs, advance roots,
move collateral, or control those operations. The same gate runs in the
`Contracts` CI job and in `just check-all`.

## Current measured baseline

Captured on 2026-07-15 with Forge 1.6.0-v1.7.0 (`f83bad9`). Foundry disables
optimizer settings and `viaIR` for coverage. All 79 tests passed.

| Production file | Lines | Statements | Branches | Functions |
|---|---:|---:|---:|---:|
| `src/OpenVmVerifierAdapter.sol` | 100.00% (26/26) | 100.00% (28/28) | 100.00% (9/9) | 100.00% (4/4) |
| `src/SybilSettlement.sol` | 95.45% (63/66) | 97.30% (72/74) | 100.00% (17/17) | 90.91% (10/11) |
| `src/SybilVault.sol` | 100.00% (157/157) | 100.00% (180/180) | 100.00% (43/43) | 100.00% (23/23) |
| `src/access/SybilAccessControl.sol` | 95.83% (46/48) | 94.00% (47/50) | 88.89% (8/9) | 90.91% (10/11) |
| **Filtered total** | **98.32% (292/297)** | **98.49% (327/332)** | **98.72% (77/78)** | **95.92% (47/49)** |

The only currently uncredited production branch is the false arm of the
inherited `onlyAdmin` modifier. Exact-selector tests invoke that arm on both
settlement and vault, but this Foundry source-map build does not attribute the
hit to `SybilAccessControl.sol`.

## Enforced floors

| Production file | Branch floor |
|---|---:|
| `src/OpenVmVerifierAdapter.sol` | 85% |
| `src/SybilSettlement.sol` | 90% |
| `src/SybilVault.sol` | 90% |
| `src/access/SybilAccessControl.sol` | 75% |
| **Aggregate** | **95%** |

The per-file floors prevent a highly covered vault from hiding erosion in the
smaller adapter or access-control boundary. The aggregate floor permits at
most a very small amount of instrumentation drift from the current 77/78 while
still failing broad regression. Floors deliberately retain headroom instead of
turning compiler/source-map details into protocol requirements; branch coverage
is a spotlight, not a substitute for the named behavior tests.

## Named money-path behavior

The focused failure suite now pins:

- settlement rejection for an unset vault; broken previous height/root;
  non-forward height; zero/duplicate roots; deposit count/root mismatch,
  regression, or unavailable zero roots; pause; and invalid proofs;
- deposit zero-amount, pause/escape shutdown, and ERC-20 `transferFrom`
  returning `false` without moving custody or advancing the accumulator;
- withdrawal claim-kind, amount, token, accepted-root, proof, and nullifier
  validation before queueing;
- early, unknown, canceled, finalized, and replayed withdrawal operations,
  including payout `false` rollback followed by a successful retry;
- frozen-root/height/nullifier escape validation, repeated activation,
  invalid-proof and payout-failure rollback, double-claim prevention, and the
  deliberate pause bypass;
- zero deployment/rotation dependencies, non-admin calls, unknown/duplicate/
  early/replayed timelock operations, verifier/vault/delay/timeout mutations,
  admin transfer, and timestamp overflow.

Scripts are deployment plumbing rather than contract state machines and remain
outside the floor. `src/dev/` is deliberately excluded: both accept-all
adapters and the publicly mintable Sepolia collateral are unsafe environment
fixtures, not production validity or custody implementations. Their chain and
warning boundaries have focused tests and deployment smoke instead.
Golden-vector parity and Anvil deployment smoke remain separate gates.
