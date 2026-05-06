// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {IOpenVmVerifierAdapter} from "../interfaces/IOpenVmVerifierAdapter.sol";

/// @notice Devnet-only verifier adapter that accepts every proof.
/// @dev This keeps SybilSettlement/SybilVault wired through the final verifier
///      boundary while allowing local VM/devnet plumbing before EVM verifier
///      artifacts are cheap enough to run routinely.
contract UnsafeAcceptAllVerifierAdapter is IOpenVmVerifierAdapter {
    string public constant WARNING = "UNSAFE_ACCEPT_ALL_VERIFIER_ADAPTER";

    function unsafeAcceptsAllProofs() external pure returns (bool) {
        return true;
    }

    function verify(
        bytes calldata proof,
        bytes32 publicInputHash
    ) external pure returns (bool) {
        proof;
        publicInputHash;
        return true;
    }
}
