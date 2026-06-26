//! Zeuxis binary entrypoint: MCP stdio server and hidden capture worker mode.
//!
//! Default invocation serves MCP over stdin/stdout. The hidden `__worker`
//! subcommand runs one-shot subprocess capture for production servers; logs go
//! to stderr so stdout stays protocol-clean.

use std::{error::Error, path::PathBuf};

use clap::{Parser, Subcommand};
use tracing_subscriber::{EnvFilter, fmt};
use zeuxis::{
    mcp::ZeuxisScreenshotServer,
    runtime_config::{
        DEFAULT_BLOCKING_TASK_TIMEOUT_MS, DEFAULT_MAX_ARTIFACT_BYTES, DEFAULT_MAX_ARTIFACTS,
        DEFAULT_MAX_CONCURRENT_CAPTURES, DEFAULT_MAX_WORKER_STDOUT_BYTES,
        DEFAULT_WORKER_KILL_GRACE_MS, ENV_ARTIFACT_DIR, ENV_BLOCKING_TASK_TIMEOUT_MS,
        ENV_CAPTURE_SOUND_FILE, ENV_MAX_ARTIFACT_BYTES, ENV_MAX_ARTIFACTS,
        ENV_MAX_CONCURRENT_CAPTURES, ENV_MAX_WORKER_STDOUT_BYTES, ENV_WORKER_KILL_GRACE_MS,
        MAX_BLOCKING_TASK_TIMEOUT_MS, MAX_MAX_ARTIFACT_BYTES, MAX_MAX_ARTIFACTS,
        MAX_MAX_CONCURRENT_CAPTURES, MAX_MAX_WORKER_STDOUT_BYTES, MAX_WORKER_KILL_GRACE_MS,
        MIN_BLOCKING_TASK_TIMEOUT_MS, MIN_MAX_ARTIFACT_BYTES, MIN_MAX_ARTIFACTS,
        MIN_MAX_CONCURRENT_CAPTURES, MIN_MAX_WORKER_STDOUT_BYTES, MIN_WORKER_KILL_GRACE_MS,
        RuntimeConfig,
    },
    worker::child::run_stdio_worker,
};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Local read-only MCP screenshot server",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Option<InternalCommand>,

    #[arg(
        long,
        env = ENV_MAX_CONCURRENT_CAPTURES,
        default_value_t = DEFAULT_MAX_CONCURRENT_CAPTURES,
        value_parser = parse_max_concurrent_captures
    )]
    max_concurrent_captures: usize,

    #[arg(
        long,
        env = ENV_MAX_ARTIFACTS,
        default_value_t = DEFAULT_MAX_ARTIFACTS,
        value_parser = parse_max_artifacts
    )]
    max_artifacts: usize,

    #[arg(
        long,
        env = ENV_MAX_ARTIFACT_BYTES,
        default_value_t = DEFAULT_MAX_ARTIFACT_BYTES,
        value_parser = parse_max_artifact_bytes
    )]
    max_artifact_bytes: u64,

    #[arg(long, env = ENV_ARTIFACT_DIR)]
    artifact_dir: Option<PathBuf>,

    #[arg(
        long,
        env = ENV_BLOCKING_TASK_TIMEOUT_MS,
        default_value_t = DEFAULT_BLOCKING_TASK_TIMEOUT_MS,
        value_parser = parse_blocking_task_timeout_ms
    )]
    blocking_task_timeout_ms: u64,

    #[arg(long, env = ENV_CAPTURE_SOUND_FILE)]
    capture_sound_file: Option<PathBuf>,

    #[arg(
        long,
        env = ENV_WORKER_KILL_GRACE_MS,
        default_value_t = DEFAULT_WORKER_KILL_GRACE_MS,
        value_parser = parse_worker_kill_grace_ms
    )]
    worker_kill_grace_ms: u64,

    #[arg(
        long,
        env = ENV_MAX_WORKER_STDOUT_BYTES,
        default_value_t = DEFAULT_MAX_WORKER_STDOUT_BYTES,
        value_parser = parse_max_worker_stdout_bytes
    )]
    max_worker_stdout_bytes: u64,
}

#[derive(Debug, Clone, Subcommand, PartialEq, Eq)]
enum InternalCommand {
    #[command(name = "__worker", hide = true)]
    Worker,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    if matches!(cli.command, Some(InternalCommand::Worker)) {
        run_stdio_worker()?;
        return Ok(());
    }
    init_tracing();

    let mut runtime_config = RuntimeConfig::from_env();
    runtime_config.max_concurrent_captures = cli.max_concurrent_captures;
    runtime_config.max_artifacts = cli.max_artifacts;
    runtime_config.max_artifact_bytes = cli.max_artifact_bytes;
    runtime_config.artifact_dir = cli.artifact_dir;
    runtime_config.blocking_task_timeout_ms = cli.blocking_task_timeout_ms;
    runtime_config.capture_sound_file = cli.capture_sound_file;
    runtime_config.worker_kill_grace_ms = cli.worker_kill_grace_ms;
    runtime_config.max_worker_stdout_bytes = cli.max_worker_stdout_bytes;

    let server = ZeuxisScreenshotServer::with_runtime_config(runtime_config);
    server.serve_stdio().await?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // MCP stdio responses are emitted on stdout; keep logs on stderr to avoid protocol corruption.
    let _ = fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();
}

fn parse_max_concurrent_captures(value: &str) -> Result<usize, String> {
    parse_usize_in_range(
        value,
        "max-concurrent-captures",
        MIN_MAX_CONCURRENT_CAPTURES,
        MAX_MAX_CONCURRENT_CAPTURES,
    )
}

fn parse_max_artifacts(value: &str) -> Result<usize, String> {
    parse_usize_in_range(value, "max-artifacts", MIN_MAX_ARTIFACTS, MAX_MAX_ARTIFACTS)
}

fn parse_max_artifact_bytes(value: &str) -> Result<u64, String> {
    parse_u64_in_range(
        value,
        "max-artifact-bytes",
        MIN_MAX_ARTIFACT_BYTES,
        MAX_MAX_ARTIFACT_BYTES,
    )
}

fn parse_blocking_task_timeout_ms(value: &str) -> Result<u64, String> {
    parse_u64_in_range(
        value,
        "blocking-task-timeout-ms",
        MIN_BLOCKING_TASK_TIMEOUT_MS,
        MAX_BLOCKING_TASK_TIMEOUT_MS,
    )
}

fn parse_worker_kill_grace_ms(value: &str) -> Result<u64, String> {
    parse_u64_in_range(
        value,
        "worker-kill-grace-ms",
        MIN_WORKER_KILL_GRACE_MS,
        MAX_WORKER_KILL_GRACE_MS,
    )
}

fn parse_max_worker_stdout_bytes(value: &str) -> Result<u64, String> {
    parse_u64_in_range(
        value,
        "max-worker-stdout-bytes",
        MIN_MAX_WORKER_STDOUT_BYTES,
        MAX_MAX_WORKER_STDOUT_BYTES,
    )
}

fn parse_usize_in_range(value: &str, name: &str, min: usize, max: usize) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("{name} must be a valid integer"))?;
    if parsed < min || parsed > max {
        return Err(format!("{name} must be in range {min}..={max}"));
    }
    Ok(parsed)
}

fn parse_u64_in_range(value: &str, name: &str, min: u64, max: u64) -> Result<u64, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be a valid integer"))?;
    if parsed < min || parsed > max {
        return Err(format!("{name} must be in range {min}..={max}"));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_cli_parse_max_concurrent_captures_accepts_range_bounds() {
        assert_eq!(
            parse_max_concurrent_captures(&MIN_MAX_CONCURRENT_CAPTURES.to_string())
                .expect("min should parse"),
            MIN_MAX_CONCURRENT_CAPTURES
        );
        assert_eq!(
            parse_max_concurrent_captures(&MAX_MAX_CONCURRENT_CAPTURES.to_string())
                .expect("max should parse"),
            MAX_MAX_CONCURRENT_CAPTURES
        );
    }

    #[test]
    fn main_cli_parse_max_concurrent_captures_rejects_invalid_values() {
        assert!(parse_max_concurrent_captures("0").is_err());
        assert!(parse_max_concurrent_captures("abc").is_err());
    }

    #[test]
    fn main_cli_parse_max_artifacts_and_bytes_validate_ranges() {
        assert_eq!(
            parse_max_artifacts(&MIN_MAX_ARTIFACTS.to_string()).expect("min artifacts"),
            MIN_MAX_ARTIFACTS
        );
        assert!(parse_max_artifacts("0").is_err());
        assert_eq!(
            parse_max_artifact_bytes(&MIN_MAX_ARTIFACT_BYTES.to_string())
                .expect("min artifact bytes"),
            MIN_MAX_ARTIFACT_BYTES
        );
        assert!(parse_max_artifact_bytes("not-a-number").is_err());
        assert!(parse_max_artifact_bytes(&(MAX_MAX_ARTIFACT_BYTES + 1).to_string()).is_err());
    }

    #[test]
    fn main_cli_parse_blocking_task_timeout_ms_validate_ranges() {
        assert_eq!(
            parse_blocking_task_timeout_ms(&MIN_BLOCKING_TASK_TIMEOUT_MS.to_string())
                .expect("min timeout ms"),
            MIN_BLOCKING_TASK_TIMEOUT_MS
        );
        assert_eq!(
            parse_blocking_task_timeout_ms(&MAX_BLOCKING_TASK_TIMEOUT_MS.to_string())
                .expect("max timeout ms"),
            MAX_BLOCKING_TASK_TIMEOUT_MS
        );
        assert!(parse_blocking_task_timeout_ms("0").is_err());
        assert!(
            parse_blocking_task_timeout_ms(&(MAX_BLOCKING_TASK_TIMEOUT_MS + 1).to_string())
                .is_err()
        );
    }

    #[test]
    fn main_cli_parse_worker_kill_grace_ms_validate_ranges() {
        assert_eq!(
            parse_worker_kill_grace_ms(&MIN_WORKER_KILL_GRACE_MS.to_string())
                .expect("min worker kill grace ms"),
            MIN_WORKER_KILL_GRACE_MS
        );
        assert_eq!(
            parse_worker_kill_grace_ms(&MAX_WORKER_KILL_GRACE_MS.to_string())
                .expect("max worker kill grace ms"),
            MAX_WORKER_KILL_GRACE_MS
        );
        assert!(parse_worker_kill_grace_ms("0").is_err());
        assert!(parse_worker_kill_grace_ms(&(MAX_WORKER_KILL_GRACE_MS + 1).to_string()).is_err());
    }

    #[test]
    fn main_cli_parse_max_worker_stdout_bytes_validate_ranges() {
        assert_eq!(
            parse_max_worker_stdout_bytes(&MIN_MAX_WORKER_STDOUT_BYTES.to_string())
                .expect("min max worker stdout bytes"),
            MIN_MAX_WORKER_STDOUT_BYTES
        );
        assert_eq!(
            parse_max_worker_stdout_bytes(&MAX_MAX_WORKER_STDOUT_BYTES.to_string())
                .expect("max max worker stdout bytes"),
            MAX_MAX_WORKER_STDOUT_BYTES
        );
        assert!(parse_max_worker_stdout_bytes("0").is_err());
        assert!(
            parse_max_worker_stdout_bytes(&(MAX_MAX_WORKER_STDOUT_BYTES + 1).to_string()).is_err()
        );
    }

    #[test]
    fn main_cli_init_tracing_is_idempotent() {
        init_tracing();
        init_tracing();
    }

    #[test]
    fn main_cli_parse_hidden_worker_subcommand() {
        let cli = Cli::try_parse_from(["zeuxis", "__worker"]).expect("worker mode should parse");
        assert!(matches!(cli.command, Some(InternalCommand::Worker)));
    }
}
