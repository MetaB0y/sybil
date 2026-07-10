// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

abstract contract SybilAccessControl {
    address public admin;
    uint64 public adminActionDelay;

    struct TimelockProposal {
        bytes32 operation;
        bytes32 dataHash;
        uint64 executableAt;
        bool exists;
    }

    mapping(bytes32 proposalId => TimelockProposal proposal) public timelockProposals;

    bytes32 public constant OP_ADMIN_TRANSFER = keccak256("sybil/admin/transfer/v1");
    bytes32 public constant OP_ADMIN_ACTION_DELAY = keccak256("sybil/admin/action-delay/v1");

    event TimelockProposed(
        bytes32 indexed proposalId,
        bytes32 indexed operation,
        bytes32 indexed dataHash,
        uint64 executableAt
    );
    event TimelockExecuted(
        bytes32 indexed proposalId, bytes32 indexed operation, bytes32 indexed dataHash
    );
    event TimelockCanceled(
        bytes32 indexed proposalId, bytes32 indexed operation, bytes32 indexed dataHash
    );
    event AdminTransferred(address indexed oldAdmin, address indexed newAdmin);
    event AdminActionDelayUpdated(uint64 oldDelay, uint64 newDelay);

    error OnlyAdmin(address account);
    error ZeroAddress();
    error ProposalAlreadyExists(bytes32 proposalId);
    error UnknownProposal(bytes32 proposalId);
    error TimelockNotReady(bytes32 proposalId, uint64 executableAt);
    error TimestampOverflow();

    modifier onlyAdmin() {
        if (msg.sender != admin) revert OnlyAdmin(msg.sender);
        _;
    }

    constructor(
        address initialAdmin,
        uint64 initialAdminActionDelay
    ) {
        if (initialAdmin == address(0)) revert ZeroAddress();
        admin = initialAdmin;
        adminActionDelay = initialAdminActionDelay;
        emit AdminTransferred(address(0), initialAdmin);
        emit AdminActionDelayUpdated(0, initialAdminActionDelay);
    }

    function proposalId(
        bytes32 operation,
        bytes calldata data
    ) external view returns (bytes32) {
        return _proposalId(operation, data);
    }

    function cancelProposal(
        bytes32 id
    ) external onlyAdmin {
        TimelockProposal memory proposal = timelockProposals[id];
        if (!proposal.exists) revert UnknownProposal(id);
        delete timelockProposals[id];
        emit TimelockCanceled(id, proposal.operation, proposal.dataHash);
    }

    function proposeAdminTransfer(
        address newAdmin
    ) external onlyAdmin returns (bytes32 id) {
        if (newAdmin == address(0)) revert ZeroAddress();
        return _proposeTimelock(OP_ADMIN_TRANSFER, abi.encode(newAdmin));
    }

    function executeAdminTransfer(
        address newAdmin
    ) external onlyAdmin {
        if (newAdmin == address(0)) revert ZeroAddress();
        _consumeTimelock(OP_ADMIN_TRANSFER, abi.encode(newAdmin));
        address oldAdmin = admin;
        admin = newAdmin;
        emit AdminTransferred(oldAdmin, newAdmin);
    }

    function proposeAdminActionDelay(
        uint64 newDelay
    ) external onlyAdmin returns (bytes32 id) {
        return _proposeTimelock(OP_ADMIN_ACTION_DELAY, abi.encode(newDelay));
    }

    function executeAdminActionDelay(
        uint64 newDelay
    ) external onlyAdmin {
        _consumeTimelock(OP_ADMIN_ACTION_DELAY, abi.encode(newDelay));
        uint64 oldDelay = adminActionDelay;
        adminActionDelay = newDelay;
        emit AdminActionDelayUpdated(oldDelay, newDelay);
    }

    function _proposeTimelock(
        bytes32 operation,
        bytes memory data
    ) internal returns (bytes32 id) {
        id = _proposalId(operation, data);
        if (timelockProposals[id].exists) revert ProposalAlreadyExists(id);

        uint256 executableAtU256 = block.timestamp + adminActionDelay;
        if (executableAtU256 > type(uint64).max) revert TimestampOverflow();
        uint64 executableAt = uint64(executableAtU256);

        bytes32 dataHash = keccak256(data);
        timelockProposals[id] = TimelockProposal({
            operation: operation, dataHash: dataHash, executableAt: executableAt, exists: true
        });
        emit TimelockProposed(id, operation, dataHash, executableAt);
    }

    function _consumeTimelock(
        bytes32 operation,
        bytes memory data
    ) internal returns (bytes32 id) {
        id = _proposalId(operation, data);
        TimelockProposal memory proposal = timelockProposals[id];
        if (!proposal.exists) revert UnknownProposal(id);
        if (block.timestamp < proposal.executableAt) {
            revert TimelockNotReady(id, proposal.executableAt);
        }
        delete timelockProposals[id];
        emit TimelockExecuted(id, operation, proposal.dataHash);
    }

    function _proposalId(
        bytes32 operation,
        bytes memory data
    ) internal view returns (bytes32) {
        return keccak256(abi.encode(address(this), operation, keccak256(data)));
    }
}
