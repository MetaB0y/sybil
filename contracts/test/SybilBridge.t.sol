// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {SybilSettlement} from "../src/SybilSettlement.sol";
import {SybilVault} from "../src/SybilVault.sol";
import {SybilTypes} from "../src/SybilTypes.sol";
import {UnsafeAcceptAllVerifierAdapter} from "../src/dev/UnsafeAcceptAllVerifierAdapter.sol";
import {IOpenVmVerifierAdapter} from "../src/interfaces/IOpenVmVerifierAdapter.sol";
import {MockOpenVmVerifierAdapter} from "./mocks/MockOpenVmVerifierAdapter.sol";
import {MockUSDC} from "./mocks/MockUSDC.sol";

interface Vm {
    function warp(
        uint256 newTimestamp
    ) external;

    function prank(
        address msgSender
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
    MockOpenVmVerifierAdapter private escapeVerifier;
    SybilSettlement private settlement;
    SybilVault private vault;

    uint64 private constant WITHDRAWAL_DELAY = 1 days;
    uint64 private constant ADMIN_TIMELOCK = 2 days;
    uint64 private constant ESCAPE_TIMEOUT = 7 days;
    bytes32 private constant ACCOUNT_KEY = keccak256("account-key");
    bytes32 private constant BLOCK_HASH = keccak256("block");
    bytes32 private constant EVENTS_ROOT = keccak256("events");
    bytes32 private constant WITNESS_ROOT = keccak256("witness");
    bytes32 private constant DA_COMMITMENT = keccak256("da");

    function setUp() public {
        token = new MockUSDC();
        verifier = new MockOpenVmVerifierAdapter();
        escapeVerifier = new MockOpenVmVerifierAdapter();
        settlement = new SybilSettlement(address(this), verifier, ADMIN_TIMELOCK);
        vault = new SybilVault(
            address(this),
            token,
            settlement,
            verifier,
            escapeVerifier,
            WITHDRAWAL_DELAY,
            ESCAPE_TIMEOUT,
            ADMIN_TIMELOCK
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

    function testUnsafeAcceptAllVerifierAcceptsAnyProofThroughSettlement() public {
        UnsafeAcceptAllVerifierAdapter unsafeVerifier = new UnsafeAcceptAllVerifierAdapter();
        UnsafeAcceptAllVerifierAdapter unsafeEscapeVerifier = new UnsafeAcceptAllVerifierAdapter();
        SybilSettlement unsafeSettlement =
            new SybilSettlement(address(this), unsafeVerifier, ADMIN_TIMELOCK);
        SybilVault unsafeVault = new SybilVault(
            address(this),
            token,
            unsafeSettlement,
            unsafeVerifier,
            unsafeEscapeVerifier,
            WITHDRAWAL_DELAY,
            ESCAPE_TIMEOUT,
            ADMIN_TIMELOCK
        );
        unsafeSettlement.setVault(unsafeVault);

        SybilTypes.StateTransitionPublicInputs memory inputs = SybilTypes.StateTransitionPublicInputs({
            previousHeight: 0,
            newHeight: 1,
            previousStateRoot: bytes32(0),
            newStateRoot: keccak256("unsafe-state-1"),
            blockHash: BLOCK_HASH,
            eventsRoot: EVENTS_ROOT,
            witnessRoot: WITNESS_ROOT,
            daCommitment: DA_COMMITMENT,
            depositRoot: unsafeVault.depositRootByCount(0),
            depositCount: 0
        });

        unsafeSettlement.submitStateRoot(inputs, "arbitrary-openvm-app-proof");

        require(unsafeVerifier.unsafeAcceptsAllProofs(), "unsafe marker");
        require(unsafeSettlement.latestHeight() == 1, "latest height");
        require(unsafeSettlement.latestStateRoot() == inputs.newStateRoot, "latest root");
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

    function testStateRootSubmissionRejectsDepositCountBeyondVaultZeroRootBypass() public {
        SybilTypes.StateTransitionPublicInputs memory inputs =
            _nextRootInputs(bytes32(0), 0, keccak256("state-1"));
        inputs.depositCount = vault.depositCount() + 1;
        inputs.depositRoot = bytes32(0);

        (bool ok,) = address(settlement)
            .call(
                abi.encodeWithSelector(
                    SybilSettlement.submitStateRoot.selector, inputs, bytes("proof")
                )
            );
        require(!ok, "unreached zero deposit root accepted");
    }

    function testStateRootSubmissionRejectsDepositCountRegression() public {
        vault.deposit(1_000_000, ACCOUNT_KEY);
        bytes32 stateRoot1 = keccak256("state-1");
        settlement.submitStateRoot(_nextRootInputs(bytes32(0), 0, stateRoot1), "proof");

        SybilTypes.StateTransitionPublicInputs memory inputs =
            _nextRootInputs(stateRoot1, 1, keccak256("state-2"));
        inputs.depositCount = 0;
        inputs.depositRoot = vault.depositRootByCount(0);

        (bool ok,) = address(settlement)
            .call(
                abi.encodeWithSelector(
                    SybilSettlement.submitStateRoot.selector, inputs, bytes("proof")
                )
            );
        require(!ok, "deposit count regression accepted");
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

    function testWithdrawalCancelDuringWindowAllowsReRequest() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        SybilTypes.WithdrawalPublicInputs memory inputs = SybilTypes.WithdrawalPublicInputs({
            stateRoot: stateRoot,
            height: settlement.latestHeight(),
            nullifier: keccak256("cancel-requeue-nullifier"),
            recipient: address(this),
            token: address(token),
            amount: 100_000,
            claimKind: vault.CLAIM_KIND_NORMAL()
        });

        vault.requestWithdrawal(inputs, "withdrawal-proof");
        vault.cancelWithdrawal(inputs.nullifier, "fraud response");

        require(!vault.nullifierUsed(inputs.nullifier), "cancelled nullifier stayed burned");
        vault.requestWithdrawal(inputs, "withdrawal-proof-2");
        require(vault.nullifierUsed(inputs.nullifier), "requeued nullifier not burned");
    }

    function testWithdrawalCancelRejectedAfterDelay() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        bytes32 nullifier = keccak256("late-cancel-nullifier");
        SybilTypes.WithdrawalPublicInputs memory inputs = SybilTypes.WithdrawalPublicInputs({
            stateRoot: stateRoot,
            height: settlement.latestHeight(),
            nullifier: nullifier,
            recipient: address(this),
            token: address(token),
            amount: 100_000,
            claimKind: vault.CLAIM_KIND_NORMAL()
        });

        vault.requestWithdrawal(inputs, "withdrawal-proof");
        vm.warp(block.timestamp + WITHDRAWAL_DELAY);

        (bool ok,) = address(vault)
            .call(abi.encodeWithSelector(SybilVault.cancelWithdrawal.selector, nullifier, "late"));
        require(!ok, "late cancel accepted");
    }

    function testPauseBlocksDeposits() public {
        vault.pause();
        (bool ok,) = address(vault)
            .call(abi.encodeWithSelector(SybilVault.deposit.selector, 1_000_000, ACCOUNT_KEY));
        require(!ok, "paused deposit accepted");
    }

    function testPauseBlocksWithdrawalRequestAndFinalization() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        bytes32 nullifier = keccak256("paused-finalization-nullifier");
        SybilTypes.WithdrawalPublicInputs memory inputs = SybilTypes.WithdrawalPublicInputs({
            stateRoot: stateRoot,
            height: settlement.latestHeight(),
            nullifier: nullifier,
            recipient: address(this),
            token: address(token),
            amount: 100_000,
            claimKind: vault.CLAIM_KIND_NORMAL()
        });

        vault.requestWithdrawal(inputs, "withdrawal-proof");
        vault.pause();

        inputs.nullifier = keccak256("paused-request-nullifier");
        (bool requestOk,) = address(vault)
            .call(
                abi.encodeWithSelector(
                    SybilVault.requestWithdrawal.selector, inputs, bytes("withdrawal-proof")
                )
            );
        require(!requestOk, "paused request accepted");

        vm.warp(block.timestamp + WITHDRAWAL_DELAY);
        (bool finalizeOk,) = address(vault)
            .call(abi.encodeWithSelector(SybilVault.finalizeWithdrawal.selector, nullifier));
        require(!finalizeOk, "paused finalize accepted");

        vault.unpause();
        vault.finalizeWithdrawal(nullifier);
    }

    function testPauseBlocksRootSubmission() public {
        settlement.pause();
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

    function testSettlementVerifierChangeRequiresTimelock() public {
        MockOpenVmVerifierAdapter newVerifier = new MockOpenVmVerifierAdapter();

        settlement.proposeVerifier(newVerifier);
        (bool earlyOk,) = address(settlement)
            .call(abi.encodeWithSelector(SybilSettlement.setVerifier.selector, newVerifier));
        require(!earlyOk, "early verifier update accepted");

        vm.warp(block.timestamp + ADMIN_TIMELOCK);
        settlement.setVerifier(newVerifier);

        require(address(settlement.verifier()) == address(newVerifier), "verifier not updated");
        require(settlement.verifierVersion() == 2, "verifier version");
    }

    function testTimelockProposalCanBeCancelled() public {
        bytes32 proposal = vault.proposeWithdrawalDelay(2 days);
        vault.cancelProposal(proposal);
        vm.warp(block.timestamp + ADMIN_TIMELOCK);

        (bool ok,) = address(vault)
            .call(abi.encodeWithSelector(SybilVault.setWithdrawalDelay.selector, uint64(2 days)));
        require(!ok, "cancelled proposal executed");
        require(vault.withdrawalDelay() == WITHDRAWAL_DELAY, "delay changed");
    }

    function testVaultWithdrawalDelayChangeRequiresTimelock() public {
        uint64 newDelay = 2 days;

        vault.proposeWithdrawalDelay(newDelay);
        (bool earlyOk,) = address(vault)
            .call(abi.encodeWithSelector(SybilVault.setWithdrawalDelay.selector, newDelay));
        require(!earlyOk, "early delay update accepted");

        vm.warp(block.timestamp + ADMIN_TIMELOCK);
        vault.setWithdrawalDelay(newDelay);

        require(vault.withdrawalDelay() == newDelay, "delay not updated");
    }

    function testAdminTransferRequiresTimelock() public {
        address newAdmin = address(0xBEEF);

        vault.proposeAdminTransfer(newAdmin);
        (bool earlyOk,) = address(vault)
            .call(
                abi.encodeWithSelector(bytes4(keccak256("executeAdminTransfer(address)")), newAdmin)
            );
        require(!earlyOk, "early admin transfer accepted");

        vm.warp(block.timestamp + ADMIN_TIMELOCK);
        vault.executeAdminTransfer(newAdmin);

        require(vault.admin() == newAdmin, "admin not transferred");
        (bool oldAdminOk,) = address(vault).call(abi.encodeWithSelector(SybilVault.pause.selector));
        require(!oldAdminOk, "old admin still controls vault");

        vm.prank(newAdmin);
        vault.pause();
        require(vault.paused(), "new admin cannot pause");
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

    function testEscapeModeActivatesBeforeFirstRootAfterDeploymentTimeout() public {
        // No root has ever been accepted, but a user deposited before the
        // operator produced any root. Escape must still become activatable once
        // escapeTimeout elapses from deployment, so those deposits are not
        // trapped by the operator disappearing pre-genesis.
        vault.deposit(1_000_000, ACCOUNT_KEY);
        require(settlement.latestRootVerifiedAt() == 0, "no root yet");

        (bool earlyOk,) =
            address(vault).call(abi.encodeWithSelector(SybilVault.activateEscapeMode.selector));
        require(!earlyOk, "early escape before first root");

        vm.warp(vault.deployedAt() + ESCAPE_TIMEOUT + 1);
        vault.activateEscapeMode();
        require(vault.escapeModeActive(), "escape active pre-first-root");
    }

    function testEscapeClaimPaysWhileVaultPaused() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        _activateEscapeMode();
        vault.pause();

        address recipient = address(0xBEEF);
        uint256 amount = 125_000;
        SybilTypes.EscapeClaimPublicInputs memory inputs =
            _escapeInputs(stateRoot, settlement.latestHeight(), 41, recipient, amount);

        uint256 recipientBefore = token.balanceOf(recipient);
        vault.escapeClaim(inputs, "escape-proof");

        require(vault.paused(), "vault unexpectedly unpaused");
        require(token.balanceOf(recipient) == recipientBefore + amount, "paused escape not paid");
    }

    function testEscapeClaimRevertsWhenEscapeModeInactive() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        SybilTypes.EscapeClaimPublicInputs memory inputs =
            _escapeInputs(stateRoot, settlement.latestHeight(), 42, address(0xBEEF), 100_000);

        (bool ok,) = address(vault)
            .call(abi.encodeWithSelector(SybilVault.escapeClaim.selector, inputs, bytes("proof")));

        require(!ok, "inactive escape claim accepted");
        require(!vault.nullifierUsed(inputs.nullifier), "inactive claim burned nullifier");
    }

    function testEscapeClaimRevertsForStaleRoot() public {
        bytes32 staleRoot = _acceptRootWithDeposit();
        _activateEscapeMode();

        // Latest-at-claim semantics remain in force if the operator resumes
        // root submission after escape activation.
        bytes32 latestRoot = keccak256("state-2");
        settlement.submitStateRoot(_nextRootInputs(staleRoot, 1, latestRoot), "proof");

        SybilTypes.EscapeClaimPublicInputs memory inputs =
            _escapeInputs(staleRoot, 1, 43, address(0xBEEF), 100_000);
        (bool ok,) = address(vault)
            .call(abi.encodeWithSelector(SybilVault.escapeClaim.selector, inputs, bytes("proof")));

        require(!ok, "stale-root escape claim accepted");
        require(!vault.nullifierUsed(inputs.nullifier), "stale claim burned nullifier");
    }

    function testEscapeClaimRevertsForWrongHeightAtLatestRoot() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        _activateEscapeMode();

        SybilTypes.EscapeClaimPublicInputs memory inputs =
            _escapeInputs(stateRoot, settlement.latestHeight() + 1, 44, address(0xBEEF), 100_000);
        (bool ok,) = address(vault)
            .call(abi.encodeWithSelector(SybilVault.escapeClaim.selector, inputs, bytes("proof")));

        require(!ok, "wrong-height escape claim accepted");
        require(!vault.nullifierUsed(inputs.nullifier), "wrong-height claim burned nullifier");
    }

    function testEscapeClaimRevertsForWrongNullifier() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        _activateEscapeMode();

        SybilTypes.EscapeClaimPublicInputs memory inputs =
            _escapeInputs(stateRoot, settlement.latestHeight(), 48, address(0xBEEF), 100_000);
        inputs.nullifier = keccak256("wrong-escape-nullifier");
        (bool ok,) = address(vault)
            .call(abi.encodeWithSelector(SybilVault.escapeClaim.selector, inputs, bytes("proof")));

        require(!ok, "wrong escape nullifier accepted");
        require(!vault.nullifierUsed(inputs.nullifier), "wrong nullifier burned");
    }

    function testEscapeClaimInvalidProofDoesNotBurnNullifier() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        _activateEscapeMode();
        escapeVerifier.setShouldVerify(false);

        SybilTypes.EscapeClaimPublicInputs memory inputs =
            _escapeInputs(stateRoot, settlement.latestHeight(), 49, address(0xBEEF), 100_000);
        (bool ok,) = address(vault)
            .call(abi.encodeWithSelector(SybilVault.escapeClaim.selector, inputs, bytes("proof")));

        require(!ok, "invalid escape proof accepted");
        require(!vault.nullifierUsed(inputs.nullifier), "invalid proof burned nullifier");
    }

    function testEscapeClaimDoubleClaimReverts() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        _activateEscapeMode();

        address recipient = address(0xBEEF);
        uint256 amount = 100_000;
        SybilTypes.EscapeClaimPublicInputs memory inputs =
            _escapeInputs(stateRoot, settlement.latestHeight(), 45, recipient, amount);
        vault.escapeClaim(inputs, "escape-proof");
        uint256 balanceAfterFirstClaim = token.balanceOf(recipient);

        (bool ok,) = address(vault)
            .call(
                abi.encodeWithSelector(
                    SybilVault.escapeClaim.selector, inputs, bytes("second-proof")
                )
            );

        require(!ok, "double escape claim accepted");
        require(token.balanceOf(recipient) == balanceAfterFirstClaim, "double claim paid");
    }

    function testEscapeClaimMockVerifierProofPlumbingPaysExactAmount() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        _activateEscapeMode();

        address recipient = address(0xBEEF);
        uint256 amount = 234_567;
        SybilTypes.EscapeClaimPublicInputs memory inputs =
            _escapeInputs(stateRoot, settlement.latestHeight(), 46, recipient, amount);
        bytes32 inputHash = vault.escapeClaimPublicInputHash(inputs);
        vm.expectCall(
            address(escapeVerifier),
            abi.encodeWithSelector(
                IOpenVmVerifierAdapter.verify.selector, bytes("escape-proof"), inputHash
            )
        );

        uint256 recipientBefore = token.balanceOf(recipient);
        uint256 vaultBefore = token.balanceOf(address(vault));
        vault.escapeClaim(inputs, "escape-proof");

        require(token.balanceOf(recipient) == recipientBefore + amount, "recipient amount");
        require(token.balanceOf(address(vault)) == vaultBefore - amount, "vault amount");
        require(vault.nullifierUsed(inputs.nullifier), "escape nullifier not consumed");
    }

    function testEscapeAndWithdrawalNullifiersDoNotCollideOrBlockEachOther() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        _activateEscapeMode();

        uint64 accountId = 47;
        uint64 withdrawalId = 47;
        address recipient = address(0xBEEF);
        uint256 amount = 100_000;
        bytes32 withdrawalNullifier = keccak256(
            abi.encode(
                "sybil/withdrawal-nullifier/v1",
                block.chainid,
                address(vault),
                withdrawalId,
                accountId,
                recipient,
                address(token),
                amount
            )
        );
        SybilTypes.WithdrawalPublicInputs memory withdrawalInputs = SybilTypes.WithdrawalPublicInputs({
            stateRoot: stateRoot,
            height: settlement.latestHeight(),
            nullifier: withdrawalNullifier,
            recipient: recipient,
            token: address(token),
            amount: amount,
            claimKind: vault.CLAIM_KIND_NORMAL()
        });
        SybilTypes.EscapeClaimPublicInputs memory escapeInputs =
            _escapeInputs(stateRoot, settlement.latestHeight(), accountId, recipient, amount);

        require(withdrawalNullifier != escapeInputs.nullifier, "nullifier domains collided");
        vault.requestWithdrawal(withdrawalInputs, "withdrawal-proof");
        vault.escapeClaim(escapeInputs, "escape-proof");
        require(vault.nullifierUsed(withdrawalNullifier), "withdrawal nullifier not used");
        require(vault.nullifierUsed(escapeInputs.nullifier), "escape nullifier not used");

        // The only un-spend path applies to an actual queued withdrawal and
        // must never clear the independently consumed escape nullifier.
        vault.cancelWithdrawal(withdrawalNullifier, "cross-domain test");
        require(!vault.nullifierUsed(withdrawalNullifier), "withdrawal nullifier not released");
        require(vault.nullifierUsed(escapeInputs.nullifier), "escape nullifier released");
        vault.requestWithdrawal(withdrawalInputs, "withdrawal-proof-2");
        require(vault.nullifierUsed(withdrawalNullifier), "escape blocked withdrawal requeue");
    }

    function testVaultEscapeVerifierChangeRequiresTimelock() public {
        MockOpenVmVerifierAdapter newEscapeVerifier = new MockOpenVmVerifierAdapter();

        vault.proposeEscapeVerifier(newEscapeVerifier);
        (bool earlyOk,) = address(vault)
            .call(abi.encodeWithSelector(SybilVault.setEscapeVerifier.selector, newEscapeVerifier));
        require(!earlyOk, "early escape verifier update accepted");

        vm.warp(block.timestamp + ADMIN_TIMELOCK);
        vault.setEscapeVerifier(newEscapeVerifier);

        require(
            address(vault.escapeVerifier()) == address(newEscapeVerifier),
            "escape verifier not updated"
        );
    }

    function testRequestWithdrawalRejectsNonNormalClaimKind() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        SybilTypes.WithdrawalPublicInputs memory inputs = SybilTypes.WithdrawalPublicInputs({
            stateRoot: stateRoot,
            height: settlement.latestHeight(),
            nullifier: keccak256("escape-claim-nullifier"),
            recipient: address(this),
            token: address(token),
            amount: 100_000,
            claimKind: keccak256("sybil/claim-kind/escape-cash/v1")
        });

        (bool ok,) = address(vault)
            .call(
                abi.encodeWithSelector(
                    SybilVault.requestWithdrawal.selector, inputs, bytes("withdrawal-proof")
                )
            );
        require(!ok, "non-normal claim kind accepted");
        // Fail-closed must not burn the nullifier or queue anything.
        require(!vault.nullifierUsed(inputs.nullifier), "nullifier burned on reject");
    }

    function _acceptRootWithDeposit() internal returns (bytes32) {
        vault.deposit(1_000_000, ACCOUNT_KEY);
        bytes32 stateRoot = keccak256("state-1");
        settlement.submitStateRoot(_nextRootInputs(bytes32(0), 0, stateRoot), "proof");
        return stateRoot;
    }

    function _activateEscapeMode() internal {
        vm.warp(block.timestamp + ESCAPE_TIMEOUT + 1);
        vault.activateEscapeMode();
    }

    function _escapeInputs(
        bytes32 stateRoot,
        uint64 height,
        uint64 accountId,
        address recipient,
        uint256 amount
    ) internal view returns (SybilTypes.EscapeClaimPublicInputs memory) {
        bytes32 nullifier = keccak256(
            abi.encode(
                "sybil/escape-nullifier/v1", block.chainid, address(vault), accountId, stateRoot
            )
        );
        return SybilTypes.EscapeClaimPublicInputs({
            stateRoot: stateRoot,
            height: height,
            accountId: accountId,
            recipient: recipient,
            amount: amount,
            nullifier: nullifier
        });
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
