// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {SybilSettlement} from "../src/SybilSettlement.sol";
import {SybilTypes} from "../src/SybilTypes.sol";
import {SybilVault} from "../src/SybilVault.sol";
import {SybilAccessControl} from "../src/access/SybilAccessControl.sol";
import {ISybilVaultDepositRoots} from "../src/interfaces/ISybilVaultDepositRoots.sol";
import {MockOpenVmVerifierAdapter} from "./mocks/MockOpenVmVerifierAdapter.sol";
import {MockUSDC} from "./mocks/MockUSDC.sol";

interface MoneyPathVm {
    function prank(
        address msgSender
    ) external;

    function warp(
        uint256 newTimestamp
    ) external;
}

contract MockDepositRoots is ISybilVaultDepositRoots {
    uint64 public depositCount;
    mapping(uint64 count => bytes32 root) public depositRootByCount;

    function setDepositCount(
        uint64 count
    ) external {
        depositCount = count;
    }

    function setDepositRoot(
        uint64 count,
        bytes32 root
    ) external {
        depositRootByCount[count] = root;
    }
}

contract SybilMoneyPathFailuresTest {
    MoneyPathVm private constant vm =
        MoneyPathVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    uint64 private constant WITHDRAWAL_DELAY = 1 days;
    uint64 private constant ADMIN_TIMELOCK = 2 days;
    uint64 private constant ESCAPE_TIMEOUT = 7 days;
    bytes32 private constant ACCOUNT_KEY = keccak256("money-path-account-key");
    bytes32 private constant BLOCK_HASH = keccak256("money-path-block");
    bytes32 private constant EVENTS_ROOT = keccak256("money-path-events");
    bytes32 private constant WITNESS_ROOT = keccak256("money-path-witness");
    bytes32 private constant DA_COMMITMENT = keccak256("money-path-da");

    MockUSDC private token;
    MockOpenVmVerifierAdapter private verifier;
    MockOpenVmVerifierAdapter private escapeVerifier;
    SybilSettlement private settlement;
    SybilVault private vault;

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

    function testSettlementRejectsSubmissionBeforeVaultIsSet() public {
        SybilSettlement unset = new SybilSettlement(address(this), verifier, ADMIN_TIMELOCK);
        SybilTypes.StateTransitionPublicInputs memory inputs =
            _rootInputs(bytes32(0), 0, 1, keccak256("state-1"), bytes32(uint256(1)), 0);

        _assertRevertSelector(
            address(unset),
            abi.encodeWithSelector(
                SybilSettlement.submitStateRoot.selector, inputs, bytes("proof")
            ),
            SybilSettlement.VaultNotSet.selector
        );
    }

    function testSettlementRejectsZeroAddressDeploymentAndRotations() public {
        try new SybilSettlement(address(0), verifier, ADMIN_TIMELOCK) returns (
            SybilSettlement unexpected
        ) {
            unexpected;
            revert("zero-admin settlement deployed");
        } catch (bytes memory revertData) {
            _assertRevertDataSelector(revertData, SybilAccessControl.ZeroAddress.selector);
        }
        try new SybilSettlement(
            address(this), MockOpenVmVerifierAdapter(address(0)), ADMIN_TIMELOCK
        ) returns (
            SybilSettlement unexpected
        ) {
            unexpected;
            revert("zero-verifier settlement deployed");
        } catch (bytes memory revertData) {
            _assertRevertDataSelector(revertData, SybilAccessControl.ZeroAddress.selector);
        }

        _assertRevertSelector(
            address(settlement),
            abi.encodeWithSelector(
                SybilSettlement.proposeVault.selector, ISybilVaultDepositRoots(address(0))
            ),
            SybilAccessControl.ZeroAddress.selector
        );
        _assertRevertSelector(
            address(settlement),
            abi.encodeWithSelector(
                SybilSettlement.setVault.selector, ISybilVaultDepositRoots(address(0))
            ),
            SybilAccessControl.ZeroAddress.selector
        );
        _assertRevertSelector(
            address(settlement),
            abi.encodeWithSelector(
                SybilSettlement.proposeVerifier.selector, MockOpenVmVerifierAdapter(address(0))
            ),
            SybilAccessControl.ZeroAddress.selector
        );
        _assertRevertSelector(
            address(settlement),
            abi.encodeWithSelector(
                SybilSettlement.setVerifier.selector, MockOpenVmVerifierAdapter(address(0))
            ),
            SybilAccessControl.ZeroAddress.selector
        );
    }

    function testSettlementRejectsWrongPreviousHeight() public {
        SybilTypes.StateTransitionPublicInputs memory inputs =
            _validRootInputs(bytes32(0), 0, keccak256("state-1"));
        inputs.previousHeight = 1;
        inputs.newHeight = 2;

        _assertRootSubmissionReverts(inputs, SybilSettlement.NonMonotonicHeight.selector);
    }

    function testSettlementRejectsWrongPreviousRoot() public {
        SybilTypes.StateTransitionPublicInputs memory inputs =
            _validRootInputs(keccak256("unknown-previous-root"), 0, keccak256("state-1"));

        _assertRootSubmissionReverts(inputs, SybilSettlement.UnknownStateRoot.selector);
    }

    function testSettlementRejectsNonForwardHeight() public {
        bytes32 firstRoot = _acceptRootWithDeposit();
        SybilTypes.StateTransitionPublicInputs memory inputs =
            _validRootInputs(firstRoot, 1, keccak256("state-2"));
        inputs.newHeight = 1;

        _assertRootSubmissionReverts(inputs, SybilSettlement.NonMonotonicHeight.selector);
    }

    function testSettlementRejectsZeroAndDuplicateNewRoots() public {
        SybilTypes.StateTransitionPublicInputs memory zeroRootInputs =
            _validRootInputs(bytes32(0), 0, bytes32(0));
        _assertRootSubmissionReverts(zeroRootInputs, SybilSettlement.UnknownStateRoot.selector);

        bytes32 firstRoot = _acceptRootWithDeposit();
        SybilTypes.StateTransitionPublicInputs memory duplicateInputs =
            _validRootInputs(firstRoot, 1, firstRoot);
        _assertRootSubmissionReverts(duplicateInputs, SybilSettlement.RootAlreadyAccepted.selector);
    }

    function testSettlementRejectsUnavailableZeroDepositRoot() public {
        MockDepositRoots depositRoots = new MockDepositRoots();
        depositRoots.setDepositCount(1);
        SybilSettlement isolated = new SybilSettlement(address(this), verifier, ADMIN_TIMELOCK);
        isolated.setVault(depositRoots);
        SybilTypes.StateTransitionPublicInputs memory inputs =
            _rootInputs(bytes32(0), 0, 1, keccak256("state-1"), bytes32(0), 1);

        _assertRevertSelector(
            address(isolated),
            abi.encodeWithSelector(
                SybilSettlement.submitStateRoot.selector, inputs, bytes("proof")
            ),
            SybilSettlement.DepositRootMismatch.selector
        );
    }

    function testDepositFalseReturnLeavesCustodyAndAccumulatorUnchanged() public {
        uint256 ownerBalance = token.balanceOf(address(this));
        bytes32 initialRoot = vault.currentDepositRoot();
        token.setFailTransferFrom(true);

        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilVault.deposit.selector, 1_000_000, ACCOUNT_KEY),
            SybilVault.TransferFailed.selector
        );

        require(token.balanceOf(address(this)) == ownerBalance, "owner balance changed");
        require(token.balanceOf(address(vault)) == 0, "vault received collateral");
        require(vault.depositCount() == 0, "deposit count advanced");
        require(vault.currentDepositRoot() == initialRoot, "deposit root changed");
    }

    function testVaultRejectsZeroAmountDepositAndZeroVerifierRotations() public {
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilVault.deposit.selector, 0, ACCOUNT_KEY),
            SybilVault.AmountZero.selector
        );
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(
                SybilVault.proposeVerifier.selector, MockOpenVmVerifierAdapter(address(0))
            ),
            SybilAccessControl.ZeroAddress.selector
        );
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(
                SybilVault.setVerifier.selector, MockOpenVmVerifierAdapter(address(0))
            ),
            SybilAccessControl.ZeroAddress.selector
        );
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(
                SybilVault.proposeEscapeVerifier.selector, MockOpenVmVerifierAdapter(address(0))
            ),
            SybilAccessControl.ZeroAddress.selector
        );
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(
                SybilVault.setEscapeVerifier.selector, MockOpenVmVerifierAdapter(address(0))
            ),
            SybilAccessControl.ZeroAddress.selector
        );
    }

    function testVaultConstructorRejectsEveryZeroDependency() public {
        _assertVaultConstructionReverts(
            address(0), address(settlement), address(verifier), address(escapeVerifier)
        );
        _assertVaultConstructionReverts(
            address(token), address(0), address(verifier), address(escapeVerifier)
        );
        _assertVaultConstructionReverts(
            address(token), address(settlement), address(0), address(escapeVerifier)
        );
        _assertVaultConstructionReverts(
            address(token), address(settlement), address(verifier), address(0)
        );
    }

    function testWithdrawalRequestRejectsZeroAmount() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        SybilTypes.WithdrawalPublicInputs memory inputs =
            _withdrawalInputs(stateRoot, keccak256("zero-amount"));
        inputs.amount = 0;

        _assertWithdrawalRequestReverts(inputs, SybilVault.AmountZero.selector);
    }

    function testWithdrawalRequestRejectsUnsupportedToken() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        SybilTypes.WithdrawalPublicInputs memory inputs =
            _withdrawalInputs(stateRoot, keccak256("unsupported-token"));
        inputs.token = address(0xBAD);

        _assertWithdrawalRequestReverts(inputs, SybilVault.TokenUnsupported.selector);
    }

    function testWithdrawalRequestRejectsUnknownRoot() public {
        SybilTypes.WithdrawalPublicInputs memory inputs =
            _withdrawalInputs(keccak256("unknown-root"), keccak256("unknown-root-nullifier"));

        _assertWithdrawalRequestReverts(inputs, SybilVault.UnknownStateRoot.selector);
    }

    function testFinalizeRejectsUnknownCanceledAndFinalizedWithdrawals() public {
        bytes32 unknown = keccak256("unknown-withdrawal");
        _assertFinalizeReverts(unknown, SybilVault.UnknownWithdrawal.selector);

        SybilTypes.WithdrawalPublicInputs memory canceled =
            _queueWithdrawal(keccak256("canceled-withdrawal"));
        vault.cancelWithdrawal(canceled.nullifier, "cancel for terminal-state test");
        _assertFinalizeReverts(canceled.nullifier, SybilVault.WithdrawalCanceledError.selector);

        SybilTypes.WithdrawalPublicInputs memory finalized =
            _withdrawalInputs(canceled.stateRoot, keccak256("finalized-withdrawal"));
        vault.requestWithdrawal(finalized, "proof");
        vm.warp(block.timestamp + WITHDRAWAL_DELAY);
        vault.finalizeWithdrawal(finalized.nullifier);
        _assertFinalizeReverts(finalized.nullifier, SybilVault.WithdrawalFinalizedError.selector);
    }

    function testFinalizeFalseReturnRollsBackAndCanRetry() public {
        SybilTypes.WithdrawalPublicInputs memory inputs =
            _queueWithdrawal(keccak256("payout-false"));
        vm.warp(block.timestamp + WITHDRAWAL_DELAY);
        token.setFailTransfer(true);

        _assertFinalizeReverts(inputs.nullifier, SybilVault.TransferFailed.selector);
        (,,,,,,,, bool finalized,) = vault.withdrawals(inputs.nullifier);
        require(!finalized, "failed payout finalized withdrawal");

        token.setFailTransfer(false);
        vault.finalizeWithdrawal(inputs.nullifier);
        (,,,,,,,, finalized,) = vault.withdrawals(inputs.nullifier);
        require(finalized, "retry did not finalize withdrawal");
    }

    function testCancelRejectsUnknownCanceledAndFinalizedWithdrawals() public {
        bytes32 unknown = keccak256("unknown-cancel");
        _assertCancelReverts(unknown, SybilVault.UnknownWithdrawal.selector);

        SybilTypes.WithdrawalPublicInputs memory canceled =
            _queueWithdrawal(keccak256("already-canceled"));
        vault.cancelWithdrawal(canceled.nullifier, "first cancellation");
        _assertCancelReverts(canceled.nullifier, SybilVault.WithdrawalCanceledError.selector);

        SybilTypes.WithdrawalPublicInputs memory finalized =
            _withdrawalInputs(canceled.stateRoot, keccak256("cancel-finalized"));
        vault.requestWithdrawal(finalized, "proof");
        vm.warp(block.timestamp + WITHDRAWAL_DELAY);
        vault.finalizeWithdrawal(finalized.nullifier);
        _assertCancelReverts(finalized.nullifier, SybilVault.WithdrawalFinalizedError.selector);
    }

    function testNonAdminCannotControlMoneyPaths() public {
        address intruder = address(0xBEEF);

        vm.prank(intruder);
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilVault.pause.selector),
            SybilAccessControl.OnlyAdmin.selector
        );
        vm.prank(intruder);
        _assertRevertSelector(
            address(settlement),
            abi.encodeWithSelector(SybilSettlement.pause.selector),
            SybilAccessControl.OnlyAdmin.selector
        );
        vm.prank(intruder);
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(
                SybilVault.cancelWithdrawal.selector, keccak256("unknown"), "unauthorized"
            ),
            SybilAccessControl.OnlyAdmin.selector
        );
        vm.prank(intruder);
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilVault.proposeEscapeTimeout.selector, uint64(1 days)),
            SybilAccessControl.OnlyAdmin.selector
        );
    }

    function testEscapeActivationCannotReplay() public {
        _acceptRootWithDeposit();
        _activateEscapeMode();

        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilVault.activateEscapeMode.selector),
            SybilVault.EscapeModeAlreadyActive.selector
        );
    }

    function testEscapeTransferFalseRollsBackNullifierAndCanRetry() public {
        bytes32 stateRoot = _acceptRootWithDeposit();
        _activateEscapeMode();
        SybilTypes.EscapeClaimPublicInputs memory inputs =
            _escapeInputs(stateRoot, settlement.latestHeight(), 71, address(0xCAFE), 100_000);
        token.setFailTransfer(true);

        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilVault.escapeClaim.selector, inputs, bytes("proof")),
            SybilVault.TransferFailed.selector
        );
        require(!vault.nullifierUsed(inputs.nullifier), "failed escape burned nullifier");

        token.setFailTransfer(false);
        vault.escapeClaim(inputs, "proof");
        require(vault.nullifierUsed(inputs.nullifier), "escape retry did not consume nullifier");
        require(token.balanceOf(inputs.recipient) == inputs.amount, "escape retry did not pay");
    }

    function testSettlementVaultRotationRequiresTimelockAndCannotReplay() public {
        MockDepositRoots newVault = new MockDepositRoots();
        settlement.proposeVault(newVault);
        _assertRevertSelector(
            address(settlement),
            abi.encodeWithSelector(SybilSettlement.setVault.selector, newVault),
            SybilAccessControl.TimelockNotReady.selector
        );

        vm.warp(block.timestamp + ADMIN_TIMELOCK);
        settlement.setVault(newVault);
        require(address(settlement.vault()) == address(newVault), "vault not rotated");
        _assertRevertSelector(
            address(settlement),
            abi.encodeWithSelector(SybilSettlement.setVault.selector, newVault),
            SybilAccessControl.UnknownProposal.selector
        );
    }

    function testVaultVerifierRotationRequiresTimelockAndCannotReplay() public {
        MockOpenVmVerifierAdapter newVerifier = new MockOpenVmVerifierAdapter();
        vault.proposeVerifier(newVerifier);
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilVault.setVerifier.selector, newVerifier),
            SybilAccessControl.TimelockNotReady.selector
        );

        vm.warp(block.timestamp + ADMIN_TIMELOCK);
        vault.setVerifier(newVerifier);
        require(address(vault.verifier()) == address(newVerifier), "verifier not rotated");
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilVault.setVerifier.selector, newVerifier),
            SybilAccessControl.UnknownProposal.selector
        );
    }

    function testEscapeTimeoutMutationRejectsDuplicateProposalAndRequiresTimelock() public {
        uint64 newTimeout = 14 days;
        vault.proposeEscapeTimeout(newTimeout);
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilVault.proposeEscapeTimeout.selector, newTimeout),
            SybilAccessControl.ProposalAlreadyExists.selector
        );
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilVault.setEscapeTimeout.selector, newTimeout),
            SybilAccessControl.TimelockNotReady.selector
        );

        vm.warp(block.timestamp + ADMIN_TIMELOCK);
        vault.setEscapeTimeout(newTimeout);
        require(vault.escapeTimeout() == newTimeout, "escape timeout not updated");
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilVault.setEscapeTimeout.selector, newTimeout),
            SybilAccessControl.UnknownProposal.selector
        );
    }

    function testAdminActionDelayMutationControlsLaterProposals() public {
        uint64 newDelay = 3 days;
        vault.proposeAdminActionDelay(newDelay);
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilAccessControl.executeAdminActionDelay.selector, newDelay),
            SybilAccessControl.TimelockNotReady.selector
        );

        vm.warp(block.timestamp + ADMIN_TIMELOCK);
        vault.executeAdminActionDelay(newDelay);
        require(vault.adminActionDelay() == newDelay, "admin delay not updated");

        bytes32 proposal = vault.proposeWithdrawalDelay(4 days);
        (,, uint64 executableAt, bool exists) = vault.timelockProposals(proposal);
        require(exists, "later proposal missing");
        require(executableAt == block.timestamp + newDelay, "later proposal used old delay");
    }

    function testAccessControlRejectsUnknownZeroAndOverflowingAdminActions() public {
        bytes32 unknownProposal = keccak256("unknown-proposal");
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilAccessControl.cancelProposal.selector, unknownProposal),
            SybilAccessControl.UnknownProposal.selector
        );
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilAccessControl.proposeAdminTransfer.selector, address(0)),
            SybilAccessControl.ZeroAddress.selector
        );
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilAccessControl.executeAdminTransfer.selector, address(0)),
            SybilAccessControl.ZeroAddress.selector
        );

        vm.warp(type(uint64).max);
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilVault.proposeWithdrawalDelay.selector, uint64(4 days)),
            SybilAccessControl.TimestampOverflow.selector
        );
    }

    function _acceptRootWithDeposit() private returns (bytes32 stateRoot) {
        vault.deposit(1_000_000, ACCOUNT_KEY);
        stateRoot = keccak256("money-path-state-1");
        settlement.submitStateRoot(_validRootInputs(bytes32(0), 0, stateRoot), "proof");
    }

    function _activateEscapeMode() private {
        vm.warp(block.timestamp + ESCAPE_TIMEOUT + 1);
        vault.activateEscapeMode();
    }

    function _queueWithdrawal(
        bytes32 nullifier
    ) private returns (SybilTypes.WithdrawalPublicInputs memory inputs) {
        bytes32 stateRoot = _acceptRootWithDeposit();
        inputs = _withdrawalInputs(stateRoot, nullifier);
        vault.requestWithdrawal(inputs, "proof");
    }

    function _withdrawalInputs(
        bytes32 stateRoot,
        bytes32 nullifier
    ) private view returns (SybilTypes.WithdrawalPublicInputs memory) {
        return SybilTypes.WithdrawalPublicInputs({
            stateRoot: stateRoot,
            height: settlement.latestHeight(),
            nullifier: nullifier,
            recipient: address(0xCAFE),
            token: address(token),
            amount: 100_000,
            claimKind: vault.CLAIM_KIND_NORMAL()
        });
    }

    function _escapeInputs(
        bytes32 stateRoot,
        uint64 height,
        uint64 accountId,
        address recipient,
        uint256 amount
    ) private view returns (SybilTypes.EscapeClaimPublicInputs memory) {
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

    function _validRootInputs(
        bytes32 previousRoot,
        uint64 previousHeight,
        bytes32 newRoot
    ) private view returns (SybilTypes.StateTransitionPublicInputs memory) {
        return _rootInputs(
            previousRoot,
            previousHeight,
            previousHeight + 1,
            newRoot,
            vault.depositRootByCount(vault.depositCount()),
            vault.depositCount()
        );
    }

    function _rootInputs(
        bytes32 previousRoot,
        uint64 previousHeight,
        uint64 newHeight,
        bytes32 newRoot,
        bytes32 depositRoot,
        uint64 depositCount
    ) private pure returns (SybilTypes.StateTransitionPublicInputs memory) {
        return SybilTypes.StateTransitionPublicInputs({
            previousHeight: previousHeight,
            newHeight: newHeight,
            previousStateRoot: previousRoot,
            newStateRoot: newRoot,
            blockHash: BLOCK_HASH,
            eventsRoot: EVENTS_ROOT,
            witnessRoot: WITNESS_ROOT,
            daCommitment: DA_COMMITMENT,
            depositRoot: depositRoot,
            depositCount: depositCount
        });
    }

    function _assertRootSubmissionReverts(
        SybilTypes.StateTransitionPublicInputs memory inputs,
        bytes4 expectedSelector
    ) private {
        _assertRevertSelector(
            address(settlement),
            abi.encodeWithSelector(
                SybilSettlement.submitStateRoot.selector, inputs, bytes("proof")
            ),
            expectedSelector
        );
    }

    function _assertVaultConstructionReverts(
        address tokenAddress,
        address settlementAddress,
        address verifierAddress,
        address escapeVerifierAddress
    ) private {
        try new SybilVault(
            address(this),
            MockUSDC(tokenAddress),
            SybilSettlement(settlementAddress),
            MockOpenVmVerifierAdapter(verifierAddress),
            MockOpenVmVerifierAdapter(escapeVerifierAddress),
            WITHDRAWAL_DELAY,
            ESCAPE_TIMEOUT,
            ADMIN_TIMELOCK
        ) returns (
            SybilVault unexpected
        ) {
            unexpected;
            revert("zero-dependency vault deployed");
        } catch (bytes memory revertData) {
            _assertRevertDataSelector(revertData, SybilAccessControl.ZeroAddress.selector);
        }
    }

    function _assertWithdrawalRequestReverts(
        SybilTypes.WithdrawalPublicInputs memory inputs,
        bytes4 expectedSelector
    ) private {
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilVault.requestWithdrawal.selector, inputs, bytes("proof")),
            expectedSelector
        );
        require(!vault.nullifierUsed(inputs.nullifier), "rejected request burned nullifier");
    }

    function _assertFinalizeReverts(
        bytes32 nullifier,
        bytes4 expectedSelector
    ) private {
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilVault.finalizeWithdrawal.selector, nullifier),
            expectedSelector
        );
    }

    function _assertCancelReverts(
        bytes32 nullifier,
        bytes4 expectedSelector
    ) private {
        _assertRevertSelector(
            address(vault),
            abi.encodeWithSelector(SybilVault.cancelWithdrawal.selector, nullifier, "retry"),
            expectedSelector
        );
    }

    function _assertRevertSelector(
        address target,
        bytes memory callData,
        bytes4 expectedSelector
    ) private {
        (bool ok, bytes memory revertData) = target.call(callData);
        require(!ok, "call unexpectedly succeeded");
        _assertRevertDataSelector(revertData, expectedSelector);
    }

    function _assertRevertDataSelector(
        bytes memory revertData,
        bytes4 expectedSelector
    ) private pure {
        require(revertData.length >= 4, "revert data missing selector");
        bytes4 actualSelector;
        assembly {
            actualSelector := mload(add(revertData, 32))
        }
        require(actualSelector == expectedSelector, "unexpected revert selector");
    }
}
