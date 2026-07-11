// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {UnsafeAcceptAllVerifierAdapter} from "../src/dev/UnsafeAcceptAllVerifierAdapter.sol";
import {SybilSettlement} from "../src/SybilSettlement.sol";
import {SybilTypes} from "../src/SybilTypes.sol";
import {SybilVault} from "../src/SybilVault.sol";
import {MockUSDC} from "../test/mocks/MockUSDC.sol";

interface EscapeSetupVm {
    function addr(
        uint256 privateKey
    ) external returns (address);
    function envBytes32(
        string calldata name
    ) external view returns (bytes32);
    function envUint(
        string calldata name
    ) external view returns (uint256);
    function startBroadcast(
        uint256 privateKey
    ) external;
    function stopBroadcast() external;
}

/// @dev Throwaway-only setup for the custody CLI fixture-proof drill. The
/// accept-all adapter is what lets the default drill test payout and calldata
/// plumbing without invoking OpenVM proving on development/CI machines.
contract UnsafeAnvilEscapeSetup {
    EscapeSetupVm private constant vm =
        EscapeSetupVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    uint64 private constant ADMIN_TIMELOCK = 0;
    uint64 private constant ESCAPE_TIMEOUT = 1;
    uint256 private constant VAULT_FUNDING = 100_000_000;

    event EscapeFixtureDeployed(address token, address settlement, address vault);

    function run() external {
        uint256 privateKey = vm.envUint("PRIVATE_KEY");
        address admin = vm.addr(privateKey);
        uint64 height = uint64(vm.envUint("ROOT_HEIGHT"));

        vm.startBroadcast(privateKey);
        MockUSDC token = new MockUSDC();
        UnsafeAcceptAllVerifierAdapter verifier = new UnsafeAcceptAllVerifierAdapter();
        UnsafeAcceptAllVerifierAdapter escapeVerifier = new UnsafeAcceptAllVerifierAdapter();
        SybilSettlement settlement = new SybilSettlement(admin, verifier, ADMIN_TIMELOCK);
        SybilVault vault = new SybilVault(
            admin, token, settlement, verifier, escapeVerifier, 0, ESCAPE_TIMEOUT, ADMIN_TIMELOCK
        );
        settlement.setVault(vault);

        token.mint(admin, VAULT_FUNDING);
        require(token.approve(address(vault), VAULT_FUNDING), "approve");
        vault.deposit(VAULT_FUNDING, keccak256("sybil/custody-e2e/funding"));

        settlement.submitStateRoot(
            SybilTypes.StateTransitionPublicInputs({
                previousHeight: 0,
                newHeight: height,
                previousStateRoot: bytes32(0),
                newStateRoot: vm.envBytes32("STATE_ROOT"),
                blockHash: vm.envBytes32("BLOCK_HASH"),
                eventsRoot: bytes32(uint256(1)),
                witnessRoot: vm.envBytes32("WITNESS_ROOT"),
                daCommitment: vm.envBytes32("DA_COMMITMENT"),
                depositRoot: vault.depositRootByCount(vault.depositCount()),
                depositCount: vault.depositCount()
            }),
            hex"01"
        );
        emit EscapeFixtureDeployed(address(token), address(settlement), address(vault));
        vm.stopBroadcast();
    }
}
