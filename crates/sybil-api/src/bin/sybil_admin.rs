use std::fmt::{Display, Formatter};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args, Parser, Subcommand};
use matching_engine::NANOS_PER_DOLLAR;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sybil_api_types::request::{CreateMarketRequest, ResolveMarketRequest};
use sybil_api_types::response::{CreateMarketResponse, ResolveMarketResponse};

#[derive(Parser, Debug)]
#[command(name = "sybil-admin", about = "Admin CLI for Sybil market curation")]
struct Cli {
    #[arg(
        long,
        env = "SYBIL_API_URL",
        default_value = "http://127.0.0.1:3001",
        help = "Base URL for the sybil-api server"
    )]
    api_url: String,

    #[arg(
        long,
        env = "SYBIL_ADMIN_AUDIT_LOG",
        help = "Path to the append-only JSONL audit log"
    )]
    audit_log: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Market(MarketArgs),
    Audit(AuditArgs),
}

#[derive(Args, Debug)]
struct MarketArgs {
    #[command(subcommand)]
    command: MarketCommand,
}

#[derive(Subcommand, Debug)]
enum MarketCommand {
    /// Create a market from a YAML spec file.
    Create {
        #[arg(long, help = "Path to YAML spec matching CreateMarketRequest")]
        file: PathBuf,
    },
    /// Resolve a market with YES/NO/fractional payout.
    Resolve {
        #[arg(long)]
        market_id: u32,
        #[arg(long, conflicts_with_all = ["no", "payout_nanos"])]
        yes: bool,
        #[arg(long, conflicts_with_all = ["yes", "payout_nanos"])]
        no: bool,
        #[arg(long, help = "YES payout in nanos (0..=1_000_000_000)")]
        payout_nanos: Option<u64>,
    },
}

#[derive(Args, Debug)]
struct AuditArgs {
    #[command(subcommand)]
    command: AuditCommand,
}

#[derive(Subcommand, Debug)]
enum AuditCommand {
    /// Show the most recent audit entries.
    Tail {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}

#[derive(Debug)]
enum CliError {
    Io(std::io::Error),
    Http(reqwest::Error),
    Json(serde_json::Error),
    Yaml(serde_yaml::Error),
    Api { status: StatusCode, message: String },
    InvalidArgs(String),
}

impl Display for CliError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Http(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "{err}"),
            Self::Yaml(err) => write!(f, "{err}"),
            Self::Api { status, message } => write!(f, "API {status}: {message}"),
            Self::InvalidArgs(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for CliError {}

impl From<std::io::Error> for CliError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<reqwest::Error> for CliError {
    fn from(value: reqwest::Error) -> Self {
        Self::Http(value)
    }
}

impl From<serde_json::Error> for CliError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<serde_yaml::Error> for CliError {
    fn from(value: serde_yaml::Error) -> Self {
        Self::Yaml(value)
    }
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    error: String,
    #[allow(dead_code)]
    code: Option<String>,
    details: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AuditEntry {
    timestamp_ms: u64,
    action: String,
    endpoint: String,
    success: bool,
    http_status: Option<u16>,
    request: Option<Value>,
    response: Option<Value>,
    error: Option<String>,
}

struct ApiClient {
    base_url: String,
    audit_log: PathBuf,
    http: reqwest::Client,
}

impl ApiClient {
    fn new(base_url: String, audit_log: PathBuf) -> Self {
        // TODO(SYBIL): extract this client into a shared crate if we build a user/trading CLI.
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            audit_log,
            http: reqwest::Client::new(),
        }
    }

    async fn create_market(
        &self,
        request: &CreateMarketRequest,
    ) -> Result<CreateMarketResponse, CliError> {
        self.post_json("/v1/markets", "market.create", Some(request), request)
            .await
    }

    async fn resolve_market(
        &self,
        market_id: u32,
        request: &ResolveMarketRequest,
    ) -> Result<ResolveMarketResponse, CliError> {
        self.post_json(
            &format!("/v1/markets/{market_id}/resolve"),
            "market.resolve",
            Some(request),
            request,
        )
        .await
    }

    async fn post_json<TReq, TResp>(
        &self,
        path: &str,
        action: &str,
        audit_request: Option<&impl Serialize>,
        request: &TReq,
    ) -> Result<TResp, CliError>
    where
        TReq: Serialize + ?Sized,
        TResp: DeserializeOwned + Serialize,
    {
        let url = format!("{}{}", self.base_url, path);
        let response = self.http.post(&url).json(request).send().await?;
        self.decode_response(path, action, response, audit_request)
            .await
    }

    async fn decode_response<TResp>(
        &self,
        path: &str,
        action: &str,
        response: reqwest::Response,
        audit_request: Option<&impl Serialize>,
    ) -> Result<TResp, CliError>
    where
        TResp: DeserializeOwned + Serialize,
    {
        let status = response.status();
        let text = response.text().await?;
        let request_json = audit_request
            .map(serde_json::to_value)
            .transpose()
            .unwrap_or(None);

        if status.is_success() {
            let parsed: TResp = serde_json::from_str(&text)?;
            self.append_audit_log(AuditEntry {
                timestamp_ms: now_ms(),
                action: action.to_string(),
                endpoint: path.to_string(),
                success: true,
                http_status: Some(status.as_u16()),
                request: request_json,
                response: Some(serde_json::to_value(&parsed)?),
                error: None,
            })?;
            Ok(parsed)
        } else {
            let message = match serde_json::from_str::<ApiErrorBody>(&text) {
                Ok(body) => match body.details {
                    Some(details) => format!("{} ({details})", body.error),
                    None => body.error,
                },
                Err(_) => text.clone(),
            };
            self.append_audit_log(AuditEntry {
                timestamp_ms: now_ms(),
                action: action.to_string(),
                endpoint: path.to_string(),
                success: false,
                http_status: Some(status.as_u16()),
                request: request_json,
                response: serde_json::from_str(&text).ok(),
                error: Some(message.clone()),
            })?;
            Err(CliError::Api { status, message })
        }
    }

    fn append_audit_log(&self, entry: AuditEntry) -> Result<(), CliError> {
        if let Some(parent) = self.audit_log.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.audit_log)?;
        serde_json::to_writer(&mut file, &entry)?;
        writeln!(file)?;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), CliError> {
    let cli = Cli::parse();
    let audit_log = cli.audit_log.unwrap_or_else(default_audit_log_path);
    let client = ApiClient::new(cli.api_url, audit_log.clone());

    match cli.command {
        Command::Market(market) => match market.command {
            MarketCommand::Create { file } => {
                let spec = load_market_spec(&file)?;
                let response = client.create_market(&spec).await?;
                print_json(&response)?;
            }
            MarketCommand::Resolve {
                market_id,
                yes,
                no,
                payout_nanos,
            } => {
                let payout_nanos = resolve_payout(yes, no, payout_nanos)?;
                let response = client
                    .resolve_market(
                        market_id,
                        &ResolveMarketRequest {
                            payout_nanos,
                            attestation: None,
                        },
                    )
                    .await?;
                print_json(&response)?;
            }
        },
        Command::Audit(audit) => match audit.command {
            AuditCommand::Tail { limit } => tail_audit_log(&audit_log, limit)?,
        },
    }

    Ok(())
}

fn load_market_spec(path: &Path) -> Result<CreateMarketRequest, CliError> {
    let raw = fs::read_to_string(path)?;
    Ok(serde_yaml::from_str(&raw)?)
}

fn resolve_payout(yes: bool, no: bool, payout_nanos: Option<u64>) -> Result<u64, CliError> {
    let payout = if yes {
        NANOS_PER_DOLLAR
    } else if no {
        0
    } else if let Some(payout_nanos) = payout_nanos {
        payout_nanos
    } else {
        return Err(CliError::InvalidArgs(
            "set one of --yes, --no, or --payout-nanos".to_string(),
        ));
    };

    if payout > NANOS_PER_DOLLAR {
        return Err(CliError::InvalidArgs(format!(
            "payout must be <= {NANOS_PER_DOLLAR}, got {payout}"
        )));
    }

    Ok(payout)
}

fn default_audit_log_path() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home)
            .join(".sybil")
            .join("audit")
            .join("sybil-admin.jsonl"),
        None => PathBuf::from(".sybil-admin.jsonl"),
    }
}

fn tail_audit_log(path: &Path, limit: usize) -> Result<(), CliError> {
    if !path.exists() {
        println!("audit log not found: {}", path.display());
        return Ok(());
    }

    let file = OpenOptions::new().read(true).open(path)?;
    let lines: Vec<String> = BufReader::new(file)
        .lines()
        .collect::<Result<Vec<_>, _>>()?;

    let start = lines.len().saturating_sub(limit);
    for line in &lines[start..] {
        let entry: AuditEntry = serde_json::from_str(line)?;
        println!(
            "{} {} status={} success={}{}",
            entry.timestamp_ms,
            entry.action,
            entry
                .http_status
                .map(|status| status.to_string())
                .unwrap_or_else(|| "-".to_string()),
            entry.success,
            entry
                .error
                .as_ref()
                .map(|error| format!(" error={error}"))
                .unwrap_or_default()
        );
    }
    Ok(())
}

fn print_json<T: Serialize>(value: &T) -> Result<(), CliError> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
