use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Args, Subcommand};
use matching_engine::MarketSet;
use matching_sequencer::store::Store;
use matching_sequencer::{AccountStore, AdminOracle, BlockSequencer, SequencerConfig};

use crate::{collect_state_transition_proof_job, StateTransitionProofJobId};

#[derive(Args)]
pub struct WitgenArgs {
    #[command(subcommand)]
    pub command: WitgenCommand,
}

#[derive(Subcommand)]
pub enum WitgenCommand {
    /// Export the latest committed block as a state-transition proof job.
    ExportLatest(ExportLatestArgs),
    /// Create a one-block local smoke fixture and export its proof job.
    SmokeJob(SmokeJobArgs),
    /// Create a keyed, traded devnet state and export a Form-L claim input.
    EscapeSmoke(EscapeSmokeArgs),
}

#[derive(Args)]
pub struct EscapeSmokeArgs {
    /// Path to the sequencer redb store to create.
    #[arg(long)]
    store: PathBuf,
    /// Output path for the MessagePack-encoded EscapeClaimGuestInput.
    #[arg(long)]
    guest_input: PathBuf,
    /// Timestamp to use for the fixture block.
    #[arg(long, default_value_t = 1_000)]
    timestamp_ms: u64,
}

#[derive(Args)]
pub struct ExportLatestArgs {
    /// Path to the sequencer redb store, usually data/sybil.redb.
    #[arg(long)]
    store: PathBuf,
    /// Output path for the MessagePack-encoded StateTransitionProofJob.
    #[arg(long)]
    job: PathBuf,
}

#[derive(Args)]
pub struct SmokeJobArgs {
    /// Path to the sequencer redb store to create.
    #[arg(long)]
    store: PathBuf,
    /// Output path for the MessagePack-encoded StateTransitionProofJob.
    #[arg(long)]
    job: PathBuf,
    /// Timestamp to use for the single smoke block.
    #[arg(long, default_value_t = 1_000)]
    timestamp_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum WitgenCliError {
    #[error("sequencer store does not exist: {path}")]
    StoreNotFound { path: PathBuf },
    #[error("refusing to overwrite existing smoke store: {path}")]
    SmokeStoreExists { path: PathBuf },
    #[error("open sequencer store {path}: {source}")]
    OpenStore {
        path: PathBuf,
        #[source]
        source: matching_sequencer::store::StoreError,
    },
    #[error("sequencer store has no persisted latest block witness")]
    MissingLatestWitness,
    #[error("collect proof job: {0}")]
    CollectProofJob(#[from] crate::SequencerStoreWitgenError),
    #[error("encode MessagePack proof job for {path}: {source}")]
    Encode {
        path: PathBuf,
        #[source]
        source: rmp_serde::encode::Error,
    },
    #[error("write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("read latest block witness: {0}")]
    ReadWitness(#[source] matching_sequencer::store::StoreError),
    #[error("persist smoke block: {0}")]
    PersistSmokeBlock(#[source] matching_sequencer::store::StoreError),
    #[error("escape smoke fixture: {0}")]
    EscapeSmoke(String),
}

pub async fn run(args: WitgenArgs) -> Result<(), WitgenCliError> {
    match args.command {
        WitgenCommand::ExportLatest(args) => export_latest(args).await,
        WitgenCommand::SmokeJob(args) => smoke_job(args).await,
        WitgenCommand::EscapeSmoke(args) => escape_smoke(args).await,
    }
}

async fn escape_smoke(args: EscapeSmokeArgs) -> Result<(), WitgenCliError> {
    use matching_engine::{outcome_buy, outcome_sell, NANOS_PER_DOLLAR};
    use matching_sequencer::crypto::PublicKey;
    use matching_sequencer::OrderSubmission;
    use p256::ecdsa::signature::Signer as _;
    use p256::ecdsa::{Signature, SigningKey};
    use sybil_escape_claim::{
        compute_withdrawable_token_units, escape_nullifier, AccountReservationLeafWitness,
        EscapeClaimGuestInput, EscapeClaimPublicInputs, MarketLeafWitness,
    };
    use sybil_verifier::commitments::state_schema;

    if args.store.exists() {
        return Err(WitgenCliError::SmokeStoreExists {
            path: args.store.clone(),
        });
    }
    let store = Store::open(&args.store).map_err(|source| WitgenCliError::OpenStore {
        path: args.store.clone(),
        source,
    })?;

    let mut markets = MarketSet::new();
    let market_id = markets.add_binary("Form-L exported-state smoke");
    let mut accounts = AccountStore::new();
    let claimant = accounts.create_account(2 * NANOS_PER_DOLLAR as i64);
    let seller = accounts.create_account(0);
    accounts
        .get_mut(seller)
        .ok_or_else(|| WitgenCliError::EscapeSmoke("seller account missing".to_string()))?
        .positions
        .insert((market_id, 0), 1_000);

    let oracle = Arc::new(AdminOracle::new());
    let mut sequencer = BlockSequencer::with_default_solver(
        accounts,
        markets.clone(),
        vec![],
        oracle,
        SequencerConfig::default(),
    );
    let signing = SigningKey::from_slice(&[0x31; 32])
        .map_err(|error| WitgenCliError::EscapeSmoke(error.to_string()))?;
    sequencer
        .register_pubkey(claimant, PublicKey(*signing.verifying_key()))
        .map_err(|error| WitgenCliError::EscapeSmoke(error.to_string()))?;
    let production = sequencer.produce_block(
        vec![
            OrderSubmission {
                account_id: claimant,
                orders: vec![outcome_buy(&markets, 0, market_id, 0, 700_000_000, 1_000)],
                mm_constraint: None,
            },
            OrderSubmission {
                account_id: seller,
                orders: vec![outcome_sell(&markets, 0, market_id, 0, 300_000_000, 1_000)],
                mm_constraint: None,
            },
        ],
        args.timestamp_ms,
    );
    if production.block.fills.len() != 2 {
        return Err(WitgenCliError::EscapeSmoke(
            "fixture trade did not clear both crossing orders".to_string(),
        ));
    }
    store
        .save_block_with_witness(sequencer.snapshot(), &production.witness)
        .await
        .map_err(WitgenCliError::PersistSmokeBlock)?;

    let witness = &production.witness;
    let root = store
        .current_state_qmdb_root()
        .await
        .map_err(|error| WitgenCliError::EscapeSmoke(error.to_string()))?
        .ok_or_else(|| WitgenCliError::EscapeSmoke("fixture qMDB root missing".to_string()))?;
    if root.root != witness.header.state_root {
        return Err(WitgenCliError::EscapeSmoke(
            "fixture qMDB root does not match block header".to_string(),
        ));
    }
    let account = witness
        .post_state
        .iter()
        .find(|account| account.id == claimant.0)
        .cloned()
        .ok_or_else(|| WitgenCliError::EscapeSmoke("claimant snapshot missing".to_string()))?;
    let market = witness
        .state_sidecar
        .markets
        .iter()
        .find(|market| market.market_id == market_id)
        .cloned()
        .ok_or_else(|| WitgenCliError::EscapeSmoke("market snapshot missing".to_string()))?;
    if market.last_clearing_prices.is_empty() {
        return Err(WitgenCliError::EscapeSmoke(
            "fixture market did not export its clearing prices".to_string(),
        ));
    }
    let account_key = state_schema::account_leaf_key(claimant.0);
    let account_proof = store
        .state_qmdb_leaf_proof(root.slot, &account_key)
        .await
        .map_err(|error| WitgenCliError::EscapeSmoke(error.to_string()))?
        .ok_or_else(|| WitgenCliError::EscapeSmoke("account proof missing".to_string()))?;
    if !account_proof.verify() {
        return Err(WitgenCliError::EscapeSmoke(
            "native account proof verification failed".to_string(),
        ));
    }
    let market_key = state_schema::market_leaf_key(market_id);
    let market_proof = store
        .state_qmdb_leaf_proof(root.slot, &market_key)
        .await
        .map_err(|error| WitgenCliError::EscapeSmoke(error.to_string()))?
        .ok_or_else(|| WitgenCliError::EscapeSmoke("market proof missing".to_string()))?;
    if !market_proof.verify() {
        return Err(WitgenCliError::EscapeSmoke(
            "native market proof verification failed".to_string(),
        ));
    }
    let reservation_key = state_schema::account_reservation_leaf_key(claimant.0);
    let reservation_proof = store
        .state_qmdb_leaf_exclusion_proof(root.slot, &reservation_key)
        .await
        .map_err(|error| WitgenCliError::EscapeSmoke(error.to_string()))?
        .ok_or_else(|| {
            WitgenCliError::EscapeSmoke("reservation exclusion proof missing".to_string())
        })?;
    if !reservation_proof.verify() {
        return Err(WitgenCliError::EscapeSmoke(
            "native reservation exclusion proof verification failed".to_string(),
        ));
    }
    let active_keys = witness
        .account_keys
        .iter()
        .find(|(account_id, _)| *account_id == claimant.0)
        .map(|(_, keys)| keys.clone())
        .ok_or_else(|| WitgenCliError::EscapeSmoke("claimant key opening missing".to_string()))?;
    let key = active_keys
        .first()
        .copied()
        .ok_or_else(|| WitgenCliError::EscapeSmoke("claimant key set empty".to_string()))?;

    let chain_id = 31_337;
    let vault_address = [0x44; 20];
    let recipient = [0x55; 20];
    let market_witness = MarketLeafWitness {
        market,
        proof: market_proof.proof_parts(),
    };
    let amount = compute_withdrawable_token_units(
        &account,
        0,
        core::slice::from_ref(&market_witness),
        &root.root,
    )
    .map_err(|error| WitgenCliError::EscapeSmoke(error.to_string()))?;
    let public_inputs = EscapeClaimPublicInputs {
        state_root: root.root,
        height: witness.header.height,
        account_id: claimant.0,
        recipient,
        amount,
        nullifier: escape_nullifier(chain_id, vault_address, claimant.0, root.root),
    };
    let canonical = sybil_verifier::canonical_escape_claim_bytes(
        witness.genesis_hash,
        chain_id,
        vault_address,
        root.root,
        witness.header.height,
        claimant.0,
        recipient,
        amount,
    );
    let signature: Signature = signing.sign(&canonical);
    let input = EscapeClaimGuestInput {
        public_inputs,
        genesis_hash: witness.genesis_hash,
        chain_id,
        vault_address,
        account,
        account_proof: account_proof.proof_parts(),
        account_reservation: AccountReservationLeafWitness::Exclusion {
            proof: reservation_proof.proof_parts(),
        },
        markets: vec![market_witness],
        active_keys,
        authorization: sybil_verifier::KeyOpAuth::RawP256 {
            signer_pubkey: key.pubkey_sec1,
            signature: signature.to_bytes().into(),
        },
    };
    sybil_escape_claim::verify_escape_claim(&input)
        .map_err(|error| WitgenCliError::EscapeSmoke(error.to_string()))?;
    write_msgpack_named(&args.guest_input, &input)?;

    println!("block_height={}", witness.header.height);
    println!("state_root=0x{}", hex::encode(root.root));
    println!("account_id={}", claimant.0);
    println!("amount_token_units={amount}");
    println!("store={}", args.store.display());
    println!("guest_input={}", args.guest_input.display());
    Ok(())
}

async fn export_latest(args: ExportLatestArgs) -> Result<(), WitgenCliError> {
    if !args.store.exists() {
        return Err(WitgenCliError::StoreNotFound {
            path: args.store.clone(),
        });
    }

    let store = Store::open(&args.store).map_err(|source| WitgenCliError::OpenStore {
        path: args.store.clone(),
        source,
    })?;
    let witness = store
        .latest_block_witness()
        .map_err(WitgenCliError::ReadWitness)?
        .ok_or(WitgenCliError::MissingLatestWitness)?;
    let job = collect_state_transition_proof_job(&store, witness).await?;
    let job_id = job.id();

    write_msgpack_named(&args.job, &job)?;

    print_job_id(&job_id);
    println!("state_leaf_proofs={}", job.state_leaf_proofs.len());
    println!("job={}", args.job.display());
    Ok(())
}

async fn smoke_job(args: SmokeJobArgs) -> Result<(), WitgenCliError> {
    if args.store.exists() {
        return Err(WitgenCliError::SmokeStoreExists {
            path: args.store.clone(),
        });
    }

    let store = Store::open(&args.store).map_err(|source| WitgenCliError::OpenStore {
        path: args.store.clone(),
        source,
    })?;

    let accounts = AccountStore::new();
    let markets = MarketSet::new();
    let oracle = Arc::new(AdminOracle::new());
    let mut sequencer = BlockSequencer::with_default_solver(
        accounts,
        markets,
        vec![],
        oracle,
        SequencerConfig::default(),
    );
    let production = sequencer.produce_block(vec![], args.timestamp_ms);
    store
        .save_block_with_witness(sequencer.snapshot(), &production.witness)
        .await
        .map_err(WitgenCliError::PersistSmokeBlock)?;

    let job = collect_state_transition_proof_job(&store, production.witness).await?;
    write_msgpack_named(&args.job, &job)?;

    print_job_id(&job.id());
    println!("state_leaf_proofs={}", job.state_leaf_proofs.len());
    println!("store={}", args.store.display());
    println!("job={}", args.job.display());
    Ok(())
}

fn write_msgpack_named<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), WitgenCliError> {
    let bytes = rmp_serde::to_vec_named(value).map_err(|source| WitgenCliError::Encode {
        path: path.to_path_buf(),
        source,
    })?;
    std::fs::write(path, bytes).map_err(|source| WitgenCliError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn print_job_id(job_id: &StateTransitionProofJobId) {
    println!("block_height={}", job_id.block_height);
    println!("block_hash=0x{}", hex::encode(job_id.block_hash));
    println!("state_root=0x{}", hex::encode(job_id.state_root));
}
