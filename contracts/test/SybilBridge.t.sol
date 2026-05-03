// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {SybilSettlement} from "../src/SybilSettlement.sol";
import {SybilVault} from "../src/SybilVault.sol";
import {SybilTypes} from "../src/SybilTypes.sol";
import {IOpenVmVerifierAdapter} from "../src/interfaces/IOpenVmVerifierAdapter.sol";
import {MockOpenVmVerifierAdapter} from "./mocks/MockOpenVmVerifierAdapter.sol";
import {MockUSDC} from "./mocks/MockUSDC.sol";

interface Vm {
    function warp(
        uint256 newTimestamp
    ) external;

    function expectCall(
        address callee,
        bytes calldata data
    ) external;
}

contract SybilBridgeTest {
    Vm private constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));

    MockUSDC private token;
    MockOpenVmVerifierAdapter private verifier;
    SybilSettlement private settlement;
    SybilVault private vault;

    uint64 private constant WITHDRAWAL_DELAY = 1 days;
    uint64 private constant ESCAPE_TIMEOUT = 7 days;
    bytes32 private constant ACCOUNT_KEY = keccak256("account-key");
    bytes32 private constant BLOCK_HASH = keccak256("block");
    bytes32 private constant EVENTS_ROOT = keccak256("events");
    bytes32 private constant WITNESS_ROOT = keccak256("witness");
    bytes32 private constant DA_COMMITMENT = keccak256("da");

    function setUp() public {
        token = new MockUSDC();
        verifier = new MockOpenVmVerifierAdapter();
        settlement = new SybilSettlement(address(this), verifier);
        vault = new SybilVault(
            address(this), token, settlement, verifier, WITHDRAWAL_DELAY, ESCAPE_TIMEOUT
        );
        settlement.setVault(vault);

        token.mint(address(this), 10_000_000_000);
        require(token.approve(address(vault), type(uint256).max), "approve");
    }

    function testDepositUpdatesDepth32MerkleRoot() public {
        vault.deposit(1_000_000, ACCOUNT_KEY);

        bytes32 leaf =
            vault.hashDepositLeaf(vault.depositLeaf(1, address(this), ACCOUNT_KEY, 1_000_000));
        bytes32 expected = leaf;
        for (uint8 level = 0; level < 32; level++) {
            expected = vault.hashNode(expected, vault.zeroHashes(level));
        }

        require(vault.depositCount() == 1, "deposit count");
        require(vault.depositRootByCount(1) == expected, "deposit root");
        require(token.balanceOf(address(vault)) == 1_000_000, "vault balance");
    }

    function testStateRootSubmissionUsesMockOpenVmVerifier() public {
        vault.deposit(1_000_000, ACCOUNT_KEY);

        SybilTypes.StateTransitionPublicInputs memory inputs =
            _nextRootInputs(bytes32(0), 0, keccak256("state-1"));

        bytes32 inputHash = settlement.stateTransitionPublicInputHash(inputs);
        vm.expectCall(
            address(verifier),
            abi.encodeWithSelector(
                IOpenVmVerifierAdapter.verify.selector, bytes("proof"), inputHash
            )
        );
        settlement.submitStateRoot(inputs, "proof");

        require(settlement.latestHeight() == 1, "latest height");
        require(settlement.latestStateRoot() == inputs.newStateRoot, "latest root");
        require(settlement.isAcceptedRoot(inputs.newStateRoot), "accepted");
    }

    function testStateRootSubmissionRejectsBadDepositRoot() public {
        SybilTypes.StateTransitionPublicInputs memory inputs =
            _nextRootInputs(bytes32(0), 0, keccak256("state-1"));
        inputs.depositRoot = keccak256("bad-root");

        (bool ok,) = address(settlement)
            .call(
                abi.encodeWithSelector(
                    SybilSettlement.submitStateRoot.selector, inputs, bytes("proof")
                )
            );
        require(!ok, "bad deposit root accepted");
    }

    function testStateRootSubmissionStoresDaCommitment() public {
        SybilTypes.StateTransitionPublicInputs memory inputs =
            _nextRootInputs(bytes32(0), 0, keccak256("state-1"));

        settlement.submitStateRoot(inputs, "proof");

        SybilTypes.RootRecord memory record = settlement.rootAt(inputs.newHeight);
        require(record.daCommitment == DA_COMMITMENT, "da commitment");
    }

    function testStateRootSubmissionRejectsInvalidProof() public {
        vault.deposit(1_000_000, ACCOUNT_KEY);
        verifier.setShouldVerify(false);

        SybilTypes.StateTransitionPublicInputs memory inputs =
            _nextRootInputs(bytes32(0), 0, keccak256("state-1"));

        (bool ok,) = address(settlement)
            .call(
                abi.encodeWithSelector(
                    SybilSettlement.submitStateRoot.selector, inputs, bytes("proof")
                )
            );
        require(!ok, "invalid root proof accepted");
    }

    function testWithdrawalQueueDelayAndFinalize() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        bytes32 nullifier = keccak256("withdrawal-nullifier");
        uint256 balanceBefore = token.balanceOf(address(this));

        SybilTypes.WithdrawalPublicInputs memory inputs = SybilTypes.WithdrawalPublicInputs({
            stateRoot: stateRoot,
            height: settlement.latestHeight(),
            nullifier: nullifier,
            recipient: address(this),
            token: address(token),
            amount: 250_000,
            claimKind: vault.CLAIM_KIND_NORMAL()
        });

        bytes32 inputHash = vault.withdrawalPublicInputHash(inputs);
        vm.expectCall(
            address(verifier),
            abi.encodeWithSelector(
                IOpenVmVerifierAdapter.verify.selector, bytes("withdrawal-proof"), inputHash
            )
        );
        vault.requestWithdrawal(inputs, "withdrawal-proof");

        (bool earlyOk,) = address(vault)
            .call(abi.encodeWithSelector(SybilVault.finalizeWithdrawal.selector, nullifier));
        require(!earlyOk, "early finalize");

        vm.warp(block.timestamp + WITHDRAWAL_DELAY);
        vault.finalizeWithdrawal(nullifier);

        require(token.balanceOf(address(this)) == balanceBefore + 250_000, "recipient paid");
    }

    function testWithdrawalRejectsInvalidProof() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        verifier.setShouldVerify(false);

        SybilTypes.WithdrawalPublicInputs memory inputs = SybilTypes.WithdrawalPublicInputs({
            stateRoot: stateRoot,
            height: settlement.latestHeight(),
            nullifier: keccak256("invalid-withdrawal-proof"),
            recipient: address(this),
            token: address(token),
            amount: 100_000,
            claimKind: vault.CLAIM_KIND_NORMAL()
        });

        (bool ok,) = address(vault)
            .call(
                abi.encodeWithSelector(
                    SybilVault.requestWithdrawal.selector, inputs, bytes("withdrawal-proof")
                )
            );
        require(!ok, "invalid withdrawal proof accepted");
    }

    function testWithdrawalNullifierCannotReplay() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        SybilTypes.WithdrawalPublicInputs memory inputs = SybilTypes.WithdrawalPublicInputs({
            stateRoot: stateRoot,
            height: settlement.latestHeight(),
            nullifier: keccak256("replay-nullifier"),
            recipient: address(this),
            token: address(token),
            amount: 100_000,
            claimKind: vault.CLAIM_KIND_NORMAL()
        });

        vault.requestWithdrawal(inputs, "withdrawal-proof");
        (bool ok,) = address(vault)
            .call(
                abi.encodeWithSelector(
                    SybilVault.requestWithdrawal.selector, inputs, bytes("proof")
                )
            );
        require(!ok, "replay accepted");
    }

    function testPauseBlocksDeposits() public {
        vault.setDepositsPaused(true);
        (bool ok,) = address(vault)
            .call(abi.encodeWithSelector(SybilVault.deposit.selector, 1_000_000, ACCOUNT_KEY));
        require(!ok, "paused deposit accepted");
    }

    function testPauseBlocksRootSubmission() public {
        settlement.setRootSubmissionsPaused(true);
        SybilTypes.StateTransitionPublicInputs memory inputs =
            _nextRootInputs(bytes32(0), 0, keccak256("state-1"));
        (bool ok,) = address(settlement)
            .call(
                abi.encodeWithSelector(
                    SybilSettlement.submitStateRoot.selector, inputs, bytes("proof")
                )
            );
        require(!ok, "paused root accepted");
    }

    function testEscapeModeActivatesAfterTimeout() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        require(stateRoot != bytes32(0), "root");

        (bool earlyOk,) =
            address(vault).call(abi.encodeWithSelector(SybilVault.activateEscapeMode.selector));
        require(!earlyOk, "early escape");

        vm.warp(block.timestamp + ESCAPE_TIMEOUT + 1);
        vault.activateEscapeMode();
        require(vault.escapeModeActive(), "escape active");
    }

    function _acceptRootWithDeposit() internal returns (bytes32) {
        vault.deposit(1_000_000, ACCOUNT_KEY);
        bytes32 stateRoot = keccak256("state-1");
        settlement.submitStateRoot(_nextRootInputs(bytes32(0), 0, stateRoot), "proof");
        return stateRoot;
    }

    function _nextRootInputs(
        bytes32 previousRoot,
        uint64 previousHeight,
        bytes32 newRoot
    ) internal view returns (SybilTypes.StateTransitionPublicInputs memory) {
        return SybilTypes.StateTransitionPublicInputs({
            previousHeight: previousHeight,
            newHeight: previousHeight + 1,
            previousStateRoot: previousRoot,
            newStateRoot: newRoot,
            blockHash: BLOCK_HASH,
            eventsRoot: EVENTS_ROOT,
            witnessRoot: WITNESS_ROOT,
            daCommitment: DA_COMMITMENT,
            depositRoot: vault.depositRootByCount(vault.depositCount()),
            depositCount: vault.depositCount()
        });
    }
}
