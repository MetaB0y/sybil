// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {SybilTypes} from "../SybilTypes.sol";

interface ISybilSettlement {
    function submitStateRoot(
        SybilTypes.StateTransitionPublicInputs calldata inputs,
        bytes calldata proof
    ) external;

    function isAcceptedRoot(
        bytes32 stateRoot
    ) external view returns (bool);
    function latestHeight() external view returns (uint64);
    function latestStateRoot() external view returns (bytes32);
    function latestRootVerifiedAt() external view returns (uint64);
    function rootAt(
        uint64 height
    ) external view returns (SybilTypes.RootRecord memory);
}
