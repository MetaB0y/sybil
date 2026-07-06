// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {SybilAccessControl} from "./access/SybilAccessControl.sol";
import {IOpenVmVerifierAdapter} from "./interfaces/IOpenVmVerifierAdapter.sol";
import {ISybilSettlement} from "./interfaces/ISybilSettlement.sol";
import {ISybilVaultDepositRoots} from "./interfaces/ISybilVaultDepositRoots.sol";
import {SybilTypes} from "./SybilTypes.sol";

contract SybilSettlement is SybilAccessControl, ISybilSettlement {
    bytes32 public constant OP_SET_VERIFIER = keccak256("sybil/settlement/set-verifier/v1");
    bytes32 public constant OP_SET_VAULT = keccak256("sybil/settlement/set-vault/v1");

    IOpenVmVerifierAdapter public verifier;
    ISybilVaultDepositRoots public vault;
    uint32 public verifierVersion;
    bool public paused;

    uint64 public latestHeight;
    bytes32 public latestStateRoot;
    uint64 public latestRootVerifiedAt;
    uint64 public latestDepositCount;

    mapping(uint64 height => SybilTypes.RootRecord record) private _rootByHeight;
    mapping(bytes32 stateRoot => uint64 height) public acceptedRootHeight;

    event StateRootVerified(
        uint64 indexed height,
        bytes32 indexed stateRoot,
        bytes32 previousStateRoot,
        bytes32 blockHash,
        bytes32 daCommitment,
        bytes32 depositRoot,
        uint64 depositCount,
        uint32 verifierVersion
    );
    event VerifierUpgraded(uint32 indexed version, address verifier);
    event VaultSet(address indexed vault);
    event Paused(address indexed admin);
    event Unpaused(address indexed admin);

    error InvalidProof();
    error UnknownStateRoot(bytes32 stateRoot);
    error NonMonotonicHeight(uint64 expectedPrevious, uint64 providedPrevious);
    error DepositCountBeyondVault(uint64 providedCount, uint64 vaultCount);
    error NonMonotonicDepositCount(uint64 latestCount, uint64 providedCount);
    error DepositRootMismatch(bytes32 expectedRoot, bytes32 providedRoot);
    error RootAlreadyAccepted(bytes32 stateRoot, uint64 height);
    error ContractPaused();
    error VaultNotSet();

    constructor(
        address admin,
        IOpenVmVerifierAdapter initialVerifier,
        uint64 initialAdminActionDelay
    ) SybilAccessControl(admin, initialAdminActionDelay) {
        if (address(initialVerifier) == address(0)) revert ZeroAddress();
        verifier = initialVerifier;
        verifierVersion = 1;
        emit VerifierUpgraded(verifierVersion, address(initialVerifier));
    }

    function setVault(
        ISybilVaultDepositRoots newVault
    ) external onlyAdmin {
        if (address(newVault) == address(0)) revert ZeroAddress();
        if (address(vault) != address(0)) {
            _consumeTimelock(OP_SET_VAULT, abi.encode(newVault));
        }
        vault = newVault;
        emit VaultSet(address(newVault));
    }

    function proposeVault(
        ISybilVaultDepositRoots newVault
    ) external onlyAdmin returns (bytes32 id) {
        if (address(newVault) == address(0)) revert ZeroAddress();
        return _proposeTimelock(OP_SET_VAULT, abi.encode(newVault));
    }

    function pause() external onlyAdmin {
        paused = true;
        emit Paused(msg.sender);
    }

    function unpause() external onlyAdmin {
        paused = false;
        emit Unpaused(msg.sender);
    }

    function proposeVerifier(
        IOpenVmVerifierAdapter newVerifier
    ) external onlyAdmin returns (bytes32 id) {
        if (address(newVerifier) == address(0)) revert ZeroAddress();
        return _proposeTimelock(OP_SET_VERIFIER, abi.encode(newVerifier));
    }

    function setVerifier(
        IOpenVmVerifierAdapter newVerifier
    ) external onlyAdmin {
        if (address(newVerifier) == address(0)) revert ZeroAddress();
        _consumeTimelock(OP_SET_VERIFIER, abi.encode(newVerifier));
        verifier = newVerifier;
        verifierVersion += 1;
        emit VerifierUpgraded(verifierVersion, address(newVerifier));
    }

    function submitStateRoot(
        SybilTypes.StateTransitionPublicInputs calldata inputs,
        bytes calldata proof
    ) external {
        if (paused) revert ContractPaused();
        if (address(vault) == address(0)) revert VaultNotSet();

        if (inputs.previousHeight != latestHeight) {
            revert NonMonotonicHeight(latestHeight, inputs.previousHeight);
        }
        if (inputs.previousStateRoot != latestStateRoot) {
            revert UnknownStateRoot(inputs.previousStateRoot);
        }
        if (inputs.newHeight <= latestHeight) {
            revert NonMonotonicHeight(latestHeight, inputs.newHeight);
        }
        if (inputs.newStateRoot == bytes32(0)) revert UnknownStateRoot(inputs.newStateRoot);

        uint64 existingHeight = acceptedRootHeight[inputs.newStateRoot];
        if (existingHeight != 0) revert RootAlreadyAccepted(inputs.newStateRoot, existingHeight);

        uint64 vaultDepositCount = vault.depositCount();
        if (inputs.depositCount > vaultDepositCount) {
            revert DepositCountBeyondVault(inputs.depositCount, vaultDepositCount);
        }
        if (inputs.depositCount < latestDepositCount) {
            revert NonMonotonicDepositCount(latestDepositCount, inputs.depositCount);
        }
        bytes32 expectedDepositRoot = vault.depositRootByCount(inputs.depositCount);
        if (expectedDepositRoot == bytes32(0) || expectedDepositRoot != inputs.depositRoot) {
            revert DepositRootMismatch(expectedDepositRoot, inputs.depositRoot);
        }

        bytes32 inputHash = stateTransitionPublicInputHash(inputs);
        if (!verifier.verify(proof, inputHash)) revert InvalidProof();

        uint64 verifiedAt = uint64(block.timestamp);
        SybilTypes.RootRecord memory record = SybilTypes.RootRecord({
            height: inputs.newHeight,
            stateRoot: inputs.newStateRoot,
            previousStateRoot: inputs.previousStateRoot,
            blockHash: inputs.blockHash,
            eventsRoot: inputs.eventsRoot,
            witnessRoot: inputs.witnessRoot,
            daCommitment: inputs.daCommitment,
            depositRoot: inputs.depositRoot,
            depositCount: inputs.depositCount,
            verifiedAt: verifiedAt,
            verifierVersion: verifierVersion
        });

        _rootByHeight[inputs.newHeight] = record;
        acceptedRootHeight[inputs.newStateRoot] = inputs.newHeight;
        latestHeight = inputs.newHeight;
        latestStateRoot = inputs.newStateRoot;
        latestRootVerifiedAt = verifiedAt;
        latestDepositCount = inputs.depositCount;

        emit StateRootVerified(
            inputs.newHeight,
            inputs.newStateRoot,
            inputs.previousStateRoot,
            inputs.blockHash,
            inputs.daCommitment,
            inputs.depositRoot,
            inputs.depositCount,
            verifierVersion
        );
    }

    function isAcceptedRoot(
        bytes32 stateRoot
    ) external view returns (bool) {
        return acceptedRootHeight[stateRoot] != 0;
    }

    function rootAt(
        uint64 height
    ) external view returns (SybilTypes.RootRecord memory) {
        return _rootByHeight[height];
    }

    function stateTransitionPublicInputHash(
        SybilTypes.StateTransitionPublicInputs memory inputs
    ) public pure returns (bytes32) {
        return keccak256(
            abi.encode(
                "sybil/openvm/state-transition/v1",
                inputs.previousHeight,
                inputs.newHeight,
                inputs.previousStateRoot,
                inputs.newStateRoot,
                inputs.blockHash,
                inputs.eventsRoot,
                inputs.witnessRoot,
                inputs.daCommitment,
                inputs.depositRoot,
                inputs.depositCount
            )
        );
    }
}
