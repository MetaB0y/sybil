use alloy::primitives::{Address, B256, Bytes};
use alloy::providers::{DynProvider, Provider};
use alloy::rpc::types::{BlockNumberOrTag, Filter, Log as EthLog, TransactionRequest};
use alloy::sol_types::SolCall;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use sybil_l1_abi::SybilVault;
use sybil_l1_protocol::{
    Bytes32, EthAddress, deposit_received_topic0, withdrawal_cancelled_topic0,
    withdrawal_finalized_topic0, withdrawal_queued_topic0,
};

use super::{IndexerError, L1Rpc, Result};

/// Trust boundary used to turn raw JSON-RPC responses into one L1 view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(super) enum RpcTrustMode {
    /// One endpoint plus a block-count delay. This is intentionally limited to
    /// disposable local chains and makes no public-finality claim.
    UnsafeSingleDev,
    /// Every configured, independently operated endpoint must agree on the
    /// finalized prefix and on every block-pinned log/state response.
    UnanimousFinalized,
}

impl RpcTrustMode {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::UnsafeSingleDev => "unsafe-single-dev",
            Self::UnanimousFinalized => "unanimous-finalized",
        }
    }
}

/// Non-secret durable identity of the configured L1 trust boundary. Provider
/// ids are operator-assigned names, never URLs or credentials.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SourceIdentity {
    pub(super) trust_mode: String,
    pub(super) provider_ids: Vec<String>,
}

impl SourceIdentity {
    pub(super) fn new(mode: RpcTrustMode, provider_ids: Vec<String>) -> Self {
        Self {
            trust_mode: mode.as_str().to_string(),
            provider_ids,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AuthenticatedBlock {
    pub(super) number: u64,
    pub(super) hash: Bytes32,
}

pub(super) struct HttpEndpoint {
    id: String,
    provider: DynProvider,
}

impl HttpEndpoint {
    pub(super) fn new(id: String, provider: DynProvider) -> Self {
        Self { id, provider }
    }
}

pub(super) trait L1Endpoint {
    fn id(&self) -> &str;
    async fn latest_number(&self) -> Result<u64>;
    async fn finalized_block(&self) -> Result<AuthenticatedBlock>;
    async fn block_hash(&self, number: u64) -> Result<Bytes32>;
    async fn bridge_logs_at_hash(
        &self,
        vault: EthAddress,
        block: AuthenticatedBlock,
    ) -> Result<Vec<EthLog>>;
    async fn deposit_root_by_count_at_hash(
        &self,
        vault: EthAddress,
        count: u64,
        block: AuthenticatedBlock,
    ) -> Result<Bytes32>;
}

impl L1Endpoint for HttpEndpoint {
    fn id(&self) -> &str {
        &self.id
    }

    async fn latest_number(&self) -> Result<u64> {
        Ok(self.provider.get_block_number().await?)
    }

    async fn finalized_block(&self) -> Result<AuthenticatedBlock> {
        let block = self
            .provider
            .get_block_by_number(BlockNumberOrTag::Finalized)
            .await?
            .ok_or(IndexerError::MissingRpcResult)?;
        Ok(AuthenticatedBlock {
            number: block.number(),
            hash: block.hash().into(),
        })
    }

    async fn block_hash(&self, number: u64) -> Result<Bytes32> {
        let block = self
            .provider
            .get_block_by_number(BlockNumberOrTag::Number(number))
            .await?
            .ok_or(IndexerError::MissingRpcResult)?;
        if block.number() != number {
            return Err(IndexerError::AuthenticatedViewInvalid {
                context: "block header number",
                block_number: number,
                provider: self.id.clone(),
                message: format!("response carried block number {}", block.number()),
            });
        }
        Ok(block.hash().into())
    }

    async fn bridge_logs_at_hash(
        &self,
        vault: EthAddress,
        block: AuthenticatedBlock,
    ) -> Result<Vec<EthLog>> {
        let topics = [
            deposit_received_topic0(),
            withdrawal_queued_topic0(),
            withdrawal_finalized_topic0(),
            withdrawal_cancelled_topic0(),
        ]
        .into_iter()
        .map(B256::from)
        .collect::<Vec<_>>();
        let filter = Filter::new()
            .at_block_hash(B256::from(block.hash))
            .address(Address::from(vault))
            .event_signature(topics);
        Ok(self.provider.get_logs(&filter).await?)
    }

    async fn deposit_root_by_count_at_hash(
        &self,
        vault: EthAddress,
        count: u64,
        block: AuthenticatedBlock,
    ) -> Result<Bytes32> {
        let call = SybilVault::depositRootByCountCall { count };
        let request = TransactionRequest::default()
            .to(Address::from(vault))
            .input(Bytes::from(call.abi_encode()).into());
        let output = self
            .provider
            .call(request)
            .hash_canonical(B256::from(block.hash))
            .await?;
        SybilVault::depositRootByCountCall::abi_decode_returns_validate(&output)
            .map(Into::into)
            .map_err(|error| IndexerError::AuthenticatedViewInvalid {
                context: "block-hash-pinned depositRootByCount ABI",
                block_number: block.number,
                provider: self.id.clone(),
                message: error.to_string(),
            })
    }
}

/// Unanimous authenticated view over one or more raw endpoints. This is not a
/// light client: public safety assumes at least one configured provider is
/// honest and that providers are independently operated. Unanimity then makes
/// one dishonest provider fail closed instead of fabricating bridge input.
pub(super) struct TrustedL1<E> {
    endpoints: Vec<E>,
    mode: RpcTrustMode,
    identity: SourceIdentity,
}

impl<E: L1Endpoint> TrustedL1<E> {
    fn from_endpoints(mut endpoints: Vec<E>, mode: RpcTrustMode) -> Result<Self> {
        endpoints.sort_by(|left, right| left.id().cmp(right.id()));
        let provider_ids = endpoints
            .iter()
            .map(|endpoint| endpoint.id().trim().to_string())
            .collect::<Vec<_>>();
        validate_provider_config(mode, &provider_ids)?;
        Ok(Self {
            endpoints,
            mode,
            identity: SourceIdentity::new(mode, provider_ids),
        })
    }

    pub(super) fn identity(&self) -> &SourceIdentity {
        &self.identity
    }

    async fn agreed_block_hash(&self, number: u64, context: &'static str) -> Result<Bytes32> {
        let first = &self.endpoints[0];
        let expected = first.block_hash(number).await?;
        for endpoint in &self.endpoints[1..] {
            let observed = endpoint.block_hash(number).await?;
            require_provider_agreement(
                context,
                number,
                first.id(),
                endpoint.id(),
                hex::encode(expected),
                hex::encode(observed),
            )?;
        }
        Ok(expected)
    }

    async fn authenticated_tip_inner(&self, confirmations: u64) -> Result<AuthenticatedBlock> {
        match self.mode {
            RpcTrustMode::UnsafeSingleDev => {
                let endpoint = &self.endpoints[0];
                let number = endpoint
                    .latest_number()
                    .await?
                    .saturating_sub(confirmations);
                let hash = endpoint.block_hash(number).await?;
                Ok(AuthenticatedBlock { number, hash })
            }
            RpcTrustMode::UnanimousFinalized => {
                // Providers may observe the next finalized checkpoint at slightly
                // different times. The lowest reported height is finalized for
                // all of them; its hash must still agree unanimously.
                let mut lowest = None;
                for endpoint in &self.endpoints {
                    let head = endpoint.finalized_block().await?;
                    lowest = Some(lowest.map_or(head.number, |value: u64| value.min(head.number)));
                }
                let number = lowest.ok_or_else(|| {
                    IndexerError::InvalidProviderConfig("no L1 providers configured".to_string())
                })?;
                let hash = self
                    .agreed_block_hash(number, "unanimous finalized prefix")
                    .await?;
                Ok(AuthenticatedBlock { number, hash })
            }
        }
    }

    async fn agreed_bridge_logs(
        &self,
        vault: EthAddress,
        block: AuthenticatedBlock,
    ) -> Result<Vec<EthLog>> {
        let first = &self.endpoints[0];
        let expected = normalized_logs(
            first.bridge_logs_at_hash(vault, block).await?,
            block,
            first.id(),
        )?;
        for endpoint in &self.endpoints[1..] {
            let observed = normalized_logs(
                endpoint.bridge_logs_at_hash(vault, block).await?,
                block,
                endpoint.id(),
            )?;
            require_provider_agreement(
                "block-hash-pinned vault logs",
                block.number,
                first.id(),
                endpoint.id(),
                logs_identity(&expected),
                logs_identity(&observed),
            )?;
        }
        Ok(expected)
    }
}

impl TrustedL1<HttpEndpoint> {
    pub(super) fn new(endpoints: Vec<HttpEndpoint>, mode: RpcTrustMode) -> Result<Self> {
        Self::from_endpoints(endpoints, mode)
    }
}

impl<E: L1Endpoint> L1Rpc for TrustedL1<E> {
    async fn authenticated_tip(&self, confirmations: u64) -> Result<AuthenticatedBlock> {
        self.authenticated_tip_inner(confirmations).await
    }

    async fn block_hash(&self, block_number: u64) -> Result<Bytes32> {
        self.agreed_block_hash(block_number, "canonical block header")
            .await
    }

    async fn bridge_logs(
        &self,
        vault: EthAddress,
        from_block: u64,
        to_block: u64,
    ) -> Result<(Vec<EthLog>, Vec<EthLog>)> {
        let mut deposits = Vec::new();
        let mut withdrawals = Vec::new();
        for number in from_block..=to_block {
            let block = AuthenticatedBlock {
                number,
                hash: self
                    .agreed_block_hash(number, "vault-log block header")
                    .await?,
            };
            for log in self.agreed_bridge_logs(vault, block).await? {
                match log.topic0().copied() {
                    Some(topic) if topic == B256::from(deposit_received_topic0()) => {
                        deposits.push(log);
                    }
                    Some(topic)
                        if topic == B256::from(withdrawal_queued_topic0())
                            || topic == B256::from(withdrawal_finalized_topic0())
                            || topic == B256::from(withdrawal_cancelled_topic0()) =>
                    {
                        withdrawals.push(log);
                    }
                    _ => {
                        return Err(IndexerError::AuthenticatedViewInvalid {
                            context: "vault log topic",
                            block_number: number,
                            provider: self.endpoints[0].id().to_string(),
                            message: "RPC returned a log outside the requested topic set"
                                .to_string(),
                        });
                    }
                }
            }
        }
        Ok((deposits, withdrawals))
    }

    async fn deposit_root_by_count(
        &self,
        vault: EthAddress,
        count: u64,
        block: AuthenticatedBlock,
    ) -> Result<Bytes32> {
        let first = &self.endpoints[0];
        let expected = first
            .deposit_root_by_count_at_hash(vault, count, block)
            .await?;
        for endpoint in &self.endpoints[1..] {
            let observed = endpoint
                .deposit_root_by_count_at_hash(vault, count, block)
                .await?;
            require_provider_agreement(
                "block-hash-pinned depositRootByCount",
                block.number,
                first.id(),
                endpoint.id(),
                hex::encode(expected),
                hex::encode(observed),
            )?;
        }
        Ok(expected)
    }
}

fn validate_provider_config(mode: RpcTrustMode, provider_ids: &[String]) -> Result<()> {
    if provider_ids.iter().any(|id| id.is_empty()) {
        return Err(IndexerError::InvalidProviderConfig(
            "provider ids must be non-empty operator-assigned names".to_string(),
        ));
    }
    let unique = provider_ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    if unique.len() != provider_ids.len() {
        return Err(IndexerError::InvalidProviderConfig(
            "provider ids must be unique".to_string(),
        ));
    }
    match (mode, provider_ids.len()) {
        (RpcTrustMode::UnsafeSingleDev, 1) => Ok(()),
        (RpcTrustMode::UnsafeSingleDev, count) => Err(IndexerError::InvalidProviderConfig(
            format!("unsafe-single-dev requires exactly one provider, got {count}"),
        )),
        (RpcTrustMode::UnanimousFinalized, count) if count >= 2 => Ok(()),
        (RpcTrustMode::UnanimousFinalized, count) => Err(IndexerError::InvalidProviderConfig(
            format!("unanimous-finalized requires at least two independent providers, got {count}"),
        )),
    }
}

fn normalized_logs(
    mut logs: Vec<EthLog>,
    block: AuthenticatedBlock,
    provider: &str,
) -> Result<Vec<EthLog>> {
    for log in &mut logs {
        if log.removed {
            return Err(IndexerError::AuthenticatedViewInvalid {
                context: "block-hash-pinned vault log",
                block_number: block.number,
                provider: provider.to_string(),
                message: "provider marked a finalized log as removed".to_string(),
            });
        }
        if log.block_number != Some(block.number) || log.block_hash != Some(B256::from(block.hash))
        {
            return Err(IndexerError::AuthenticatedViewInvalid {
                context: "block-hash-pinned vault log",
                block_number: block.number,
                provider: provider.to_string(),
                message: format!(
                    "log carried block_number={:?} block_hash={:?}, expected {} / {}",
                    log.block_number,
                    log.block_hash,
                    block.number,
                    hex::encode(block.hash)
                ),
            });
        }
        // blockTimestamp is a non-standard optional response extension and is
        // not part of receipt identity. Ignore it when comparing providers.
        log.block_timestamp = None;
    }
    logs.sort_by_key(|log| {
        (
            log.log_index.unwrap_or(u64::MAX),
            log.transaction_index.unwrap_or(u64::MAX),
            log.transaction_hash.unwrap_or_default(),
        )
    });
    Ok(logs)
}

fn logs_identity(logs: &[EthLog]) -> String {
    let bytes = serde_json::to_vec(logs).unwrap_or_default();
    format!(
        "count={} keccak256={}",
        logs.len(),
        hex::encode(alloy::primitives::keccak256(bytes))
    )
}

fn require_provider_agreement(
    context: &'static str,
    block_number: u64,
    expected_provider: &str,
    observed_provider: &str,
    expected: String,
    observed: String,
) -> Result<()> {
    if expected != observed {
        return Err(IndexerError::ProviderDisagreement {
            context,
            block_number,
            expected_provider: expected_provider.to_string(),
            observed_provider: observed_provider.to_string(),
            expected,
            observed,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{Log as PrimitiveLog, U256};
    use alloy::sol_types::SolEvent;
    use std::collections::HashMap;

    struct FakeEndpoint {
        id: String,
        latest: u64,
        finalized: AuthenticatedBlock,
        block_hashes: HashMap<u64, Bytes32>,
        logs: HashMap<u64, Vec<EthLog>>,
        roots: HashMap<(u64, u64), Bytes32>,
    }

    impl FakeEndpoint {
        fn canonical(id: &str) -> Self {
            Self {
                id: id.to_string(),
                latest: 100,
                finalized: AuthenticatedBlock {
                    number: 96,
                    hash: test_hash(96),
                },
                block_hashes: HashMap::new(),
                logs: HashMap::new(),
                roots: HashMap::new(),
            }
        }
    }

    impl L1Endpoint for FakeEndpoint {
        fn id(&self) -> &str {
            &self.id
        }

        async fn latest_number(&self) -> Result<u64> {
            Ok(self.latest)
        }

        async fn finalized_block(&self) -> Result<AuthenticatedBlock> {
            Ok(self.finalized)
        }

        async fn block_hash(&self, number: u64) -> Result<Bytes32> {
            Ok(self
                .block_hashes
                .get(&number)
                .copied()
                .unwrap_or_else(|| test_hash(number)))
        }

        async fn bridge_logs_at_hash(
            &self,
            _vault: EthAddress,
            block: AuthenticatedBlock,
        ) -> Result<Vec<EthLog>> {
            Ok(self.logs.get(&block.number).cloned().unwrap_or_default())
        }

        async fn deposit_root_by_count_at_hash(
            &self,
            _vault: EthAddress,
            count: u64,
            block: AuthenticatedBlock,
        ) -> Result<Bytes32> {
            Ok(self
                .roots
                .get(&(count, block.number))
                .copied()
                .unwrap_or_default())
        }
    }

    fn test_hash(number: u64) -> Bytes32 {
        let mut hash = [0x42; 32];
        hash[24..].copy_from_slice(&number.to_be_bytes());
        hash
    }

    fn deposit_log(block: u64, root: Bytes32) -> EthLog {
        let event = SybilVault::DepositReceived {
            depositId: 1,
            sender: Address::from([0x30; 20]),
            sybilAccountKey: B256::from([0x44; 32]),
            token: Address::from([0x20; 20]),
            amount: U256::from(1_000_000),
            depositRoot: B256::from(root),
        };
        EthLog {
            inner: PrimitiveLog {
                address: Address::from([0x10; 20]),
                data: event.encode_log_data(),
            },
            block_number: Some(block),
            block_hash: Some(B256::from(test_hash(block))),
            transaction_hash: Some(B256::from([0xaa; 32])),
            transaction_index: Some(1),
            log_index: Some(1),
            ..Default::default()
        }
    }

    #[test]
    fn finalized_mode_requires_two_distinct_provider_identities() {
        assert!(matches!(
            validate_provider_config(RpcTrustMode::UnanimousFinalized, &["one".to_string()]),
            Err(IndexerError::InvalidProviderConfig(_))
        ));
        assert!(matches!(
            validate_provider_config(
                RpcTrustMode::UnanimousFinalized,
                &["same".to_string(), "same".to_string()]
            ),
            Err(IndexerError::InvalidProviderConfig(_))
        ));
        assert!(
            validate_provider_config(
                RpcTrustMode::UnanimousFinalized,
                &["a".to_string(), "b".to_string()]
            )
            .is_ok()
        );
    }

    #[tokio::test]
    async fn self_consistent_fabricated_fork_is_fatal_when_one_provider_is_honest() {
        let honest = FakeEndpoint::canonical("independent-a");
        let mut fabricated = FakeEndpoint::canonical("fabricated-b");
        fabricated.block_hashes.insert(96, [0x99; 32]);
        fabricated.finalized.hash = [0x99; 32];
        let source =
            TrustedL1::from_endpoints(vec![honest, fabricated], RpcTrustMode::UnanimousFinalized)
                .unwrap();

        let error = source.authenticated_tip(0).await.unwrap_err();
        assert!(matches!(
            error,
            IndexerError::ProviderDisagreement {
                context: "unanimous finalized prefix",
                block_number: 96,
                ..
            }
        ));
        assert!(error.is_fatal());
    }

    #[tokio::test]
    async fn provider_log_omission_is_fatal_before_ingress() {
        let root = [0x55; 32];
        let mut with_log = FakeEndpoint::canonical("independent-a");
        with_log.logs.insert(90, vec![deposit_log(90, root)]);
        let without_log = FakeEndpoint::canonical("independent-b");
        let source = TrustedL1::from_endpoints(
            vec![with_log, without_log],
            RpcTrustMode::UnanimousFinalized,
        )
        .unwrap();

        let error = source.bridge_logs([0x10; 20], 90, 90).await.unwrap_err();
        assert!(matches!(
            error,
            IndexerError::ProviderDisagreement {
                context: "block-hash-pinned vault logs",
                block_number: 90,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn provider_contract_state_disagreement_is_fatal() {
        let mut first = FakeEndpoint::canonical("independent-a");
        first.roots.insert((1, 90), [0x55; 32]);
        let mut second = FakeEndpoint::canonical("independent-b");
        second.roots.insert((1, 90), [0x66; 32]);
        let source =
            TrustedL1::from_endpoints(vec![first, second], RpcTrustMode::UnanimousFinalized)
                .unwrap();

        let error = source
            .deposit_root_by_count(
                [0x10; 20],
                1,
                AuthenticatedBlock {
                    number: 90,
                    hash: test_hash(90),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            IndexerError::ProviderDisagreement {
                context: "block-hash-pinned depositRootByCount",
                block_number: 90,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn log_not_bound_to_requested_header_is_fatal() {
        let mut invalid = deposit_log(90, [0x55; 32]);
        invalid.block_hash = Some(B256::from([0x99; 32]));
        let mut first = FakeEndpoint::canonical("independent-a");
        first.logs.insert(90, vec![invalid]);
        let second = FakeEndpoint::canonical("independent-b");
        let source =
            TrustedL1::from_endpoints(vec![first, second], RpcTrustMode::UnanimousFinalized)
                .unwrap();

        let error = source.bridge_logs([0x10; 20], 90, 90).await.unwrap_err();
        assert!(matches!(
            error,
            IndexerError::AuthenticatedViewInvalid {
                context: "block-hash-pinned vault log",
                block_number: 90,
                ..
            }
        ));
    }
}
