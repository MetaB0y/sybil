// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {IOpenVmVerifierAdapter} from "../interfaces/IOpenVmVerifierAdapter.sol";

/// @notice Sepolia-only adapter that deliberately accepts every proof.
/// @dev The constructor prevents this bytecode from being deployed on another
///      chain. Pair it only with a freshly deployed MintableMockUSDC vault.
contract UnsafeSepoliaMockVerifierAdapter is IOpenVmVerifierAdapter {
    uint256 public constant SEPOLIA_CHAIN_ID = 11_155_111;
    string public constant WARNING = "UNSAFE_SEPOLIA_MOCK_ACCEPT_ALL_PROOFS";

    error WrongChain(uint256 expected, uint256 actual);

    constructor() {
        if (block.chainid != SEPOLIA_CHAIN_ID) {
            revert WrongChain(SEPOLIA_CHAIN_ID, block.chainid);
        }
    }

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
