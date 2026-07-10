# Contract coverage baseline

Report-only baseline captured on 2026-07-10 with `forge coverage` using Forge
1.6.0-v1.7.0 (`f83bad9`). Foundry disabled optimizer settings and `viaIR` for
the coverage build. All 34 tests passed.

| File | Lines | Statements | Branches | Functions |
|---|---:|---:|---:|---:|
| `script/UnsafeAnvilSmoke.s.sol` | 0.00% (0/26) | 0.00% (0/33) | 0.00% (0/8) | 0.00% (0/2) |
| `src/OpenVmVerifierAdapter.sol` | 100.00% (26/26) | 92.86% (26/28) | 77.78% (7/9) | 100.00% (4/4) |
| `src/SybilSettlement.sol` | 84.85% (56/66) | 77.03% (57/74) | 29.41% (5/17) | 81.82% (9/11) |
| `src/SybilVault.sol` | 82.20% (97/118) | 76.34% (100/131) | 37.93% (11/29) | 78.95% (15/19) |
| `src/access/SybilAccessControl.sol` | 79.17% (38/48) | 70.00% (35/50) | 22.22% (2/9) | 72.73% (8/11) |
| `src/dev/UnsafeAcceptAllVerifierAdapter.sol` | 100.00% (4/4) | 100.00% (2/2) | 100.00% (0/0) | 100.00% (2/2) |
| `test/SybilGoldenVectors.t.sol` | 100.00% (13/13) | 100.00% (13/13) | 100.00% (0/0) | 100.00% (2/2) |
| `test/mocks/MockOpenVmHalo2Verifier.sol` | 100.00% (4/4) | 100.00% (2/2) | 100.00% (1/1) | 100.00% (2/2) |
| `test/mocks/MockOpenVmVerifierAdapter.sol` | 100.00% (4/4) | 100.00% (2/2) | 100.00% (0/0) | 100.00% (2/2) |
| `test/mocks/MockUSDC.sol` | 95.65% (22/23) | 94.44% (17/18) | 40.00% (2/5) | 100.00% (5/5) |
| **Total** | **79.52% (264/332)** | **71.95% (254/353)** | **35.90% (28/78)** | **81.67% (49/60)** |

## Largest settlement/vault money-path gaps

- Settlement branch coverage is only 29.41%; root submission still lacks a
  systematic rejection matrix for broken height/state-root chaining, zero or
  duplicate new roots, an unset vault, and an unavailable zero deposit root.
- Deposit tests cover successful custody and pausing, but do not exercise a
  collateral token returning `false` from `transferFrom`; that `TransferFailed`
  branch is the deposit path's primary external-call failure.
- Withdrawal request coverage does not form a full input-validation matrix for
  zero amount, unsupported token, and unknown accepted state root before proof
  verification.
- Withdrawal finalization/cancellation does not comprehensively cover unknown,
  already-finalized, and already-canceled records, or a token returning `false`
  from the final payout transfer.
- Escape/timelocked administration has broad happy-path coverage, but repeated
  escape activation and the verifier/escape-timeout mutation branches remain
  thinner than the withdrawal-delay path they can affect operationally.
