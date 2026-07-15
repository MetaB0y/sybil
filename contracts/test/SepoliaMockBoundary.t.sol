// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {MintableMockUSDC} from "../src/dev/MintableMockUSDC.sol";
import {UnsafeSepoliaMockVerifierAdapter} from "../src/dev/UnsafeSepoliaMockVerifierAdapter.sol";

interface SepoliaBoundaryVm {
    function chainId(
        uint256 newChainId
    ) external;
}

contract SepoliaMockBoundaryTest {
    uint256 private constant SEPOLIA_CHAIN_ID = 11_155_111;
    SepoliaBoundaryVm private constant vm =
        SepoliaBoundaryVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    function testAcceptAllAdapterCanOnlyDeployOnSepolia() public {
        try new UnsafeSepoliaMockVerifierAdapter() returns (
            UnsafeSepoliaMockVerifierAdapter unexpected
        ) {
            unexpected;
            revert("adapter deployed outside Sepolia");
        } catch (bytes memory revertData) {
            require(revertData.length >= 4, "missing wrong-chain selector");
            bytes4 selector;
            assembly {
                selector := mload(add(revertData, 32))
            }
            require(
                selector == UnsafeSepoliaMockVerifierAdapter.WrongChain.selector,
                "unexpected wrong-chain revert"
            );
        }

        vm.chainId(SEPOLIA_CHAIN_ID);
        UnsafeSepoliaMockVerifierAdapter adapter = new UnsafeSepoliaMockVerifierAdapter();
        require(adapter.unsafeAcceptsAllProofs(), "unsafe marker missing");
        require(adapter.verify(hex"deadbeef", keccak256("arbitrary")), "mock proof rejected");
    }

    function testMockCollateralIsPubliclyMintableAndSixDecimals() public {
        MintableMockUSDC token = new MintableMockUSDC();
        address recipient = address(0xBEEF);
        token.mint(address(this), 2_000_000);
        require(token.decimals() == 6, "wrong decimals");
        require(token.totalSupply() == 2_000_000, "wrong supply");
        require(token.transfer(recipient, 750_000), "transfer failed");
        require(token.balanceOf(recipient) == 750_000, "recipient not paid");
    }
}
