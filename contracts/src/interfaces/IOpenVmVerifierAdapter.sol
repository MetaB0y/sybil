// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

interface IOpenVmVerifierAdapter {
    function verify(
        bytes calldata proof,
        bytes32 publicInputHash
    ) external view returns (bool);
}
