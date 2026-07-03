// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {SybilAccessControl} from "./access/SybilAccessControl.sol";
import {IERC20Minimal} from "./interfaces/IERC20Minimal.sol";
import {IOpenVmVerifierAdapter} from "./interfaces/IOpenVmVerifierAdapter.sol";
import {ISybilSettlement} from "./interfaces/ISybilSettlement.sol";
import {ISybilVaultDepositRoots} from "./interfaces/ISybilVaultDepositRoots.sol";
import {SybilTypes} from "./SybilTypes.sol";

contract SybilVault is SybilAccessControl, ISybilVaultDepositRoots {
    uint256 public constant NANOS_PER_TOKEN_UNIT = 1000;
    uint8 public constant DEPOSIT_TREE_DEPTH = 32;

    bytes32 public constant PAUSER_ROLE = keccak256("SYBIL_PAUSER_ROLE");
    bytes32 public constant PARAMETER_ADMIN_ROLE = keccak256("SYBIL_PARAMETER_ADMIN_ROLE");
    bytes32 public constant GUARDIAN_ROLE = keccak256("SYBIL_GUARDIAN_ROLE");
    bytes32 public constant CLAIM_KIND_NORMAL = keccak256("sybil/claim-kind/normal-withdrawal/v1");
    // NOTE: The emergency escape-cash claim is unimplemented. It requires a
    // distinct ZK guest program (proving `acct`/`acct_resv` membership against
    // the latest accepted root and computing conservative withdrawable cash),
    // which is a different public-input shape and app commitment than the
    // single guest this vault's verifier adapter is pinned to. Until that guest
    // and a claimKind-dispatched verifier exist, `requestWithdrawal` fails
    // closed on any non-normal claim kind (see UnsupportedClaimKind below). The
    // former `CLAIM_KIND_ESCAPE` constant advertised a mechanism that does not
    // exist and has been removed; see "Emergency escape" in the L1 Settlement
    // and Vault design note for the unimplemented-mechanism record.

    IERC20Minimal public immutable token;
    ISybilSettlement public immutable settlement;
    uint64 public immutable deployedAt;
    IOpenVmVerifierAdapter public verifier;

    uint64 public withdrawalDelay;
    uint64 public escapeTimeout;
    bool public depositsPaused;
    bool public withdrawalRequestsPaused;
    bool public withdrawalFinalizationPaused;
    bool public escapeModeActive;
    uint64 public escapeModeActivatedAt;

    uint64 public depositCount;
    bytes32 public currentDepositRoot;

    mapping(uint64 count => bytes32 root) public depositRootByCount;
    mapping(bytes32 nullifier => bool used) public nullifierUsed;
    mapping(bytes32 nullifier => QueuedWithdrawal withdrawal) public withdrawals;

    bytes32[DEPOSIT_TREE_DEPTH] public filledSubtrees;
    bytes32[DEPOSIT_TREE_DEPTH + 1] public zeroHashes;

    struct QueuedWithdrawal {
        address recipient;
        address token;
        uint256 amount;
        bytes32 nullifier;
        bytes32 stateRoot;
        uint64 height;
        uint64 requestedAt;
        uint64 executableAt;
        bool finalized;
        bool canceled;
    }

    event DepositReceived(
        uint64 indexed depositId,
        address indexed sender,
        bytes32 indexed sybilAccountKey,
        address token,
        uint256 amount,
        bytes32 depositRoot
    );
    event WithdrawalRequested(
        bytes32 indexed nullifier,
        address indexed recipient,
        address token,
        uint256 amount,
        bytes32 stateRoot,
        uint64 height,
        uint64 executableAt
    );
    event WithdrawalFinalized(bytes32 indexed nullifier, address indexed recipient, uint256 amount);
    event WithdrawalCanceled(bytes32 indexed nullifier, string reason);
    event EscapeModeActivated(uint64 indexed height, bytes32 indexed stateRoot, uint64 activatedAt);
    event ParameterUpdated(bytes32 indexed key, uint256 oldValue, uint256 newValue);
    event DepositsPaused(bool paused);
    event WithdrawalRequestsPaused(bool paused);
    event WithdrawalFinalizationPaused(bool paused);

    error InvalidProof();
    error UnsupportedClaimKind(bytes32 claimKind);
    error UnknownStateRoot(bytes32 stateRoot);
    error WithdrawalAlreadyUsed(bytes32 nullifier);
    error WithdrawalNotReady(bytes32 nullifier, uint64 executableAt);
    error WithdrawalCanceledError(bytes32 nullifier);
    error WithdrawalFinalizedError(bytes32 nullifier);
    error EscapeModeInactive();
    error EscapeModeAlreadyActive();
    error AmountZero();
    error TokenUnsupported(address token);
    error DepositsPausedError();
    error WithdrawalRequestsPausedError();
    error WithdrawalFinalizationPausedError();
    error TransferFailed();
    error UnknownWithdrawal(bytes32 nullifier);
    error NotPaused();

    constructor(
        address admin,
        IERC20Minimal collateralToken,
        ISybilSettlement settlementContract,
        IOpenVmVerifierAdapter verifierAdapter,
        uint64 initialWithdrawalDelay,
        uint64 initialEscapeTimeout
    ) SybilAccessControl(admin) {
        if (address(collateralToken) == address(0)) revert ZeroAddress();
        if (address(settlementContract) == address(0)) revert ZeroAddress();
        if (address(verifierAdapter) == address(0)) revert ZeroAddress();

        token = collateralToken;
        settlement = settlementContract;
        deployedAt = uint64(block.timestamp);
        verifier = verifierAdapter;
        withdrawalDelay = initialWithdrawalDelay;
        escapeTimeout = initialEscapeTimeout;

        zeroHashes[0] = bytes32(0);
        for (uint8 level = 0; level < DEPOSIT_TREE_DEPTH; level++) {
            zeroHashes[level + 1] = hashNode(zeroHashes[level], zeroHashes[level]);
        }
        currentDepositRoot = zeroHashes[DEPOSIT_TREE_DEPTH];
        depositRootByCount[0] = currentDepositRoot;

        _grantRole(PAUSER_ROLE, admin);
        _grantRole(PARAMETER_ADMIN_ROLE, admin);
        _grantRole(GUARDIAN_ROLE, admin);
    }

    function deposit(
        uint256 amount,
        bytes32 sybilAccountKey
    ) external {
        if (depositsPaused) revert DepositsPausedError();
        if (amount == 0) revert AmountZero();

        if (!token.transferFrom(msg.sender, address(this), amount)) revert TransferFailed();

        uint64 depositId = depositCount + 1;
        bytes32 depositLeafHash =
            hashDepositLeaf(depositLeaf(depositId, msg.sender, sybilAccountKey, amount));
        currentDepositRoot = _appendDepositLeaf(depositId, depositLeafHash);
        depositCount = depositId;
        depositRootByCount[depositId] = currentDepositRoot;

        emit DepositReceived(
            depositId, msg.sender, sybilAccountKey, address(token), amount, currentDepositRoot
        );
    }

    function requestWithdrawal(
        SybilTypes.WithdrawalPublicInputs calldata inputs,
        bytes calldata proof
    ) external returns (bytes32 nullifier) {
        if (withdrawalRequestsPaused) revert WithdrawalRequestsPausedError();
        // OL-3: this entrypoint serves normal withdrawal-leaf claims only. The
        // claimKind is bound into the proof public-input hash, so accepting a
        // non-normal kind here would advertise the unimplemented escape-cash
        // path. Fail closed until a dedicated escape entrypoint exists.
        if (inputs.claimKind != CLAIM_KIND_NORMAL) revert UnsupportedClaimKind(inputs.claimKind);
        if (inputs.amount == 0) revert AmountZero();
        if (inputs.token != address(token)) revert TokenUnsupported(inputs.token);
        if (!settlement.isAcceptedRoot(inputs.stateRoot)) {
            revert UnknownStateRoot(inputs.stateRoot);
        }
        if (nullifierUsed[inputs.nullifier]) revert WithdrawalAlreadyUsed(inputs.nullifier);

        bytes32 inputHash = withdrawalPublicInputHash(inputs);
        if (!verifier.verify(proof, inputHash)) revert InvalidProof();

        nullifierUsed[inputs.nullifier] = true;
        uint64 requestedAt = uint64(block.timestamp);
        uint64 executableAt = requestedAt + withdrawalDelay;

        withdrawals[inputs.nullifier] = QueuedWithdrawal({
            recipient: inputs.recipient,
            token: inputs.token,
            amount: inputs.amount,
            nullifier: inputs.nullifier,
            stateRoot: inputs.stateRoot,
            height: inputs.height,
            requestedAt: requestedAt,
            executableAt: executableAt,
            finalized: false,
            canceled: false
        });

        emit WithdrawalRequested(
            inputs.nullifier,
            inputs.recipient,
            inputs.token,
            inputs.amount,
            inputs.stateRoot,
            inputs.height,
            executableAt
        );
        return inputs.nullifier;
    }

    function finalizeWithdrawal(
        bytes32 nullifier
    ) external {
        if (withdrawalFinalizationPaused) revert WithdrawalFinalizationPausedError();

        QueuedWithdrawal storage queued = withdrawals[nullifier];
        if (queued.nullifier == bytes32(0)) revert UnknownWithdrawal(nullifier);
        if (queued.canceled) revert WithdrawalCanceledError(nullifier);
        if (queued.finalized) revert WithdrawalFinalizedError(nullifier);
        if (block.timestamp < queued.executableAt) {
            revert WithdrawalNotReady(nullifier, queued.executableAt);
        }

        queued.finalized = true;
        if (!token.transfer(queued.recipient, queued.amount)) revert TransferFailed();
        emit WithdrawalFinalized(nullifier, queued.recipient, queued.amount);
    }

    function cancelWithdrawal(
        bytes32 nullifier,
        string calldata reason
    ) external onlyRole(GUARDIAN_ROLE) {
        if (!withdrawalRequestsPaused && !withdrawalFinalizationPaused) revert NotPaused();
        QueuedWithdrawal storage queued = withdrawals[nullifier];
        if (queued.nullifier == bytes32(0)) revert UnknownWithdrawal(nullifier);
        if (queued.finalized) revert WithdrawalFinalizedError(nullifier);
        queued.canceled = true;
        emit WithdrawalCanceled(nullifier, reason);
    }

    function activateEscapeMode() external {
        if (escapeModeActive) revert EscapeModeAlreadyActive();
        // Escape mode signals operator disappearance. Liveness is measured from
        // the last accepted root, but before any root is accepted there is no
        // verifiedAt to measure from. Falling back to the vault deployment time
        // means deposits made before the operator ever produced a first root
        // are not trapped: if no root arrives within escapeTimeout of
        // deployment, the mode still becomes activatable so governance and
        // timeout-driven recovery paths can proceed.
        uint64 latestVerifiedAt = settlement.latestRootVerifiedAt();
        uint64 livenessReference = latestVerifiedAt == 0 ? deployedAt : latestVerifiedAt;
        if (block.timestamp <= livenessReference + escapeTimeout) {
            revert EscapeModeInactive();
        }
        escapeModeActive = true;
        escapeModeActivatedAt = uint64(block.timestamp);
        emit EscapeModeActivated(
            settlement.latestHeight(), settlement.latestStateRoot(), escapeModeActivatedAt
        );
    }

    function setDepositsPaused(
        bool paused
    ) external onlyRole(PAUSER_ROLE) {
        depositsPaused = paused;
        emit DepositsPaused(paused);
    }

    function setWithdrawalRequestsPaused(
        bool paused
    ) external onlyRole(PAUSER_ROLE) {
        withdrawalRequestsPaused = paused;
        emit WithdrawalRequestsPaused(paused);
    }

    function setWithdrawalFinalizationPaused(
        bool paused
    ) external onlyRole(PAUSER_ROLE) {
        withdrawalFinalizationPaused = paused;
        emit WithdrawalFinalizationPaused(paused);
    }

    function setWithdrawalDelay(
        uint64 newDelay
    ) external onlyRole(PARAMETER_ADMIN_ROLE) {
        uint64 oldDelay = withdrawalDelay;
        withdrawalDelay = newDelay;
        emit ParameterUpdated("withdrawalDelay", oldDelay, newDelay);
    }

    function setEscapeTimeout(
        uint64 newTimeout
    ) external onlyRole(PARAMETER_ADMIN_ROLE) {
        uint64 oldTimeout = escapeTimeout;
        escapeTimeout = newTimeout;
        emit ParameterUpdated("escapeTimeout", oldTimeout, newTimeout);
    }

    function depositLeaf(
        uint64 depositId,
        address sender,
        bytes32 sybilAccountKey,
        uint256 amount
    ) public view returns (bytes32) {
        return keccak256(
            abi.encode(
                "sybil/l1-deposit/v1",
                block.chainid,
                address(this),
                depositId,
                address(token),
                sender,
                sybilAccountKey,
                amount
            )
        );
    }

    function hashDepositLeaf(
        bytes32 leaf
    ) public pure returns (bytes32) {
        return keccak256(abi.encodePacked(bytes1(0x00), leaf));
    }

    function hashNode(
        bytes32 left,
        bytes32 right
    ) public pure returns (bytes32) {
        return keccak256(abi.encodePacked(bytes1(0x01), left, right));
    }

    function withdrawalPublicInputHash(
        SybilTypes.WithdrawalPublicInputs memory inputs
    ) public pure returns (bytes32) {
        return keccak256(
            abi.encode(
                "sybil/openvm/withdrawal/v1",
                inputs.stateRoot,
                inputs.height,
                inputs.nullifier,
                inputs.recipient,
                inputs.token,
                inputs.amount,
                inputs.claimKind
            )
        );
    }

    function _appendDepositLeaf(
        uint64 depositId,
        bytes32 leaf
    ) internal returns (bytes32 root) {
        uint256 index = uint256(depositId - 1);
        root = leaf;
        for (uint8 level = 0; level < DEPOSIT_TREE_DEPTH; level++) {
            if (index & 1 == 0) {
                filledSubtrees[level] = root;
                root = hashNode(root, zeroHashes[level]);
            } else {
                root = hashNode(filledSubtrees[level], root);
            }
            index >>= 1;
        }
    }
}
