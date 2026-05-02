// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {IOpenVmHalo2Verifier} from "../../src/interfaces/IOpenVmHalo2Verifier.sol";

contract MockOpenVmHalo2Verifier is IOpenVmHalo2Verifier {
    bool public shouldRevert;

    function setShouldRevert(
        bool value
    ) external {
        shouldRevert = value;
    }

    function verify(
        bytes calldata publicValues,
        bytes calldata proofData,
        bytes32 appExeCommit,
        bytes32 appVmCommit
    ) external view {
        publicValues;
        proofData;
        appExeCommit;
        appVmCommit;

        if (shouldRevert) revert("mock invalid OpenVM proof");
    }
}
