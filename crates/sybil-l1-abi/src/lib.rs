#![allow(
    clippy::too_many_arguments,
    reason = "generated ABI signatures mirror Solidity function fields"
)]

//! Host-side Solidity ABI bindings shared by L1 clients.
//!
//! This crate is deliberately separate from `sybil-l1-protocol`: the latter is
//! compiled into OpenVM guests, while these generated bindings are only needed
//! by host processes that talk to Ethereum.

// The generated constructors mirror wide Solidity structs and are not called
// positionally by consumers.
alloy::sol! {
    struct RootRecord {
        uint64 height;
        bytes32 stateRoot;
        bytes32 previousStateRoot;
        bytes32 blockHash;
        bytes32 eventsRoot;
        bytes32 witnessRoot;
        bytes32 daCommitment;
        bytes32 depositRoot;
        uint64 depositCount;
        uint64 verifiedAt;
        uint32 verifierVersion;
    }

    struct StateTransitionPublicInputs {
        uint64 previousHeight;
        uint64 newHeight;
        bytes32 previousStateRoot;
        bytes32 newStateRoot;
        bytes32 blockHash;
        bytes32 eventsRoot;
        bytes32 witnessRoot;
        bytes32 daCommitment;
        bytes32 depositRoot;
        uint64 depositCount;
    }

    struct EscapeClaimPublicInputs {
        bytes32 stateRoot;
        uint64 height;
        uint64 accountId;
        address recipient;
        uint256 amount;
        bytes32 nullifier;
    }

    #[sol(rpc)]
    interface SybilSettlement {
        function submitStateRoot(
            StateTransitionPublicInputs calldata inputs,
            bytes calldata proof
        ) external;

        function latestHeight() external view returns (uint64 height);

        function rootAt(uint64 height) external view returns (RootRecord memory record);
    }

    #[sol(rpc)]
    interface SybilVault {
        event DepositReceived(
            uint64 indexed depositId,
            address indexed sender,
            bytes32 indexed sybilAccountKey,
            address token,
            uint256 amount,
            bytes32 depositRoot
        );

        event WithdrawalQueued(
            bytes32 indexed nullifier,
            address indexed recipient,
            address token,
            uint256 amount,
            bytes32 stateRoot,
            uint64 height,
            uint64 requestedAt,
            uint64 executableAt
        );

        event WithdrawalFinalized(
            bytes32 indexed nullifier,
            address indexed recipient,
            uint256 amount,
            uint64 finalizedAt,
            uint64 executableAt
        );

        event WithdrawalCancelled(
            bytes32 indexed nullifier,
            address indexed recipient,
            uint256 amount,
            uint64 cancelledAt,
            uint64 executableAt,
            string reason
        );

        function depositRootByCount(uint64 count) external view returns (bytes32 root);

        function escapeClaim(
            EscapeClaimPublicInputs calldata inputs,
            bytes calldata proof
        ) external;
    }
}
