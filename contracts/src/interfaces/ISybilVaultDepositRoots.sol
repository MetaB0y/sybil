// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

interface ISybilVaultDepositRoots {
    function depositCount() external view returns (uint64);

    function depositRootByCount(
        uint64 count
    ) external view returns (bytes32);
}
