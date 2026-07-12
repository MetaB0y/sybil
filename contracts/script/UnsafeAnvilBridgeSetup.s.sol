// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {UnsafeAcceptAllVerifierAdapter} from "../src/dev/UnsafeAcceptAllVerifierAdapter.sol";
import {SybilSettlement} from "../src/SybilSettlement.sol";
import {SybilVault} from "../src/SybilVault.sol";
import {MockUSDC} from "../test/mocks/MockUSDC.sol";

interface BridgeSetupVm {
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

/// @dev Local-Anvil-only deployment for the normal bridge round-trip harness.
/// Both adapters accept every proof. This tests contract/API/indexer plumbing;
/// it is never evidence of production withdrawal authorization.
contract UnsafeAnvilBridgeSetup {
    BridgeSetupVm private constant vm =
        BridgeSetupVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    uint256 private constant USER_FUNDING = 1_000_000_000;

    event UnsafeBridgeFixtureDeployed(address token, address settlement, address vault);

    function run() external {
        uint256 privateKey = vm.envUint("PRIVATE_KEY");
        address admin = vm.addr(privateKey);

        vm.startBroadcast(privateKey);
        MockUSDC token = new MockUSDC();
        UnsafeAcceptAllVerifierAdapter verifier = new UnsafeAcceptAllVerifierAdapter();
        UnsafeAcceptAllVerifierAdapter escapeVerifier = new UnsafeAcceptAllVerifierAdapter();
        SybilSettlement settlement = new SybilSettlement(admin, verifier, 0);
        SybilVault vault =
            new SybilVault(admin, token, settlement, verifier, escapeVerifier, 0, 7 days, 0);
        settlement.setVault(vault);

        token.mint(admin, USER_FUNDING);
        require(token.approve(address(vault), type(uint256).max), "approve");

        emit UnsafeBridgeFixtureDeployed(address(token), address(settlement), address(vault));
        vm.stopBroadcast();
    }
}
