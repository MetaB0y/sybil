use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use p256::ecdsa::{Signature, SigningKey, signature::Signer as _};
use serde::Deserialize;
use sybil_api_types::{
    QmdbStateExclusionProofResponse, QmdbStateInclusionProofResponse,
    QmdbStateOperationProofResponse, QmdbStateRangeProofResponse, StateProofResponse,
};
use sybil_escape_claim::{
    AccountReservationLeafWitness, EscapeClaimGuestInput, EscapeClaimPublicInputs,
    MarketLeafWitness, compute_withdrawable_token_units, escape_claim_public_input_hash,
    escape_nullifier, verify_escape_claim,
};
use sybil_verifier::commitments::state_schema;
use sybil_verifier::{AccountSnapshot, KeyOpAuth};
use sybil_zk::{
    QmdbStateExclusionProof, QmdbStateKeyValueProof, QmdbStateOperationProof, QmdbStateRangeProof,
};

use crate::abi::{adapter_proof_from_openvm_json, encode_adapter_proof, escape_claim_calldata};
use crate::format::{CUSTODY_SNAPSHOT_VERSION, CustodySnapshot};
use crate::rpc::{chain_id, decode20, decode32, fetch_root_record, latest_height};

pub struct ClaimRequest<'a> {
    pub snapshot: &'a CustodySnapshot,
    pub rpc_url: &'a str,
    pub settlement: [u8; 20],
    pub vault: [u8; 20],
    pub recipient: [u8; 20],
    pub p256_private_key: &'a str,
    pub work_dir: &'a Path,
    pub fixture_proof: bool,
}

pub struct ClaimArtifacts {
    pub input: EscapeClaimGuestInput,
    pub adapter_proof: Vec<u8>,
    pub calldata: Vec<u8>,
    pub guest_input_path: PathBuf,
    pub proof_path: Option<PathBuf>,
}

pub async fn assemble_claim(request: ClaimRequest<'_>) -> Result<ClaimArtifacts> {
    if request.snapshot.version != CUSTODY_SNAPSHOT_VERSION {
        bail!(
            "unsupported custody snapshot version {}",
            request.snapshot.version
        );
    }
    let latest = latest_height(request.rpc_url, request.settlement).await?;
    if latest != request.snapshot.block_height {
        bail!(
            "custody snapshot height {} is stale; settlement latest height is {latest}",
            request.snapshot.block_height
        );
    }
    let root_record = fetch_root_record(request.rpc_url, request.settlement, latest).await?;
    let snapshot_root = decode32("snapshot state_root", &request.snapshot.state_root)?;
    if root_record.state_root != snapshot_root {
        bail!("custody snapshot root is not the settlement's latest accepted root");
    }
    let chain_id = chain_id(request.rpc_url).await?;
    let input = input_from_snapshot(
        request.snapshot,
        chain_id,
        request.vault,
        request.recipient,
        request.p256_private_key,
    )?;
    verify_escape_claim(&input).context("locally verify assembled Form-L claim")?;

    std::fs::create_dir_all(request.work_dir)
        .with_context(|| format!("create {}", request.work_dir.display()))?;
    let guest_input_path = request.work_dir.join("escape-claim-input.msgpack");
    let guest_bytes = rmp_serde::to_vec(&input).context("encode escape guest input")?;
    std::fs::write(&guest_input_path, guest_bytes)
        .with_context(|| format!("write {}", guest_input_path.display()))?;

    let (adapter_proof, proof_path) = if request.fixture_proof {
        let public_values = escape_claim_public_input_hash(&input.public_inputs);
        let commitments = read_escape_commitments()?;
        (
            encode_adapter_proof(
                &public_values,
                &[1, 2, 3, 4],
                commitments.app_exe_commit,
                commitments.app_vm_commit,
            ),
            None,
        )
    } else {
        let proof = prove_and_verify(request.work_dir, &guest_input_path).await?;
        let bytes = std::fs::read(&proof)
            .with_context(|| format!("read OpenVM proof {}", proof.display()))?;
        (adapter_proof_from_openvm_json(&bytes)?, Some(proof))
    };
    let calldata = escape_claim_calldata(&input.public_inputs, &adapter_proof);
    Ok(ClaimArtifacts {
        input,
        adapter_proof,
        calldata,
        guest_input_path,
        proof_path,
    })
}

pub fn input_from_snapshot(
    snapshot: &CustodySnapshot,
    chain_id: u64,
    vault: [u8; 20],
    recipient: [u8; 20],
    private_key_hex: &str,
) -> Result<EscapeClaimGuestInput> {
    let state_root = decode32("snapshot state_root", &snapshot.state_root)?;
    let openings = form_l_openings(snapshot)?;
    let account = openings.account;
    let account_proof = openings.account_proof;
    let reservation = openings.reservation;
    let markets = openings.markets;
    let active_keys = snapshot.active_keys.clone();
    if sybil_verifier::account_keys_digest(account.id, active_keys.iter().copied())
        != account.keys_digest
    {
        bail!("snapshot active key list does not match committed keys_digest");
    }

    let signing = signing_key(private_key_hex)?;
    let signer_bytes: [u8; 33] = signing
        .verifying_key()
        .to_sec1_point(true)
        .as_bytes()
        .try_into()
        .expect("compressed P256 key is 33 bytes");
    if !active_keys
        .iter()
        .any(|key| key.auth_scheme == 0 && key.pubkey_sec1 == signer_bytes)
    {
        bail!("provided raw P256 key is not active for this account");
    }
    let reserved_balance = match &reservation {
        AccountReservationLeafWitness::Inclusion { reservation, .. } => {
            reservation.reserved_balance
        }
        AccountReservationLeafWitness::Exclusion { .. } => 0,
    };
    let amount =
        compute_withdrawable_token_units(&account, reserved_balance, &markets, &state_root)
            .context("compute proven withdrawable amount")?;
    let public_inputs = EscapeClaimPublicInputs {
        state_root,
        height: snapshot.block_height,
        account_id: snapshot.account_id,
        recipient,
        amount,
        nullifier: escape_nullifier(chain_id, vault, snapshot.account_id, state_root),
    };
    let genesis_hash = decode32("snapshot genesis_hash", &snapshot.genesis_hash)?;
    let canonical = sybil_verifier::canonical_escape_claim_bytes(
        genesis_hash,
        chain_id,
        vault,
        state_root,
        snapshot.block_height,
        snapshot.account_id,
        recipient,
        amount,
    );
    let signature: Signature = signing.sign(&canonical);
    Ok(EscapeClaimGuestInput {
        public_inputs,
        genesis_hash,
        chain_id,
        vault_address: vault,
        account,
        account_proof,
        account_reservation: reservation,
        markets,
        active_keys,
        authorization: KeyOpAuth::RawP256 {
            signer_pubkey: signer_bytes,
            signature: signature.to_bytes().into(),
        },
    })
}

struct FormLOpenings {
    account: AccountSnapshot,
    account_proof: QmdbStateKeyValueProof,
    reservation: AccountReservationLeafWitness,
    markets: Vec<MarketLeafWitness>,
}

fn form_l_openings(snapshot: &CustodySnapshot) -> Result<FormLOpenings> {
    if snapshot.account.id != snapshot.account_id {
        bail!("account opening id does not match custody snapshot");
    }
    require_opening_value(
        &snapshot.account_proof,
        &state_schema::account_leaf_value(&snapshot.account),
        "account",
    )?;
    let account_proof = inclusion_proof(&snapshot.account_proof, "account")?;
    let reservation = match (
        &snapshot.reservation,
        snapshot.reservation_proof.proof_kind.as_str(),
    ) {
        (Some(reservation), "inclusion") => {
            require_opening_value(
                &snapshot.reservation_proof,
                &state_schema::account_reservation_leaf_value(reservation),
                "account reservation",
            )?;
            AccountReservationLeafWitness::Inclusion {
                reservation: reservation.clone(),
                proof: inclusion_proof(&snapshot.reservation_proof, "account reservation")?,
            }
        }
        (None, "exclusion") => AccountReservationLeafWitness::Exclusion {
            proof: exclusion_proof(&snapshot.reservation_proof)?,
        },
        _ => bail!("reservation opening and qMDB proof kind disagree"),
    };
    if snapshot.markets.len() != snapshot.market_proofs.len() {
        bail!("market openings and proof counts differ");
    }
    let markets = snapshot
        .markets
        .iter()
        .zip(&snapshot.market_proofs)
        .map(|(market, proof)| {
            require_opening_value(proof, &state_schema::market_leaf_value(market), "market")?;
            Ok(MarketLeafWitness {
                market: market.clone(),
                proof: inclusion_proof(proof, "market")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(FormLOpenings {
        account: snapshot.account.clone(),
        account_proof,
        reservation,
        markets,
    })
}

pub fn snapshot_openings(
    snapshot: &CustodySnapshot,
) -> Result<(AccountSnapshot, i64, Vec<MarketLeafWitness>)> {
    let openings = form_l_openings(snapshot)?;
    let reserved_balance = match &openings.reservation {
        AccountReservationLeafWitness::Inclusion { reservation, .. } => {
            reservation.reserved_balance
        }
        AccountReservationLeafWitness::Exclusion { .. } => 0,
    };
    Ok((openings.account, reserved_balance, openings.markets))
}

fn signing_key(value: &str) -> Result<SigningKey> {
    let bytes = hex::decode(value.trim_start_matches("0x")).context("decode P256 private key")?;
    SigningKey::from_slice(&bytes).context("invalid P256 private key")
}

fn inclusion_value(proof: &StateProofResponse, name: &str) -> Result<Vec<u8>> {
    if proof.proof_kind != "inclusion" || !proof.verified {
        bail!("{name} is not a verified inclusion proof");
    }
    hex::decode(
        proof
            .leaf_value_hex
            .as_deref()
            .with_context(|| format!("{name} inclusion omitted value"))?,
    )
    .with_context(|| format!("decode {name} value"))
}

fn require_opening_value(proof: &StateProofResponse, expected: &[u8], name: &str) -> Result<()> {
    if inclusion_value(proof, name)? != expected {
        bail!("{name} opening does not match canonical proof value");
    }
    Ok(())
}

fn inclusion_proof(proof: &StateProofResponse, name: &str) -> Result<QmdbStateKeyValueProof> {
    let inclusion = proof
        .inclusion_proof
        .as_ref()
        .with_context(|| format!("{name} inclusion omitted qMDB proof"))?;
    convert_inclusion(inclusion)
}

fn convert_inclusion(proof: &QmdbStateInclusionProofResponse) -> Result<QmdbStateKeyValueProof> {
    Ok(QmdbStateKeyValueProof {
        operation: convert_operation(&proof.operation)?,
        next_key: hex::decode(&proof.next_key_hex).context("decode qMDB next key")?,
    })
}

fn exclusion_proof(proof: &StateProofResponse) -> Result<QmdbStateExclusionProof> {
    let proof = proof
        .exclusion_proof
        .as_ref()
        .context("exclusion response omitted qMDB proof")?;
    convert_exclusion(proof)
}

fn convert_exclusion(proof: &QmdbStateExclusionProofResponse) -> Result<QmdbStateExclusionProof> {
    match proof.variant.as_str() {
        "key_value" => Ok(QmdbStateExclusionProof::KeyValue {
            operation: convert_operation(&proof.operation)?,
            span_key: decode_required(&proof.span_key_hex, "span_key")?,
            span_value: decode_required(&proof.span_value_hex, "span_value")?,
            span_next_key: decode_required(&proof.span_next_key_hex, "span_next_key")?,
        }),
        "commit" => Ok(QmdbStateExclusionProof::Commit {
            operation: convert_operation(&proof.operation)?,
            metadata: proof
                .metadata_hex
                .as_deref()
                .map(hex::decode)
                .transpose()
                .context("decode qMDB exclusion metadata")?,
        }),
        other => bail!("unknown qMDB exclusion variant {other}"),
    }
}

fn convert_operation(proof: &QmdbStateOperationProofResponse) -> Result<QmdbStateOperationProof> {
    let chunk = hex::decode(&proof.activity_chunk_hex).context("decode qMDB activity chunk")?;
    Ok(QmdbStateOperationProof {
        location: proof.location,
        activity_chunk: chunk.try_into().map_err(|bytes: Vec<u8>| {
            anyhow::anyhow!("qMDB activity chunk must be 32 bytes, got {}", bytes.len())
        })?,
        range: convert_range(&proof.range)?,
    })
}

fn convert_range(proof: &QmdbStateRangeProofResponse) -> Result<QmdbStateRangeProof> {
    Ok(QmdbStateRangeProof {
        leaves: proof.leaves,
        inactive_peaks: proof.inactive_peaks,
        digests: proof
            .digests_hex
            .iter()
            .map(|value| decode32("qMDB digest", value))
            .collect::<Result<Vec<_>>>()?,
        partial_chunk_digest: proof
            .partial_chunk_digest_hex
            .as_deref()
            .map(|value| decode32("qMDB partial chunk digest", value))
            .transpose()?,
        ops_root: decode32("qMDB ops root", &proof.ops_root_hex)?,
    })
}

fn decode_required(value: &Option<String>, field: &str) -> Result<Vec<u8>> {
    hex::decode(
        value
            .as_deref()
            .with_context(|| format!("missing {field}"))?,
    )
    .with_context(|| format!("decode {field}"))
}

#[derive(Deserialize)]
struct CommitmentsFile {
    app_exe_commit: String,
    app_vm_commit: String,
}

struct Commitments {
    app_exe_commit: [u8; 32],
    app_vm_commit: [u8; 32],
}

fn read_escape_commitments() -> Result<Commitments> {
    let root = workspace_root();
    let path =
        root.join("zk/openvm-escape-guest/openvm/release/sybil-openvm-escape-guest.commit.json");
    let file: CommitmentsFile = serde_json::from_slice(
        &std::fs::read(&path).with_context(|| format!("read {}", path.display()))?,
    )
    .context("decode escape guest commitments")?;
    Ok(Commitments {
        app_exe_commit: decode32("app_exe_commit", &file.app_exe_commit)?,
        app_vm_commit: decode32("app_vm_commit", &file.app_vm_commit)?,
    })
}

async fn prove_and_verify(work_dir: &Path, guest_input: &Path) -> Result<PathBuf> {
    let root = workspace_root();
    let openvm_input = work_dir.join("escape-claim-openvm-input.json");
    let proof = work_dir.join("escape-claim-openvm-evm-proof.json");
    run(
        tokio::process::Command::new("cargo")
            .current_dir(&root)
            .args(["run", "--quiet", "--manifest-path"])
            .arg(root.join("zk/openvm-tools/Cargo.toml"))
            .args(["--", "encode-escape-input", "--guest-input"])
            .arg(guest_input)
            .arg("--openvm-input")
            .arg(&openvm_input),
        "encode OpenVM escape input",
    )
    .await?;
    run(
        tokio::process::Command::new("cargo")
            .current_dir(&root)
            .args(["openvm", "prove", "evm", "--manifest-path"])
            .arg(root.join("zk/openvm-escape-guest/Cargo.toml"))
            .arg("--config")
            .arg(root.join("zk/openvm-escape-guest/openvm.toml"))
            .arg("--output-dir")
            .arg(root.join("target/openvm/sybil-escape"))
            .arg("--input")
            .arg(&openvm_input)
            .arg("--proof")
            .arg(&proof),
        "prove OpenVM escape claim",
    )
    .await?;
    run(
        tokio::process::Command::new("cargo")
            .current_dir(&root)
            .args(["openvm", "verify", "evm", "--proof"])
            .arg(&proof),
        "verify OpenVM escape proof",
    )
    .await?;
    Ok(proof)
}

async fn run(command: &mut tokio::process::Command, what: &str) -> Result<()> {
    let output = command.output().await.with_context(|| what.to_string())?;
    if !output.status.success() {
        bail!(
            "{what} failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("custody crate lives under workspace/crates")
        .to_path_buf()
}

pub fn parse_address(name: &str, value: &str) -> Result<[u8; 20]> {
    decode20(name, value)
}
