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

    bytes32 public constant OP_SET_VERIFIER = keccak256("sybil/vault/set-verifier/v1");
    bytes32 public constant OP_SET_ESCAPE_VERIFIER =
        keccak256("sybil/vault/set-escape-verifier/v1");
    bytes32 public constant OP_SET_WITHDRAWAL_DELAY =
        keccak256("sybil/vault/set-withdrawal-delay/v1");
    bytes32 public constant OP_SET_ESCAPE_TIMEOUT = keccak256("sybil/vault/set-escape-timeout/v1");
    bytes32 public constant CLAIM_KIND_NORMAL = keccak256("sybil/claim-kind/normal-withdrawal/v1");

    IERC20Minimal public immutable token;
    ISybilSettlement public immutable settlement;
    uint64 public immutable deployedAt;
    IOpenVmVerifierAdapter public verifier;
    IOpenVmVerifierAdapter public escapeVerifier;

    uint64 public withdrawalDelay;
    uint64 public escapeTimeout;
    bool public paused;
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
    event WithdrawalQueued(
        bytes32 indexed nullifier,
        address indexed recipient,
        address token,
        uint256 amount,
        bytes32 stateRoot,
        uint64 height,
        uint64 requestedAt,
        uint64 executableAt
    );
    event WithdrawalFinalized(
        bytes32 indexed nullifier,
        address indexed recipient,
        uint256 amount,
        uint64 finalizedAt,
        uint64 executableAt
    );
    event WithdrawalCancelled(
        bytes32 indexed nullifier,
        address indexed recipient,
        uint256 amount,
        uint64 cancelledAt,
        uint64 executableAt,
        string reason
    );
    event EscapeModeActivated(uint64 indexed height, bytes32 indexed stateRoot, uint64 activatedAt);
    event EscapeClaimed(
        uint64 indexed accountId,
        address indexed recipient,
        uint256 amount,
        bytes32 stateRoot,
        bytes32 indexed nullifier
    );
    event ParameterUpdated(bytes32 indexed key, uint256 oldValue, uint256 newValue);
    event VerifierUpdated(address indexed verifier);
    event EscapeVerifierUpdated(address indexed verifier);
    event Paused(address indexed admin);
    event Unpaused(address indexed admin);

    error InvalidProof();
    error UnsupportedClaimKind(bytes32 claimKind);
    error UnknownStateRoot(bytes32 stateRoot);
    error WithdrawalAlreadyUsed(bytes32 nullifier);
    error WithdrawalNotReady(bytes32 nullifier, uint64 executableAt);
    error WithdrawalCanceledError(bytes32 nullifier);
    error WithdrawalFinalizedError(bytes32 nullifier);
    error EscapeModeInactive();
    error EscapeModeAlreadyActive();
    error EscapeClaimStaleRoot(bytes32 providedRoot, bytes32 latestRoot);
    error EscapeClaimHeightMismatch(uint64 providedHeight, uint64 latestHeight);
    error EscapeNullifierMismatch(bytes32 providedNullifier, bytes32 expectedNullifier);
    error EscapeNullifierAlreadyUsed(bytes32 nullifier);
    error AmountZero();
    error TokenUnsupported(address token);
    error ContractPaused();
    error TransferFailed();
    error UnknownWithdrawal(bytes32 nullifier);
    error WithdrawalCancelWindowElapsed(bytes32 nullifier, uint64 executableAt);

    constructor(
        address admin,
        IERC20Minimal collateralToken,
        ISybilSettlement settlementContract,
        IOpenVmVerifierAdapter verifierAdapter,
        IOpenVmVerifierAdapter escapeVerifierAdapter,
        uint64 initialWithdrawalDelay,
        uint64 initialEscapeTimeout,
        uint64 initialAdminActionDelay
    ) SybilAccessControl(admin, initialAdminActionDelay) {
        if (address(collateralToken) == address(0)) revert ZeroAddress();
        if (address(settlementContract) == address(0)) revert ZeroAddress();
        if (address(verifierAdapter) == address(0)) revert ZeroAddress();
        if (address(escapeVerifierAdapter) == address(0)) revert ZeroAddress();

        token = collateralToken;
        settlement = settlementContract;
        deployedAt = uint64(block.timestamp);
        verifier = verifierAdapter;
        escapeVerifier = escapeVerifierAdapter;
        withdrawalDelay = initialWithdrawalDelay;
        escapeTimeout = initialEscapeTimeout;

        zeroHashes[0] = bytes32(0);
        for (uint8 level = 0; level < DEPOSIT_TREE_DEPTH; level++) {
            zeroHashes[level + 1] = hashNode(zeroHashes[level], zeroHashes[level]);
        }
        currentDepositRoot = zeroHashes[DEPOSIT_TREE_DEPTH];
        depositRootByCount[0] = currentDepositRoot;

        emit VerifierUpdated(address(verifierAdapter));
        emit EscapeVerifierUpdated(address(escapeVerifierAdapter));
    }

    function deposit(
        uint256 amount,
        bytes32 sybilAccountKey
    ) external {
        if (paused) revert ContractPaused();
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
        if (paused) revert ContractPaused();
        // OL-3: this entrypoint serves normal withdrawal-leaf claims only. The
        // claimKind is bound into the proof public-input hash, so accepting a
        // non-normal kind here could dispatch it through the wrong verifier.
        // Escape claims use the dedicated entrypoint and domain below.
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

        emit WithdrawalQueued(
            inputs.nullifier,
            inputs.recipient,
            inputs.token,
            inputs.amount,
            inputs.stateRoot,
            inputs.height,
            requestedAt,
            executableAt
        );
        return inputs.nullifier;
    }

    function finalizeWithdrawal(
        bytes32 nullifier
    ) external {
        if (paused) revert ContractPaused();

        QueuedWithdrawal storage queued = withdrawals[nullifier];
        if (queued.nullifier == bytes32(0)) revert UnknownWithdrawal(nullifier);
        if (queued.canceled) revert WithdrawalCanceledError(nullifier);
        if (queued.finalized) revert WithdrawalFinalizedError(nullifier);
        if (block.timestamp < queued.executableAt) {
            revert WithdrawalNotReady(nullifier, queued.executableAt);
        }

        queued.finalized = true;
        if (!token.transfer(queued.recipient, queued.amount)) revert TransferFailed();
        emit WithdrawalFinalized(
            nullifier, queued.recipient, queued.amount, uint64(block.timestamp), queued.executableAt
        );
    }

    function escapeClaim(
        SybilTypes.EscapeClaimPublicInputs calldata inputs,
        bytes calldata proof
    ) external {
        if (!escapeModeActive) revert EscapeModeInactive();

        // SYB-96 PAUSE EXCEPTION: escape deliberately bypasses BOTH `paused`
        // and the normal withdrawal delay. A future pause refactor must not
        // silently re-gate this operator-disappearance recovery path.
        bytes32 latestRoot = settlement.latestStateRoot();
        if (inputs.stateRoot != latestRoot) {
            revert EscapeClaimStaleRoot(inputs.stateRoot, latestRoot);
        }
        uint64 latestHeight = settlement.latestHeight();
        if (inputs.height != latestHeight) {
            revert EscapeClaimHeightMismatch(inputs.height, latestHeight);
        }

        bytes32 expectedNullifier = keccak256(
            abi.encode(
                "sybil/escape-nullifier/v1",
                block.chainid,
                address(this),
                inputs.accountId,
                inputs.stateRoot
            )
        );
        if (inputs.nullifier != expectedNullifier) {
            revert EscapeNullifierMismatch(inputs.nullifier, expectedNullifier);
        }
        if (nullifierUsed[expectedNullifier]) {
            revert EscapeNullifierAlreadyUsed(expectedNullifier);
        }
        nullifierUsed[expectedNullifier] = true;

        bytes32 inputHash = escapeClaimPublicInputHash(inputs);
        if (!escapeVerifier.verify(proof, inputHash)) revert InvalidProof();

        if (!token.transfer(inputs.recipient, inputs.amount)) revert TransferFailed();
        emit EscapeClaimed(
            inputs.accountId, inputs.recipient, inputs.amount, inputs.stateRoot, expectedNullifier
        );
    }

    function cancelWithdrawal(
        bytes32 nullifier,
        string calldata reason
    ) external onlyAdmin {
        QueuedWithdrawal storage queued = withdrawals[nullifier];
        if (queued.nullifier == bytes32(0)) revert UnknownWithdrawal(nullifier);
        if (queued.finalized) revert WithdrawalFinalizedError(nullifier);
        if (queued.canceled) revert WithdrawalCanceledError(nullifier);
        if (block.timestamp >= queued.executableAt) {
            revert WithdrawalCancelWindowElapsed(nullifier, queued.executableAt);
        }
        queued.canceled = true;
        nullifierUsed[nullifier] = false;
        emit WithdrawalCancelled(
            nullifier,
            queued.recipient,
            queued.amount,
            uint64(block.timestamp),
            queued.executableAt,
            reason
        );
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
        emit VerifierUpdated(address(newVerifier));
    }

    function proposeEscapeVerifier(
        IOpenVmVerifierAdapter newEscapeVerifier
    ) external onlyAdmin returns (bytes32 id) {
        if (address(newEscapeVerifier) == address(0)) revert ZeroAddress();
        return _proposeTimelock(OP_SET_ESCAPE_VERIFIER, abi.encode(newEscapeVerifier));
    }

    function setEscapeVerifier(
        IOpenVmVerifierAdapter newEscapeVerifier
    ) external onlyAdmin {
        if (address(newEscapeVerifier) == address(0)) revert ZeroAddress();
        _consumeTimelock(OP_SET_ESCAPE_VERIFIER, abi.encode(newEscapeVerifier));
        escapeVerifier = newEscapeVerifier;
        emit EscapeVerifierUpdated(address(newEscapeVerifier));
    }

    function proposeWithdrawalDelay(
        uint64 newDelay
    ) external onlyAdmin returns (bytes32 id) {
        return _proposeTimelock(OP_SET_WITHDRAWAL_DELAY, abi.encode(newDelay));
    }

    function setWithdrawalDelay(
        uint64 newDelay
    ) external onlyAdmin {
        _consumeTimelock(OP_SET_WITHDRAWAL_DELAY, abi.encode(newDelay));
        uint64 oldDelay = withdrawalDelay;
        withdrawalDelay = newDelay;
        emit ParameterUpdated("withdrawalDelay", oldDelay, newDelay);
    }

    function proposeEscapeTimeout(
        uint64 newTimeout
    ) external onlyAdmin returns (bytes32 id) {
        return _proposeTimelock(OP_SET_ESCAPE_TIMEOUT, abi.encode(newTimeout));
    }

    function setEscapeTimeout(
        uint64 newTimeout
    ) external onlyAdmin {
        _consumeTimelock(OP_SET_ESCAPE_TIMEOUT, abi.encode(newTimeout));
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

    function escapeClaimPublicInputHash(
        SybilTypes.EscapeClaimPublicInputs memory inputs
    ) public pure returns (bytes32) {
        return keccak256(
            abi.encode(
                "sybil/openvm/escape-claim/v1",
                inputs.stateRoot,
                inputs.height,
                inputs.accountId,
                inputs.recipient,
                inputs.amount,
                inputs.nullifier
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
