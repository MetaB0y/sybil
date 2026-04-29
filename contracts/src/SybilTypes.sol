// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

library SybilTypes {
    struct RootRecord {
        uint64 height;
        bytes32 stateRoot;
        bytes32 previousStateRoot;
        bytes32 blockHash;
        bytes32 eventsRoot;
        bytes32 witnessRoot;
        bytes32 daCommitment;
        bytes32 depositRoot;
        uint64 depositCount;
        uint64 verifiedAt;
        uint32 verifierVersion;
    }

    struct StateTransitionPublicInputs {
        uint64 previousHeight;
        uint64 newHeight;
        bytes32 previousStateRoot;
        bytes32 newStateRoot;
        bytes32 blockHash;
        bytes32 eventsRoot;
        bytes32 witnessRoot;
        bytes32 daCommitment;
        bytes32 depositRoot;
        uint64 depositCount;
    }

    struct WithdrawalPublicInputs {
        bytes32 stateRoot;
        uint64 height;
        bytes32 nullifier;
        address recipient;
        address token;
        uint256 amount;
        bytes32 claimKind;
    }
}
