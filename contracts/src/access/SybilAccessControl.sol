// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

abstract contract SybilAccessControl {
    bytes32 public constant DEFAULT_ADMIN_ROLE = 0x00;

    mapping(bytes32 role => mapping(address account => bool hasRole)) private _roles;

    event RoleGranted(bytes32 indexed role, address indexed account, address indexed sender);
    event RoleRevoked(bytes32 indexed role, address indexed account, address indexed sender);

    error MissingRole(bytes32 role, address account);
    error ZeroAddress();

    modifier onlyRole(
        bytes32 role
    ) {
        _checkRole(role, msg.sender);
        _;
    }

    constructor(
        address admin
    ) {
        if (admin == address(0)) revert ZeroAddress();
        _grantRole(DEFAULT_ADMIN_ROLE, admin);
    }

    function hasRole(
        bytes32 role,
        address account
    ) public view returns (bool) {
        return _roles[role][account];
    }

    function grantRole(
        bytes32 role,
        address account
    ) external onlyRole(DEFAULT_ADMIN_ROLE) {
        if (account == address(0)) revert ZeroAddress();
        _grantRole(role, account);
    }

    function revokeRole(
        bytes32 role,
        address account
    ) external onlyRole(DEFAULT_ADMIN_ROLE) {
        if (_roles[role][account]) {
            _roles[role][account] = false;
            emit RoleRevoked(role, account, msg.sender);
        }
    }

    function _checkRole(
        bytes32 role,
        address account
    ) internal view {
        if (!_roles[role][account]) revert MissingRole(role, account);
    }

    function _grantRole(
        bytes32 role,
        address account
    ) internal {
        if (!_roles[role][account]) {
            _roles[role][account] = true;
            emit RoleGranted(role, account, msg.sender);
        }
    }
}
