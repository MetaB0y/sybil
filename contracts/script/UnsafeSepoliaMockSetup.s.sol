// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {SybilSettlement} from "../src/SybilSettlement.sol";
import {SybilVault} from "../src/SybilVault.sol";
import {MintableMockUSDC} from "../src/dev/MintableMockUSDC.sol";
import {UnsafeSepoliaMockVerifierAdapter} from "../src/dev/UnsafeSepoliaMockVerifierAdapter.sol";

interface SepoliaMockSetupVm {
    function addr(
        uint256 privateKey
    ) external returns (address);

    function envUint(
        string calldata name
    ) external view returns (uint256);

    function startBroadcast(
        uint256 privateKey
    ) external;

    function stopBroadcast() external;
}

/// @dev Deploys a conspicuously unsafe, valueless Sepolia bridge fixture. The
/// shell wrapper performs a second chain-id check and emits a validated manifest.
contract UnsafeSepoliaMockSetup {
    SepoliaMockSetupVm private constant vm =
        SepoliaMockSetupVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    uint256 private constant SEPOLIA_CHAIN_ID = 11_155_111;
    uint64 private constant WITHDRAWAL_DELAY = 1 hours;
    uint64 private constant ADMIN_TIMELOCK = 2 days;
    uint64 private constant ESCAPE_TIMEOUT = 7 days;
    uint256 private constant DEPLOYER_MOCK_FUNDING = 1_000_000 * 1_000_000;

    error WrongChain(uint256 expected, uint256 actual);

    event UnsafeSepoliaMockDeployed(
        address indexed admin,
        address token,
        address verifier,
        address escapeVerifier,
        address settlement,
        address vault
    );

    function run() external {
        if (block.chainid != SEPOLIA_CHAIN_ID) {
            revert WrongChain(SEPOLIA_CHAIN_ID, block.chainid);
        }

        uint256 privateKey = vm.envUint("PRIVATE_KEY");
        address admin = vm.addr(privateKey);

        vm.startBroadcast(privateKey);
        MintableMockUSDC token = new MintableMockUSDC();
        UnsafeSepoliaMockVerifierAdapter verifier = new UnsafeSepoliaMockVerifierAdapter();
        UnsafeSepoliaMockVerifierAdapter escapeVerifier = new UnsafeSepoliaMockVerifierAdapter();
        SybilSettlement settlement = new SybilSettlement(admin, verifier, ADMIN_TIMELOCK);
        SybilVault vault = new SybilVault(
            admin,
            token,
            settlement,
            verifier,
            escapeVerifier,
            WITHDRAWAL_DELAY,
            ESCAPE_TIMEOUT,
            ADMIN_TIMELOCK
        );
        settlement.setVault(vault);
        token.mint(admin, DEPLOYER_MOCK_FUNDING);

        emit UnsafeSepoliaMockDeployed(
            admin,
            address(token),
            address(verifier),
            address(escapeVerifier),
            address(settlement),
            address(vault)
        );
        vm.stopBroadcast();
    }
}
