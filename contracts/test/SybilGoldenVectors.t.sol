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

    function readFile(
        string calldata path
    ) external view returns (string memory data);

    function parseJsonAddress(
        string calldata json,
        string calldata key
    ) external pure returns (address value);

    function parseJsonBytes(
        string calldata json,
        string calldata key
    ) external pure returns (bytes memory value);

    function parseJsonBytes32(
        string calldata json,
        string calldata key
    ) external pure returns (bytes32 value);

    function parseJsonUint(
        string calldata json,
        string calldata key
    ) external pure returns (uint256 value);
}

contract SybilVaultDepositHarness is SybilVault {
    address private constant ADMIN = 0xaAaAaAaaAaAaAaaAaAAAAAAAAaaaAaAaAaaAaaAa;
    address private constant TOKEN_ADDRESS = 0x2222222222222222222222222222222222222222;
    address private constant SETTLEMENT_ADDRESS = 0xbBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB;
    address private constant VERIFIER_ADDRESS = 0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC;
    address private constant ESCAPE_VERIFIER_ADDRESS = 0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE;

    constructor()
        SybilVault(
            ADMIN,
            IERC20Minimal(TOKEN_ADDRESS),
            ISybilSettlement(SETTLEMENT_ADDRESS),
            IOpenVmVerifierAdapter(VERIFIER_ADDRESS),
            IOpenVmVerifierAdapter(ESCAPE_VERIFIER_ADDRESS),
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

    address private constant VAULT_ADDRESS = 0x1111111111111111111111111111111111111111;
    address private constant VERIFIER_ADDRESS = 0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC;

    bytes private constant ACCOUNT_KEYS_DIGEST_DOMAIN = "sybil/state/account-keys-digest/v2";

    SybilVaultDepositHarness private vault;
    SybilSettlement private settlement;
    string private golden;

    function setUp() public {
        golden = vm.readFile("../golden/golden-vectors.json");
        SybilVaultDepositHarness template = new SybilVaultDepositHarness();
        vm.etch(VAULT_ADDRESS, address(template).code);
        vault = SybilVaultDepositHarness(VAULT_ADDRESS);
        vault.initializeDepositTreeForTest();
        settlement =
            new SybilSettlement(address(this), IOpenVmVerifierAdapter(VERIFIER_ADDRESS), 2 days);
    }

    function testDepositLeafAndPrefixRootsMatchRustGoldenVectors() public {
        // Twin: crates/sybil-l1-protocol/src/lib.rs. Both suites read the same
        // generator-owned repo-root JSON.
        require(block.chainid == goldenUint(".deposits.chain_id"), "chain id");
        require(vault.depositRootByCount(0) == goldenBytes32(".deposits.empty_root"), "empty root");

        (bytes32 leaf1, bytes32 treeLeaf1, bytes32 root1) = vault.appendDepositForTest(
            uint64(goldenUint(".deposits.entries[0].deposit_id")),
            goldenAddress(".deposits.entries[0].sender"),
            goldenBytes32(".deposits.entries[0].sybil_account_key"),
            goldenUint(".deposits.entries[0].amount_token_units")
        );
        require(leaf1 == goldenBytes32(".deposits.entries[0].leaf"), "deposit 1 leaf");
        require(treeLeaf1 == goldenBytes32(".deposits.entries[0].tree_leaf"), "deposit 1 tree leaf");
        require(root1 == goldenBytes32(".deposits.entries[0].prefix_root"), "deposit 1 root");
        require(vault.depositRootByCount(1) == root1, "depositRootByCount 1");
        require(vault.filledSubtrees(0) == treeLeaf1, "frontier 1 level 0");

        (bytes32 leaf2, bytes32 treeLeaf2, bytes32 root2) = vault.appendDepositForTest(
            uint64(goldenUint(".deposits.entries[1].deposit_id")),
            goldenAddress(".deposits.entries[1].sender"),
            goldenBytes32(".deposits.entries[1].sybil_account_key"),
            goldenUint(".deposits.entries[1].amount_token_units")
        );
        require(leaf2 == goldenBytes32(".deposits.entries[1].leaf"), "deposit 2 leaf");
        require(treeLeaf2 == goldenBytes32(".deposits.entries[1].tree_leaf"), "deposit 2 tree leaf");
        require(root2 == goldenBytes32(".deposits.entries[1].prefix_root"), "deposit 2 root");
        require(vault.depositRootByCount(2) == root2, "depositRootByCount 2");
        require(
            vault.filledSubtrees(1) == goldenBytes32(".deposits.frontier_after_two_level_1"),
            "frontier 2 level 1"
        );

        (bytes32 leaf3, bytes32 treeLeaf3, bytes32 root3) = vault.appendDepositForTest(
            uint64(goldenUint(".deposits.entries[2].deposit_id")),
            goldenAddress(".deposits.entries[2].sender"),
            goldenBytes32(".deposits.entries[2].sybil_account_key"),
            goldenUint(".deposits.entries[2].amount_token_units")
        );
        require(leaf3 == goldenBytes32(".deposits.entries[2].leaf"), "deposit 3 leaf");
        require(treeLeaf3 == goldenBytes32(".deposits.entries[2].tree_leaf"), "deposit 3 tree leaf");
        require(root3 == goldenBytes32(".deposits.entries[2].prefix_root"), "deposit 3 root");
        require(vault.depositRootByCount(3) == root3, "depositRootByCount 3");
        require(vault.filledSubtrees(0) == treeLeaf3, "frontier 3 level 0");
        require(
            vault.filledSubtrees(1) == goldenBytes32(".deposits.frontier_after_two_level_1"),
            "frontier 3 level 1"
        );

        bytes32 highLeaf = vault.depositLeaf(
            uint64(goldenUint(".deposits.high_id_max_amount.deposit_id")),
            goldenAddress(".deposits.high_id_max_amount.sender"),
            goldenBytes32(".deposits.high_id_max_amount.sybil_account_key"),
            goldenUint(".deposits.high_id_max_amount.amount_token_units")
        );
        bytes32 highTreeLeaf = vault.hashDepositLeaf(highLeaf);
        require(highLeaf == goldenBytes32(".deposits.high_id_max_amount.leaf"), "high deposit leaf");
        require(
            highTreeLeaf == goldenBytes32(".deposits.high_id_max_amount.tree_leaf"),
            "high deposit tree leaf"
        );
    }

    function testStateTransitionPublicInputHashMatchesRustGoldenVector() public view {
        // Twin: crates/sybil-zk/src/lib.rs. Both suites read the same
        // generator-owned repo-root JSON.
        SybilTypes.StateTransitionPublicInputs memory inputs = SybilTypes.StateTransitionPublicInputs({
            previousHeight: uint64(goldenUint(".state_transition_public_inputs.previous_height")),
            newHeight: uint64(goldenUint(".state_transition_public_inputs.new_height")),
            previousStateRoot: goldenBytes32(".state_transition_public_inputs.previous_state_root"),
            newStateRoot: goldenBytes32(".state_transition_public_inputs.new_state_root"),
            blockHash: goldenBytes32(".state_transition_public_inputs.block_hash"),
            eventsRoot: goldenBytes32(".state_transition_public_inputs.events_root"),
            witnessRoot: goldenBytes32(".state_transition_public_inputs.witness_root"),
            daCommitment: goldenBytes32(".state_transition_public_inputs.da_commitment"),
            depositRoot: goldenBytes32(".state_transition_public_inputs.deposit_root"),
            depositCount: uint64(goldenUint(".state_transition_public_inputs.deposit_count"))
        });

        require(
            settlement.stateTransitionPublicInputHash(inputs)
                == goldenBytes32(".state_transition_public_inputs.hash"),
            "state public input hash"
        );
    }

    function testWithdrawalNullifierMatchesRustGoldenVector() public view {
        bytes32 nullifier = keccak256(
            abi.encode(
                "sybil/withdrawal-nullifier/v1",
                goldenUint(".withdrawal_nullifier.chain_id"),
                goldenAddress(".withdrawal_nullifier.vault_address"),
                goldenUint(".withdrawal_nullifier.withdrawal_id"),
                goldenUint(".withdrawal_nullifier.account_id"),
                goldenAddress(".withdrawal_nullifier.recipient"),
                goldenAddress(".withdrawal_nullifier.token_address"),
                goldenUint(".withdrawal_nullifier.amount_token_units")
            )
        );
        require(
            nullifier == goldenBytes32(".withdrawal_nullifier.nullifier"), "withdrawal nullifier"
        );
    }

    function testContractSelectorsAndEventTopicsMatchRustAlloyBindings() public view {
        require(
            SybilSettlement.submitStateRoot.selector
                == goldenSelector(".l1_abi.settlement.submit_state_root_selector"),
            "settlement submit selector"
        );
        require(
            bytes4(keccak256("latestHeight()"))
                == goldenSelector(".l1_abi.settlement.latest_height_selector"),
            "settlement latest selector"
        );
        require(
            bytes4(keccak256("rootAt(uint64)"))
                == goldenSelector(".l1_abi.settlement.root_at_selector"),
            "settlement root selector"
        );
        require(
            bytes4(keccak256("depositRootByCount(uint64)"))
                == goldenSelector(".l1_abi.vault.deposit_root_by_count_selector"),
            "vault deposit root selector"
        );
        require(
            SybilVault.escapeClaim.selector
                == goldenSelector(".l1_abi.vault.escape_claim_selector"),
            "vault escape selector"
        );
        require(
            SybilVault.DepositReceived.selector
                == goldenBytes32(".l1_abi.vault.deposit_received_topic0"),
            "vault deposit topic"
        );
        require(
            SybilVault.WithdrawalQueued.selector
                == goldenBytes32(".l1_abi.vault.withdrawal_queued_topic0"),
            "vault withdrawal queued topic"
        );
        require(
            SybilVault.WithdrawalFinalized.selector
                == goldenBytes32(".l1_abi.vault.withdrawal_finalized_topic0"),
            "vault withdrawal finalized topic"
        );
        require(
            SybilVault.WithdrawalCancelled.selector
                == goldenBytes32(".l1_abi.vault.withdrawal_cancelled_topic0"),
            "vault withdrawal cancelled topic"
        );
    }

    function testEscapeClaimPublicInputHashMatchesRustGoldenVector() public view {
        SybilTypes.EscapeClaimPublicInputs memory inputs = SybilTypes.EscapeClaimPublicInputs({
            stateRoot: goldenBytes32(".escape_claim_public_inputs.state_root"),
            height: uint64(goldenUint(".escape_claim_public_inputs.height")),
            accountId: uint64(goldenUint(".escape_claim_public_inputs.account_id")),
            recipient: goldenAddress(".escape_claim_public_inputs.recipient"),
            amount: goldenUint(".escape_claim_public_inputs.amount"),
            nullifier: goldenBytes32(".escape_claim_public_inputs.nullifier")
        });

        require(
            vault.escapeClaimPublicInputHash(inputs)
                == goldenBytes32(".escape_claim_public_inputs.hash"),
            "escape public input hash"
        );
    }

    function testAccountKeysDigestMatchesRustGoldenVector() public view {
        // Twin: crates/sybil-verifier/src/byte_identity.rs. The domain and
        // encoding stay independent; the expected values come from one JSON.
        uint64 accountId = uint64(goldenUint(".account_keys.account_id"));
        bytes32 emptyDigest = goldenBytes32(".account_keys.empty_digest");
        require(accountKeysDigest(accountId, 0, hex"") == emptyDigest, "empty keys digest");
        require(emptyDigest != bytes32(0), "empty keys digest nonzero");

        bytes memory sortedRecords = bytes.concat(
            hex"00",
            goldenBytes(".account_keys.raw_p256_key"),
            hex"ffffffff",
            hex"01",
            goldenBytes(".account_keys.webauthn_key"),
            hex"ffffffff"
        );
        require(
            accountKeysDigest(accountId, 2, sortedRecords)
                == goldenBytes32(".account_keys.two_keys_digest"),
            "two keys digest"
        );
    }

    function testCanonicalWitnessBytesHaveExplicitSolidityParityCheck() public view {
        bytes memory witnessBytes = goldenBytes(".canonical_witness.bytes");
        require(
            witnessBytes.length == goldenUint(".canonical_witness.length"),
            "canonical witness length parity"
        );
        require(
            sha256(bytes.concat(le64(uint64(witnessBytes.length)), witnessBytes))
                == goldenBytes32(".canonical_witness.length_prefixed_sha256"),
            "canonical witness digest parity"
        );
    }

    function goldenAddress(
        string memory path
    ) private view returns (address) {
        return vm.parseJsonAddress(golden, path);
    }

    function goldenBytes(
        string memory path
    ) private view returns (bytes memory) {
        return vm.parseJsonBytes(golden, path);
    }

    function goldenBytes32(
        string memory path
    ) private view returns (bytes32) {
        return vm.parseJsonBytes32(golden, path);
    }

    function goldenSelector(
        string memory path
    ) private view returns (bytes4 selector) {
        bytes memory encoded = goldenBytes(path);
        require(encoded.length == 4, "golden selector length");
        assembly ("memory-safe") {
            selector := mload(add(encoded, 32))
        }
    }

    function goldenUint(
        string memory path
    ) private view returns (uint256) {
        return vm.parseJsonUint(golden, path);
    }

    function accountKeysDigest(
        uint64 accountId,
        uint64 keyCount,
        bytes memory sortedRecords
    ) private pure returns (bytes32) {
        return sha256(
            bytes.concat(ACCOUNT_KEYS_DIGEST_DOMAIN, le64(accountId), le64(keyCount), sortedRecords)
        );
    }

    function le64(
        uint64 value
    ) private pure returns (bytes memory out) {
        out = new bytes(8);
        for (uint256 i = 0; i < 8; i++) {
            out[i] = bytes1(uint8(value >> (8 * i)));
        }
    }
}
