// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {OpenVmVerifierAdapter} from "../src/OpenVmVerifierAdapter.sol";
import {IOpenVmHalo2Verifier} from "../src/interfaces/IOpenVmHalo2Verifier.sol";
import {MockOpenVmHalo2Verifier} from "./mocks/MockOpenVmHalo2Verifier.sol";

interface OpenVmVerifierAdapterVm {
    function expectCall(
        address callee,
        bytes calldata data
    ) external;
}

contract OpenVmVerifierAdapterTest {
    OpenVmVerifierAdapterVm private constant vm =
        OpenVmVerifierAdapterVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    bytes32 private constant APP_EXE_COMMIT = keccak256("sybil-app-exe");
    bytes32 private constant APP_VM_COMMIT = keccak256("sybil-app-vm");
    bytes32 private constant PUBLIC_INPUT_HASH = keccak256("sybil-public-input");

    MockOpenVmHalo2Verifier private halo2Verifier;
    OpenVmVerifierAdapter private adapter;

    function setUp() public {
        halo2Verifier = new MockOpenVmHalo2Verifier();
        adapter = new OpenVmVerifierAdapter(halo2Verifier, APP_EXE_COMMIT, APP_VM_COMMIT);
    }

    function testVerifiesPinnedSybilOpenVmProof() public {
        bytes memory publicValues = _publicValues(PUBLIC_INPUT_HASH);
        bytes memory proofData = hex"01020304";
        bytes memory proof = abi.encode(publicValues, proofData, APP_EXE_COMMIT, APP_VM_COMMIT);

        vm.expectCall(
            address(halo2Verifier),
            abi.encodeWithSelector(
                IOpenVmHalo2Verifier.verify.selector,
                publicValues,
                proofData,
                APP_EXE_COMMIT,
                APP_VM_COMMIT
            )
        );

        require(adapter.verify(proof, PUBLIC_INPUT_HASH), "proof rejected");
    }

    function testRejectsDifferentAppExeCommit() public view {
        bytes memory proof = abi.encode(
            _publicValues(PUBLIC_INPUT_HASH), hex"01020304", keccak256("other-exe"), APP_VM_COMMIT
        );

        require(!adapter.verify(proof, PUBLIC_INPUT_HASH), "wrong exe commit accepted");
    }

    function testRejectsDifferentAppVmCommit() public view {
        bytes memory proof = abi.encode(
            _publicValues(PUBLIC_INPUT_HASH), hex"01020304", APP_EXE_COMMIT, keccak256("other-vm")
        );

        require(!adapter.verify(proof, PUBLIC_INPUT_HASH), "wrong vm commit accepted");
    }

    function testRejectsWrongPublicInputHash() public view {
        bytes memory proof = abi.encode(
            _publicValues(keccak256("other-input")), hex"01020304", APP_EXE_COMMIT, APP_VM_COMMIT
        );

        require(!adapter.verify(proof, PUBLIC_INPUT_HASH), "wrong public input accepted");
    }

    function testRejectsNonzeroTrailingPublicValues() public view {
        bytes memory publicValues = _publicValues(PUBLIC_INPUT_HASH);
        _writePublicValue(publicValues, 1, keccak256("extra-public-value"));
        bytes memory proof = abi.encode(publicValues, hex"01020304", APP_EXE_COMMIT, APP_VM_COMMIT);

        require(!adapter.verify(proof, PUBLIC_INPUT_HASH), "extra public value accepted");
    }

    function testRejectsWrongPublicValueLength() public view {
        bytes memory proof = abi.encode(
            abi.encodePacked(PUBLIC_INPUT_HASH), hex"01020304", APP_EXE_COMMIT, APP_VM_COMMIT
        );

        require(!adapter.verify(proof, PUBLIC_INPUT_HASH), "short public values accepted");
    }

    function testRejectsMalformedProofEncoding() public view {
        require(!adapter.verify(hex"1234", PUBLIC_INPUT_HASH), "malformed proof accepted");
    }

    function testReturnsFalseWhenHalo2VerifierReverts() public {
        halo2Verifier.setShouldRevert(true);
        bytes memory proof = abi.encode(
            _publicValues(PUBLIC_INPUT_HASH), hex"01020304", APP_EXE_COMMIT, APP_VM_COMMIT
        );

        require(!adapter.verify(proof, PUBLIC_INPUT_HASH), "reverting verifier accepted");
    }

    function _publicValues(
        bytes32 firstWord
    ) internal pure returns (bytes memory publicValues) {
        publicValues = new bytes(32 * 32);
        _writePublicValue(publicValues, 0, firstWord);
    }

    function _writePublicValue(
        bytes memory publicValues,
        uint256 index,
        bytes32 value
    ) internal pure {
        assembly {
            mstore(add(add(publicValues, 32), mul(index, 32)), value)
        }
    }
}
