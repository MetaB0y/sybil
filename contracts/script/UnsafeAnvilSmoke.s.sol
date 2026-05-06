// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {UnsafeAcceptAllVerifierAdapter} from "../src/dev/UnsafeAcceptAllVerifierAdapter.sol";
import {SybilSettlement} from "../src/SybilSettlement.sol";
import {SybilTypes} from "../src/SybilTypes.sol";
import {SybilVault} from "../src/SybilVault.sol";
import {MockUSDC} from "../test/mocks/MockUSDC.sol";

interface Vm {
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

contract UnsafeAnvilSmoke {
    Vm private constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));

    uint64 private constant WITHDRAWAL_DELAY = 0;
    uint64 private constant ESCAPE_TIMEOUT = 7 days;
    bytes32 private constant ACCOUNT_KEY = keccak256("sybil/anvil-smoke/account-key");
    bytes32 private constant BLOCK_HASH = keccak256("sybil/anvil-smoke/block");
    bytes32 private constant EVENTS_ROOT = keccak256("sybil/anvil-smoke/events");
    bytes32 private constant WITNESS_ROOT = keccak256("sybil/anvil-smoke/witness");
    bytes32 private constant DA_COMMITMENT = keccak256("sybil/anvil-smoke/da");

    event UnsafeAnvilSmokeDeployed(
        address indexed admin,
        address token,
        address verifier,
        address settlement,
        address vault,
        uint64 latestHeight,
        bytes32 latestStateRoot
    );

    function run() external {
        uint256 privateKey = vm.envUint("PRIVATE_KEY");
        address admin = vm.addr(privateKey);

        vm.startBroadcast(privateKey);

        MockUSDC token = new MockUSDC();
        UnsafeAcceptAllVerifierAdapter verifier = new UnsafeAcceptAllVerifierAdapter();
        SybilSettlement settlement = new SybilSettlement(admin, verifier);
        SybilVault vault =
            new SybilVault(admin, token, settlement, verifier, WITHDRAWAL_DELAY, ESCAPE_TIMEOUT);
        settlement.setVault(vault);

        token.mint(admin, 1_000_000_000);
        require(token.approve(address(vault), type(uint256).max), "approve");
        vault.deposit(1_000_000, ACCOUNT_KEY);

        SybilTypes.StateTransitionPublicInputs memory rootInputs =
            _rootInputs(vault, bytes32(0), 0, keccak256("sybil/anvil-smoke/state-root/1"));
        settlement.submitStateRoot(rootInputs, hex"01");

        bytes32 nullifier = keccak256("sybil/anvil-smoke/withdrawal/nullifier");
        SybilTypes.WithdrawalPublicInputs memory withdrawalInputs = SybilTypes.WithdrawalPublicInputs({
            stateRoot: rootInputs.newStateRoot,
            height: rootInputs.newHeight,
            nullifier: nullifier,
            recipient: admin,
            token: address(token),
            amount: 100_000,
            claimKind: vault.CLAIM_KIND_NORMAL()
        });
        vault.requestWithdrawal(withdrawalInputs, hex"02");
        vault.finalizeWithdrawal(nullifier);

        require(settlement.latestHeight() == 1, "latest height");
        require(settlement.latestStateRoot() == rootInputs.newStateRoot, "latest root");
        require(token.balanceOf(address(vault)) == 900_000, "vault balance");

        emit UnsafeAnvilSmokeDeployed(
            admin,
            address(token),
            address(verifier),
            address(settlement),
            address(vault),
            settlement.latestHeight(),
            settlement.latestStateRoot()
        );

        vm.stopBroadcast();
    }

    function _rootInputs(
        SybilVault vault,
        bytes32 previousRoot,
        uint64 previousHeight,
        bytes32 newRoot
    ) private view returns (SybilTypes.StateTransitionPublicInputs memory) {
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
