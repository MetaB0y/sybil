use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use sybil_proof_protocol::{ProofEnvelope, ProofKind, proof_payload_digest};
use uuid::Uuid;

use super::DaemonError;
use super::model::{EpochRecord, ProofBackendKind};

#[derive(Clone, Debug)]
pub struct BackendProof {
    pub envelope: ProofEnvelope,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct BackendFailure {
    pub message: String,
    pub permanent: bool,
}

#[derive(Clone, Debug)]
pub enum ProofBackend {
    Mock { pins: GuestPins },
    Stark(OpenVmStarkConfig),
}

impl ProofBackend {
    pub fn new(
        kind: ProofBackendKind,
        pins: GuestPins,
        stark: OpenVmStarkConfig,
        enable_evm: bool,
    ) -> Result<Self, DaemonError> {
        match kind {
            ProofBackendKind::Mock => Ok(Self::Mock { pins }),
            ProofBackendKind::Stark => Ok(Self::Stark(stark)),
            ProofBackendKind::Evm if !enable_evm => Err(DaemonError::Config(
                "EVM proving is disabled; use STARK now and implement GitHub issue #13 before enabling it"
                    .to_string(),
            )),
            ProofBackendKind::Evm => Err(DaemonError::Config(
                "EVM backend is intentionally not implemented in the STARK-first daemon"
                    .to_string(),
            )),
        }
    }

    pub async fn prove(
        &self,
        epoch: &EpochRecord,
        inputs: &[sybil_zk::StateTransitionGuestInput],
        owner: Uuid,
        attempt: u32,
        created_at_ms: u64,
    ) -> Result<BackendProof, BackendFailure> {
        match self {
            Self::Mock { pins } => {
                let proof = crate::mock_backend::prove_mock_epoch(
                    inputs,
                    pins.app_exe_commit,
                    pins.app_vm_commit,
                    created_at_ms,
                )
                .map_err(|error| BackendFailure {
                    message: error.to_string(),
                    permanent: true,
                })?;
                if proof.envelope.public_inputs != epoch.public_inputs {
                    return Err(BackendFailure {
                        message: "mock backend recomputed different epoch public inputs"
                            .to_string(),
                        permanent: true,
                    });
                }
                Ok(BackendProof {
                    envelope: proof.envelope,
                    payload: proof.payload,
                })
            }
            Self::Stark(config) => {
                config
                    .prove(epoch, inputs, owner, attempt, created_at_ms)
                    .await
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct OpenVmStarkConfig {
    pub pins: GuestPins,
    pub artifact_root: PathBuf,
    pub tools_manifest: PathBuf,
    pub guest_manifest: PathBuf,
    pub guest_config: PathBuf,
    pub output_dir: PathBuf,
    pub command_timeout: Duration,
    pub memory_limit_mib: u64,
}

impl OpenVmStarkConfig {
    async fn prove(
        &self,
        epoch: &EpochRecord,
        inputs: &[sybil_zk::StateTransitionGuestInput],
        owner: Uuid,
        attempt: u32,
        created_at_ms: u64,
    ) -> Result<BackendProof, BackendFailure> {
        let work_dir = self.artifact_root.join(".tmp").join(format!(
            "backend-{}-{owner}-{attempt}",
            hex::encode(epoch.epoch_id.0)
        ));
        if work_dir.exists() {
            std::fs::remove_dir_all(&work_dir).map_err(retryable_io)?;
        }
        std::fs::create_dir_all(&work_dir).map_err(retryable_io)?;
        let _work_dir_guard = AttemptWorkDirGuard(work_dir.clone());

        let mut guest_paths = Vec::with_capacity(inputs.len());
        for (index, input) in inputs.iter().enumerate() {
            let path = work_dir.join(format!("block-{index:04}.msgpack"));
            let bytes = rmp_serde::to_vec_named(input).map_err(|error| BackendFailure {
                message: format!("encode prepared guest input: {error}"),
                permanent: true,
            })?;
            std::fs::write(&path, bytes).map_err(retryable_io)?;
            guest_paths.push(path);
        }
        let openvm_input = work_dir.join("epoch-input.json");
        let proof_path = work_dir.join("epoch.app.proof");

        let mut encode_args = vec![
            OsString::from("run"),
            OsString::from("--quiet"),
            OsString::from("--manifest-path"),
            self.tools_manifest.as_os_str().to_owned(),
            OsString::from("--"),
            OsString::from("encode-epoch-input"),
        ];
        for path in &guest_paths {
            encode_args.push(OsString::from("--guest-input"));
            encode_args.push(path.as_os_str().to_owned());
        }
        encode_args.push(OsString::from("--openvm-input"));
        encode_args.push(openvm_input.as_os_str().to_owned());
        let encoded = run_limited(
            "cargo",
            &encode_args,
            self.command_timeout,
            self.memory_limit_mib,
        )
        .await
        .map_err(|message| BackendFailure {
            message: format!("prepare OpenVM epoch input: {message}"),
            permanent: true,
        })?;
        let expected_hash = format!(
            "public_input_hash=0x{}",
            hex::encode(sybil_proof_protocol::epoch_transition_public_input_hash(
                &epoch.public_inputs
            ))
        );
        if !encoded.contains(&expected_hash) {
            return Err(BackendFailure {
                message: format!(
                    "OpenVM input encoder did not report expected public input hash {expected_hash}"
                ),
                permanent: true,
            });
        }

        let prove_args = vec![
            OsString::from("openvm"),
            OsString::from("prove"),
            OsString::from("app"),
            OsString::from("--manifest-path"),
            self.guest_manifest.as_os_str().to_owned(),
            OsString::from("--config"),
            self.guest_config.as_os_str().to_owned(),
            OsString::from("--output-dir"),
            self.output_dir.as_os_str().to_owned(),
            OsString::from("--input"),
            openvm_input.as_os_str().to_owned(),
            OsString::from("--proof"),
            proof_path.as_os_str().to_owned(),
        ];
        run_limited(
            "cargo",
            &prove_args,
            self.command_timeout,
            self.memory_limit_mib,
        )
        .await
        .map_err(|message| BackendFailure {
            message: format!("OpenVM STARK prove failed: {message}"),
            permanent: false,
        })?;

        let verify_args = vec![
            OsString::from("openvm"),
            OsString::from("verify"),
            OsString::from("app"),
            OsString::from("--manifest-path"),
            self.guest_manifest.as_os_str().to_owned(),
            OsString::from("--proof"),
            proof_path.as_os_str().to_owned(),
        ];
        run_limited(
            "cargo",
            &verify_args,
            self.command_timeout,
            self.memory_limit_mib,
        )
        .await
        .map_err(|message| BackendFailure {
            message: format!("local OpenVM STARK verification failed: {message}"),
            permanent: false,
        })?;

        let payload = std::fs::read(&proof_path).map_err(retryable_io)?;
        if payload.is_empty() {
            return Err(BackendFailure {
                message: "OpenVM emitted an empty STARK proof".to_string(),
                permanent: false,
            });
        }
        let envelope = ProofEnvelope::new(
            ProofKind::OpenVmStark,
            epoch.public_inputs.clone(),
            self.pins.app_exe_commit,
            self.pins.app_vm_commit,
            proof_payload_digest(&payload),
            payload.len() as u64,
            created_at_ms,
        );
        envelope
            .validate_payload(&payload)
            .map_err(|error| BackendFailure {
                message: error.to_string(),
                permanent: true,
            })?;
        Ok(BackendProof { envelope, payload })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuestPins {
    pub app_exe_commit: [u8; 32],
    pub app_vm_commit: [u8; 32],
}

#[derive(Deserialize)]
struct GuestPinsJson {
    app_exe_commit: String,
    app_vm_commit: String,
}

impl GuestPins {
    pub fn read(path: &Path) -> Result<Self, DaemonError> {
        let bytes = std::fs::read(path)?;
        let pins: GuestPinsJson = serde_json::from_slice(&bytes)?;
        Ok(Self {
            app_exe_commit: decode_commit("app_exe_commit", &pins.app_exe_commit)?,
            app_vm_commit: decode_commit("app_vm_commit", &pins.app_vm_commit)?,
        })
    }
}

fn decode_commit(field: &'static str, value: &str) -> Result<[u8; 32], DaemonError> {
    let normalized = value.strip_prefix("0x").unwrap_or(value);
    let bytes = hex::decode(normalized)
        .map_err(|error| DaemonError::Config(format!("invalid {field}: {error}")))?;
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        DaemonError::Config(format!("{field} must be 32 bytes, got {}", bytes.len()))
    })
}

async fn run_limited(
    program: &str,
    args: &[OsString],
    timeout: Duration,
    memory_limit_mib: u64,
) -> Result<String, String> {
    let mut command = if memory_limit_mib == 0 {
        let mut command = tokio::process::Command::new(program);
        command.args(args);
        command
    } else {
        let bytes = memory_limit_mib.saturating_mul(1024 * 1024);
        let mut command = tokio::process::Command::new("prlimit");
        command
            .arg(format!("--as={bytes}"))
            .arg("--")
            .arg(program)
            .args(args);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);

    let child = command.spawn().map_err(|error| error.to_string())?;
    let mut process_group = ProcessGroupGuard::new(child.id());
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| format!("command timed out after {}s", timeout.as_secs()))?
        .map_err(|error| error.to_string())?;
    process_group.disarm();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(format!(
            "exit={} stdout={} stderr={}",
            output.status,
            bounded_output(&stdout),
            bounded_output(&stderr)
        ));
    }
    Ok(format!("{stdout}\n{stderr}"))
}

/// Tokio's `kill_on_drop` terminates the direct child. OpenVM is launched
/// through Cargo and can have descendants, so cancellation also signals the
/// dedicated Unix process group. This guard lives across the await and runs on
/// timeout, daemon shutdown, or task cancellation.
struct ProcessGroupGuard {
    pid: Option<u32>,
}

impl ProcessGroupGuard {
    const fn new(pid: Option<u32>) -> Self {
        Self { pid }
    }

    fn disarm(&mut self) {
        self.pid = None;
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            let _ = std::process::Command::new("kill")
                .args(["-TERM", "--", &format!("-{pid}")])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

fn bounded_output(output: &str) -> &str {
    const MAX: usize = 8 * 1024;
    if output.len() <= MAX {
        return output;
    }
    let mut start = output.len() - MAX;
    while !output.is_char_boundary(start) {
        start += 1;
    }
    &output[start..]
}

fn retryable_io(error: std::io::Error) -> BackendFailure {
    BackendFailure {
        message: error.to_string(),
        permanent: false,
    }
}

struct AttemptWorkDirGuard(PathBuf);

impl Drop for AttemptWorkDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
