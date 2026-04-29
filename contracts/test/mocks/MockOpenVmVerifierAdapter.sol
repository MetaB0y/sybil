// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {IOpenVmVerifierAdapter} from "../../src/interfaces/IOpenVmVerifierAdapter.sol";

contract MockOpenVmVerifierAdapter is IOpenVmVerifierAdapter {
    bool public shouldVerify = true;

    function setShouldVerify(
        bool value
    ) external {
        shouldVerify = value;
    }

    function verify(
        bytes calldata proof,
        bytes32 publicInputHash
    ) external view returns (bool) {
        proof;
        publicInputHash;
        return shouldVerify;
    }
}
