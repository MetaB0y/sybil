// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

interface ISybilVaultDepositRoots {
    function depositRootByCount(
        uint64 count
    ) external view returns (bytes32);
}
