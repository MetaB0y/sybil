// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {IOpenVmHalo2Verifier} from "./interfaces/IOpenVmHalo2Verifier.sol";
import {IOpenVmVerifierAdapter} from "./interfaces/IOpenVmVerifierAdapter.sol";

contract OpenVmVerifierAdapter is IOpenVmVerifierAdapter {
    uint256 public constant OPENVM_PUBLIC_VALUE_WORDS = 32;
    uint256 public constant OPENVM_PUBLIC_VALUES_BYTES = OPENVM_PUBLIC_VALUE_WORDS * 32;

    IOpenVmHalo2Verifier public immutable halo2Verifier;
    bytes32 public immutable expectedAppExeCommit;
    bytes32 public immutable expectedAppVmCommit;

    error ZeroAddress();
    error ZeroCommit();

    constructor(
        IOpenVmHalo2Verifier verifier,
        bytes32 appExeCommit,
        bytes32 appVmCommit
    ) {
        if (address(verifier) == address(0)) revert ZeroAddress();
        if (appExeCommit == bytes32(0) || appVmCommit == bytes32(0)) revert ZeroCommit();

        halo2Verifier = verifier;
        expectedAppExeCommit = appExeCommit;
        expectedAppVmCommit = appVmCommit;
    }

    function verify(
        bytes calldata proof,
        bytes32 publicInputHash
    ) external view returns (bool) {
        try this.decodeProof(proof) returns (
            bytes memory publicValues,
            bytes memory proofData,
            bytes32 appExeCommit,
            bytes32 appVmCommit
        ) {
            if (appExeCommit != expectedAppExeCommit || appVmCommit != expectedAppVmCommit) {
                return false;
            }
            if (!_publicValuesMatch(publicValues, publicInputHash)) {
                return false;
            }

            try halo2Verifier.verify(publicValues, proofData, appExeCommit, appVmCommit) {
                return true;
            } catch {
                return false;
            }
        } catch {
            return false;
        }
    }

    function decodeProof(
        bytes calldata proof
    )
        external
        pure
        returns (
            bytes memory publicValues,
            bytes memory proofData,
            bytes32 appExeCommit,
            bytes32 appVmCommit
        )
    {
        return abi.decode(proof, (bytes, bytes, bytes32, bytes32));
    }

    function _publicValuesMatch(
        bytes memory publicValues,
        bytes32 publicInputHash
    ) internal pure returns (bool) {
        if (publicValues.length != OPENVM_PUBLIC_VALUES_BYTES) {
            return false;
        }

        bytes32 revealedHash;
        assembly {
            revealedHash := mload(add(publicValues, 32))
        }
        if (revealedHash != publicInputHash) {
            return false;
        }

        for (uint256 offset = 32; offset < OPENVM_PUBLIC_VALUES_BYTES; offset += 32) {
            bytes32 word;
            assembly {
                word := mload(add(add(publicValues, 32), offset))
            }
            if (word != bytes32(0)) {
                return false;
            }
        }

        return true;
    }
}
