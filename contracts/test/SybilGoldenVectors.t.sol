// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {SybilSettlement} from "../src/SybilSettlement.sol";
import {SybilVault} from "../src/SybilVault.sol";
import {SybilTypes} from "../src/SybilTypes.sol";
import {IERC20Minimal} from "../src/interfaces/IERC20Minimal.sol";
import {IOpenVmVerifierAdapter} from "../src/interfaces/IOpenVmVerifierAdapter.sol";
import {ISybilSettlement} from "../src/interfaces/ISybilSettlement.sol";

interface GoldenVectorVm {
    function etch(
        address target,
        bytes calldata newRuntimeBytecode
    ) external;
}

contract SybilVaultDepositHarness is SybilVault {
    address private constant ADMIN = 0xaAaAaAaaAaAaAaaAaAAAAAAAAaaaAaAaAaaAaaAa;
    address private constant TOKEN_ADDRESS = 0x2222222222222222222222222222222222222222;
    address private constant SETTLEMENT_ADDRESS = 0xbBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB;
    address private constant VERIFIER_ADDRESS = 0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC;

    constructor()
        SybilVault(
            ADMIN,
            IERC20Minimal(TOKEN_ADDRESS),
            ISybilSettlement(SETTLEMENT_ADDRESS),
            IOpenVmVerifierAdapter(VERIFIER_ADDRESS),
            1 days,
            7 days,
            2 days
        )
    {}

    function initializeDepositTreeForTest() external {
        zeroHashes[0] = bytes32(0);
        for (uint8 level = 0; level < DEPOSIT_TREE_DEPTH; level++) {
            zeroHashes[level + 1] = hashNode(zeroHashes[level], zeroHashes[level]);
        }
        currentDepositRoot = zeroHashes[DEPOSIT_TREE_DEPTH];
        depositRootByCount[0] = currentDepositRoot;
    }

    function appendDepositForTest(
        uint64 depositId,
        address sender,
        bytes32 sybilAccountKey,
        uint256 amount
    ) external returns (bytes32 leaf, bytes32 treeLeaf, bytes32 root) {
        leaf = depositLeaf(depositId, sender, sybilAccountKey, amount);
        treeLeaf = hashDepositLeaf(leaf);
        root = _appendDepositLeaf(depositId, treeLeaf);
        currentDepositRoot = root;
        depositCount = depositId;
        depositRootByCount[depositId] = root;
    }
}

contract SybilGoldenVectorsTest {
    GoldenVectorVm private constant vm =
        GoldenVectorVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    uint256 private constant CHAIN_ID = 31_337;
    address private constant VAULT_ADDRESS = 0x1111111111111111111111111111111111111111;
    address private constant VERIFIER_ADDRESS = 0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC;

    address private constant SENDER_1 = 0x3333333333333333333333333333333333333333;
    address private constant SENDER_2 = 0x5555555555555555555555555555555555555555;
    address private constant SENDER_3 = 0x7777777777777777777777777777777777777777;
    address private constant SENDER_HIGH = 0x9999999999999999999999999999999999999999;
    bytes32 private constant KEY_1 =
        0x4444444444444444444444444444444444444444444444444444444444444444;
    bytes32 private constant KEY_2 =
        0x6666666666666666666666666666666666666666666666666666666666666666;
    bytes32 private constant KEY_3 =
        0x8888888888888888888888888888888888888888888888888888888888888888;
    bytes32 private constant KEY_HIGH =
        0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa;
    uint64 private constant HIGH_DEPOSIT_ID = 0xfedcba9876543210;
    uint256 private constant MAX_U64_AMOUNT = type(uint64).max;

    bytes32 private constant EMPTY_DEPOSIT_ROOT =
        0x7c1d0e8a93ea9c09cc13b91ead8f72de66a33cb695c30934dc2d75bffac1248e;
    bytes32 private constant DEPOSIT_1_LEAF =
        0x10348417835957783f646308469b0c1a7d42fcb7e8a67cc0774b969cd3bc4e78;
    bytes32 private constant DEPOSIT_1_TREE_LEAF =
        0xcab93c3c5e862aa9e8fc0cff679d4d6febdf3305c81f65207871cea439975d5f;
    bytes32 private constant DEPOSIT_1_ROOT =
        0x2e7fc1c1f7494f98b453f8be88ee3b99b47321b95425faf6853c3e59618de440;
    bytes32 private constant DEPOSIT_2_LEAF =
        0xaf7bae1be80d057d1c48f70e9786a8466e1c8b858fef8d5ecbb9e10bcba40776;
    bytes32 private constant DEPOSIT_2_TREE_LEAF =
        0x2da1b7553fc86717d219e442f9abbee77ccaf81f2a8c9487aa08da89a8dbe9ba;
    bytes32 private constant DEPOSIT_2_ROOT =
        0xbf00beb7a033f95b583dfb040f9f962db5f538c56e11cb9b3fa303b69d820b1f;
    bytes32 private constant DEPOSIT_3_LEAF =
        0x2c380522f079cfb922808acb18d9677576ad3bf4c0dc61de79a88edb0840b939;
    bytes32 private constant DEPOSIT_3_TREE_LEAF =
        0x4ddbf4504459403a113a894bd821e6e0ad9ee8ac9cca1ddba7a91ff9413bab75;
	    bytes32 private constant DEPOSIT_3_ROOT =
	        0x5d9b49419ded14b47faf0f943198c33647c016bd37f998b1d9196b103acfecda;
	    bytes32 private constant DEPOSIT_FRONTIER_AFTER_2_LEVEL_1 =
	        0xe167afbeb71311d09d4353dca2b4d7cd1c44431e6bbee2305720c27a9a8059e0;
	    bytes32 private constant HIGH_DEPOSIT_LEAF =
	        0x0e0fe498f14aa8310467572c634bc13d6617573ca1fe7587c1fd642fbad168a1;
    bytes32 private constant HIGH_DEPOSIT_TREE_LEAF =
        0xf7f3a6aeef19f4464f11bdfe4358124d745de1295dd03a116cccb1ab7ff2e90f;
    bytes32 private constant STATE_TRANSITION_PUBLIC_INPUT_HASH =
        0x42197d0dff7bc2f86a6e359f187adda163fc9b4ffaa0e7cfb9845561bb744830;

    SybilVaultDepositHarness private vault;
    SybilSettlement private settlement;

    function setUp() public {
        SybilVaultDepositHarness template = new SybilVaultDepositHarness();
        vm.etch(VAULT_ADDRESS, address(template).code);
        vault = SybilVaultDepositHarness(VAULT_ADDRESS);
        vault.initializeDepositTreeForTest();
        settlement = new SybilSettlement(address(this), IOpenVmVerifierAdapter(VERIFIER_ADDRESS), 2 days);
    }

    function testDepositLeafAndPrefixRootsMatchRustGoldenVectors() public {
        // Twin: crates/sybil-l1-protocol/src/lib.rs. Keep these constants
        // byte-for-byte aligned with the Rust suite.
        require(block.chainid == CHAIN_ID, "chain id");
        require(vault.depositRootByCount(0) == EMPTY_DEPOSIT_ROOT, "empty root");

        (bytes32 leaf1, bytes32 treeLeaf1, bytes32 root1) =
            vault.appendDepositForTest(1, SENDER_1, KEY_1, 1_000_000);
        require(leaf1 == DEPOSIT_1_LEAF, "deposit 1 leaf");
	        require(treeLeaf1 == DEPOSIT_1_TREE_LEAF, "deposit 1 tree leaf");
	        require(root1 == DEPOSIT_1_ROOT, "deposit 1 root");
	        require(vault.depositRootByCount(1) == DEPOSIT_1_ROOT, "depositRootByCount 1");
	        require(vault.filledSubtrees(0) == DEPOSIT_1_TREE_LEAF, "frontier 1 level 0");

        (bytes32 leaf2, bytes32 treeLeaf2, bytes32 root2) =
            vault.appendDepositForTest(2, SENDER_2, KEY_2, 2_500_000);
        require(leaf2 == DEPOSIT_2_LEAF, "deposit 2 leaf");
	        require(treeLeaf2 == DEPOSIT_2_TREE_LEAF, "deposit 2 tree leaf");
	        require(root2 == DEPOSIT_2_ROOT, "deposit 2 root");
	        require(vault.depositRootByCount(2) == DEPOSIT_2_ROOT, "depositRootByCount 2");
	        require(vault.filledSubtrees(1) == DEPOSIT_FRONTIER_AFTER_2_LEVEL_1, "frontier 2 level 1");

        (bytes32 leaf3, bytes32 treeLeaf3, bytes32 root3) =
            vault.appendDepositForTest(3, SENDER_3, KEY_3, 42_000_001);
        require(leaf3 == DEPOSIT_3_LEAF, "deposit 3 leaf");
	        require(treeLeaf3 == DEPOSIT_3_TREE_LEAF, "deposit 3 tree leaf");
	        require(root3 == DEPOSIT_3_ROOT, "deposit 3 root");
	        require(vault.depositRootByCount(3) == DEPOSIT_3_ROOT, "depositRootByCount 3");
	        require(vault.filledSubtrees(0) == DEPOSIT_3_TREE_LEAF, "frontier 3 level 0");
	        require(vault.filledSubtrees(1) == DEPOSIT_FRONTIER_AFTER_2_LEVEL_1, "frontier 3 level 1");

        bytes32 highLeaf = vault.depositLeaf(HIGH_DEPOSIT_ID, SENDER_HIGH, KEY_HIGH, MAX_U64_AMOUNT);
        bytes32 highTreeLeaf = vault.hashDepositLeaf(highLeaf);
        require(highLeaf == HIGH_DEPOSIT_LEAF, "high deposit leaf");
        require(highTreeLeaf == HIGH_DEPOSIT_TREE_LEAF, "high deposit tree leaf");
    }

    function testStateTransitionPublicInputHashMatchesRustGoldenVector() public view {
        // Twin: crates/sybil-zk/src/lib.rs. Keep these constants byte-for-byte
        // aligned with the Rust suite.
        SybilTypes.StateTransitionPublicInputs memory inputs = SybilTypes.StateTransitionPublicInputs({
            previousHeight: 41,
            newHeight: 42,
            previousStateRoot: 0x1010101010101010101010101010101010101010101010101010101010101010,
            newStateRoot: 0x2020202020202020202020202020202020202020202020202020202020202020,
            blockHash: 0x3030303030303030303030303030303030303030303030303030303030303030,
            eventsRoot: 0x4040404040404040404040404040404040404040404040404040404040404040,
            witnessRoot: 0x5050505050505050505050505050505050505050505050505050505050505050,
            daCommitment: 0x6060606060606060606060606060606060606060606060606060606060606060,
            depositRoot: DEPOSIT_3_ROOT,
            depositCount: 3
        });

        require(
            settlement.stateTransitionPublicInputHash(inputs) == STATE_TRANSITION_PUBLIC_INPUT_HASH,
            "state public input hash"
        );
    }
}
