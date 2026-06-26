//! Runtime configuration from CLI defaults, environment variables, and clamps.
//!
//! Invalid numeric environment values fall back to defaults with warnings so a
//! long-lived MCP server keeps a safe operating envelope instead of failing
//! startup on a single bad knob.

use std::path::PathBuf;

use tracing::warn;

// `ZEUXIS_*` environment variable names shared by CLI flags and `from_env`.
pub const ENV_MAX_CONCURRENT_CAPTURES: &str = "ZEUXIS_MAX_CONCURRENT_CAPTURES";
pub const ENV_MAX_ARTIFACTS: &str = "ZEUXIS_MAX_ARTIFACTS";
pub const ENV_MAX_ARTIFACT_BYTES: &str = "ZEUXIS_MAX_ARTIFACT_BYTES";
pub const ENV_ARTIFACT_DIR: &str = "ZEUXIS_ARTIFACT_DIR";
pub const ENV_ARTIFACT_HMAC_KEY: &str = "ZEUXIS_ARTIFACT_HMAC_KEY";
pub const ENV_BLOCKING_TASK_TIMEOUT_MS: &str = "ZEUXIS_BLOCKING_TASK_TIMEOUT_MS";
pub const ENV_CAPTURE_SOUND_FILE: &str = "ZEUXIS_CAPTURE_SOUND_FILE";
pub const ENV_WORKER_KILL_GRACE_MS: &str = "ZEUXIS_WORKER_KILL_GRACE_MS";
pub const ENV_MAX_WORKER_STDOUT_BYTES: &str = "ZEUXIS_MAX_WORKER_STDOUT_BYTES";

// Supported ranges for capture concurrency, artifact retention, and worker limits.
pub const DEFAULT_MAX_CONCURRENT_CAPTURES: usize = 2;
pub const MIN_MAX_CONCURRENT_CAPTURES: usize = 1;
pub const MAX_MAX_CONCURRENT_CAPTURES: usize = 16;

pub const DEFAULT_MAX_ARTIFACTS: usize = 64;
pub const MIN_MAX_ARTIFACTS: usize = 1;
pub const MAX_MAX_ARTIFACTS: usize = 10_000;

pub const DEFAULT_MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
pub const MIN_MAX_ARTIFACT_BYTES: u64 = 1024;
pub const MAX_MAX_ARTIFACT_BYTES: u64 = 10 * 1024 * 1024 * 1024;

pub const DEFAULT_BLOCKING_TASK_TIMEOUT_MS: u64 = 15_000;
pub const MIN_BLOCKING_TASK_TIMEOUT_MS: u64 = 100;
pub const MAX_BLOCKING_TASK_TIMEOUT_MS: u64 = 300_000;

pub const DEFAULT_WORKER_KILL_GRACE_MS: u64 = 250;
pub const MIN_WORKER_KILL_GRACE_MS: u64 = 10;
pub const MAX_WORKER_KILL_GRACE_MS: u64 = 30_000;

pub const DEFAULT_MAX_WORKER_STDOUT_BYTES: u64 = 64 * 1024;
pub const MIN_MAX_WORKER_STDOUT_BYTES: u64 = 1024;
pub const MAX_MAX_WORKER_STDOUT_BYTES: u64 = 4 * 1024 * 1024;

/// Operational limits and side-effect settings for a Zeuxis server instance.
///
/// CLI parsing and `RuntimeConfig::from_env` apply the same ranges for capture
/// concurrency, artifact retention, worker timeouts, worker stdout limits, and
/// optional capture feedback sound paths. Unset `artifact_dir` selects an
/// auto-managed session directory under the system temp path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    /// Concurrent capture permits enforced by the MCP server semaphore.
    pub max_concurrent_captures: usize,
    /// Maximum session artifacts retained in memory before eviction deletes files.
    pub max_artifacts: usize,
    /// Total on-disk bytes budget for managed artifacts in the artifact directory.
    pub max_artifact_bytes: u64,
    /// Directory for encoded screenshots; `None` uses an auto-managed temp session dir.
    pub artifact_dir: Option<PathBuf>,
    /// Optional HMAC key for `artifact_hmac_sha256` integrity metadata.
    pub artifact_hmac_key: Option<Vec<u8>>,
    /// Timeout for blocking capture, listing, and storage work in milliseconds.
    pub blocking_task_timeout_ms: u64,
    /// Optional operator sound file played after successful captures.
    pub capture_sound_file: Option<PathBuf>,
    /// Grace period after SIGTERM before the parent hard-kills a worker subprocess.
    pub worker_kill_grace_ms: u64,
    /// Maximum worker stdout bytes accepted for one JSON response line.
    pub max_worker_stdout_bytes: u64,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_concurrent_captures: DEFAULT_MAX_CONCURRENT_CAPTURES,
            max_artifacts: DEFAULT_MAX_ARTIFACTS,
            max_artifact_bytes: DEFAULT_MAX_ARTIFACT_BYTES,
            artifact_dir: None,
            artifact_hmac_key: None,
            blocking_task_timeout_ms: DEFAULT_BLOCKING_TASK_TIMEOUT_MS,
            capture_sound_file: None,
            worker_kill_grace_ms: DEFAULT_WORKER_KILL_GRACE_MS,
            max_worker_stdout_bytes: DEFAULT_MAX_WORKER_STDOUT_BYTES,
        }
    }
}

impl RuntimeConfig {
    /// Reads `ZEUXIS_*` environment variables with default fallback on invalid input.
    pub fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    fn from_lookup<F>(lookup: F) -> Self
    where
        F: Fn(&str) -> Option<String>,
    {
        Self {
            max_concurrent_captures: parse_lookup_usize(
                &lookup,
                ENV_MAX_CONCURRENT_CAPTURES,
                DEFAULT_MAX_CONCURRENT_CAPTURES,
                MIN_MAX_CONCURRENT_CAPTURES,
                MAX_MAX_CONCURRENT_CAPTURES,
            ),
            max_artifacts: parse_lookup_usize(
                &lookup,
                ENV_MAX_ARTIFACTS,
                DEFAULT_MAX_ARTIFACTS,
                MIN_MAX_ARTIFACTS,
                MAX_MAX_ARTIFACTS,
            ),
            max_artifact_bytes: parse_lookup_u64(
                &lookup,
                ENV_MAX_ARTIFACT_BYTES,
                DEFAULT_MAX_ARTIFACT_BYTES,
                MIN_MAX_ARTIFACT_BYTES,
                MAX_MAX_ARTIFACT_BYTES,
            ),
            artifact_dir: parse_lookup_path(&lookup, ENV_ARTIFACT_DIR),
            artifact_hmac_key: parse_lookup_non_empty_bytes(&lookup, ENV_ARTIFACT_HMAC_KEY),
            blocking_task_timeout_ms: parse_lookup_u64(
                &lookup,
                ENV_BLOCKING_TASK_TIMEOUT_MS,
                DEFAULT_BLOCKING_TASK_TIMEOUT_MS,
                MIN_BLOCKING_TASK_TIMEOUT_MS,
                MAX_BLOCKING_TASK_TIMEOUT_MS,
            ),
            capture_sound_file: parse_lookup_path(&lookup, ENV_CAPTURE_SOUND_FILE),
            worker_kill_grace_ms: parse_lookup_u64(
                &lookup,
                ENV_WORKER_KILL_GRACE_MS,
                DEFAULT_WORKER_KILL_GRACE_MS,
                MIN_WORKER_KILL_GRACE_MS,
                MAX_WORKER_KILL_GRACE_MS,
            ),
            max_worker_stdout_bytes: parse_lookup_u64(
                &lookup,
                ENV_MAX_WORKER_STDOUT_BYTES,
                DEFAULT_MAX_WORKER_STDOUT_BYTES,
                MIN_MAX_WORKER_STDOUT_BYTES,
                MAX_MAX_WORKER_STDOUT_BYTES,
            ),
        }
    }
}

fn parse_lookup_path<F>(lookup: &F, name: &str) -> Option<PathBuf>
where
    F: Fn(&str) -> Option<String>,
{
    lookup(name)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn parse_lookup_non_empty_bytes<F>(lookup: &F, name: &str) -> Option<Vec<u8>>
where
    F: Fn(&str) -> Option<String>,
{
    // HMAC keys are byte secrets: preserve the exact value, including
    // whitespace, and reject only an empty string.
    lookup(name)
        .map(|value| value.into_bytes())
        .filter(|value| !value.is_empty())
}

fn parse_lookup_usize<F>(lookup: &F, name: &str, default: usize, min: usize, max: usize) -> usize
where
    F: Fn(&str) -> Option<String>,
{
    let Some(raw) = lookup(name) else {
        return default;
    };

    match raw.parse::<usize>() {
        Ok(value) if (value >= min) && (value <= max) => value,
        Ok(value) => {
            warn!(
                env_var = name,
                provided = value,
                min,
                max,
                fallback = default,
                "runtime config out of range; using default"
            );
            default
        }
        Err(_) => {
            warn!(
                env_var = name,
                provided = %raw,
                min,
                max,
                fallback = default,
                "runtime config parse failed; using default"
            );
            default
        }
    }
}

fn parse_lookup_u64<F>(lookup: &F, name: &str, default: u64, min: u64, max: u64) -> u64
where
    F: Fn(&str) -> Option<String>,
{
    let Some(raw) = lookup(name) else {
        return default;
    };

    match raw.parse::<u64>() {
        Ok(value) if (value >= min) && (value <= max) => value,
        Ok(value) => {
            warn!(
                env_var = name,
                provided = value,
                min,
                max,
                fallback = default,
                "runtime config out of range; using default"
            );
            default
        }
        Err(_) => {
            warn!(
                env_var = name,
                provided = %raw,
                min,
                max,
                fallback = default,
                "runtime config parse failed; using default"
            );
            default
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::*;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn runtime_config_from_lookup_uses_defaults_when_values_are_missing() {
        let config = RuntimeConfig::from_lookup(|_| None);
        assert_eq!(config, RuntimeConfig::default());
    }

    #[test]
    fn runtime_config_from_lookup_reads_valid_values() {
        let config = RuntimeConfig::from_lookup(|name| match name {
            ENV_MAX_CONCURRENT_CAPTURES => Some("8".to_owned()),
            ENV_MAX_ARTIFACTS => Some("512".to_owned()),
            ENV_MAX_ARTIFACT_BYTES => Some("8192".to_owned()),
            ENV_ARTIFACT_DIR => Some(" /tmp/zeuxis-artifacts ".to_owned()),
            ENV_ARTIFACT_HMAC_KEY => Some("super-secret".to_owned()),
            ENV_BLOCKING_TASK_TIMEOUT_MS => Some("2500".to_owned()),
            ENV_CAPTURE_SOUND_FILE => Some(" /tmp/capture.aiff ".to_owned()),
            ENV_WORKER_KILL_GRACE_MS => Some("600".to_owned()),
            ENV_MAX_WORKER_STDOUT_BYTES => Some("131072".to_owned()),
            _ => None,
        });

        assert_eq!(config.max_concurrent_captures, 8);
        assert_eq!(config.max_artifacts, 512);
        assert_eq!(config.max_artifact_bytes, 8192);
        assert_eq!(
            config.artifact_dir,
            Some(std::path::PathBuf::from("/tmp/zeuxis-artifacts"))
        );
        assert_eq!(config.artifact_hmac_key, Some(b"super-secret".to_vec()));
        assert_eq!(config.blocking_task_timeout_ms, 2500);
        assert_eq!(
            config.capture_sound_file,
            Some(PathBuf::from("/tmp/capture.aiff"))
        );
        assert_eq!(config.worker_kill_grace_ms, 600);
        assert_eq!(config.max_worker_stdout_bytes, 131072);
    }

    #[test]
    fn runtime_config_from_lookup_falls_back_for_invalid_or_out_of_range_values() {
        let config = RuntimeConfig::from_lookup(|name| match name {
            ENV_MAX_CONCURRENT_CAPTURES => Some("0".to_owned()),
            ENV_MAX_ARTIFACTS => Some("1000000".to_owned()),
            ENV_MAX_ARTIFACT_BYTES => Some("not-a-number".to_owned()),
            ENV_ARTIFACT_DIR => Some("   ".to_owned()),
            ENV_ARTIFACT_HMAC_KEY => Some(String::new()),
            ENV_BLOCKING_TASK_TIMEOUT_MS => Some("0".to_owned()),
            ENV_CAPTURE_SOUND_FILE => Some("   ".to_owned()),
            ENV_WORKER_KILL_GRACE_MS => Some("0".to_owned()),
            ENV_MAX_WORKER_STDOUT_BYTES => Some("1".to_owned()),
            _ => None,
        });

        assert_eq!(
            config.max_concurrent_captures,
            DEFAULT_MAX_CONCURRENT_CAPTURES
        );
        assert_eq!(config.max_artifacts, DEFAULT_MAX_ARTIFACTS);
        assert_eq!(config.max_artifact_bytes, DEFAULT_MAX_ARTIFACT_BYTES);
        assert_eq!(config.artifact_dir, None);
        assert_eq!(config.artifact_hmac_key, None);
        assert_eq!(
            config.blocking_task_timeout_ms,
            DEFAULT_BLOCKING_TASK_TIMEOUT_MS
        );
        assert_eq!(config.capture_sound_file, None);
        assert_eq!(config.worker_kill_grace_ms, DEFAULT_WORKER_KILL_GRACE_MS);
        assert_eq!(
            config.max_worker_stdout_bytes,
            DEFAULT_MAX_WORKER_STDOUT_BYTES
        );
    }

    #[test]
    fn runtime_config_from_lookup_falls_back_for_parse_errors() {
        let config = RuntimeConfig::from_lookup(|name| match name {
            ENV_MAX_CONCURRENT_CAPTURES => Some("not-a-number".to_owned()),
            ENV_MAX_ARTIFACTS => Some("NaN".to_owned()),
            ENV_MAX_ARTIFACT_BYTES => Some("bad".to_owned()),
            ENV_BLOCKING_TASK_TIMEOUT_MS => Some("oops".to_owned()),
            ENV_WORKER_KILL_GRACE_MS => Some("invalid".to_owned()),
            ENV_MAX_WORKER_STDOUT_BYTES => Some("invalid".to_owned()),
            _ => None,
        });

        assert_eq!(
            config.max_concurrent_captures,
            DEFAULT_MAX_CONCURRENT_CAPTURES
        );
        assert_eq!(config.max_artifacts, DEFAULT_MAX_ARTIFACTS);
        assert_eq!(config.max_artifact_bytes, DEFAULT_MAX_ARTIFACT_BYTES);
        assert_eq!(
            config.blocking_task_timeout_ms,
            DEFAULT_BLOCKING_TASK_TIMEOUT_MS
        );
        assert_eq!(config.worker_kill_grace_ms, DEFAULT_WORKER_KILL_GRACE_MS);
        assert_eq!(
            config.max_worker_stdout_bytes,
            DEFAULT_MAX_WORKER_STDOUT_BYTES
        );
    }

    #[test]
    fn runtime_config_from_env_reads_process_environment() {
        let _guard = env_lock().lock().expect("lock env");
        unsafe {
            std::env::set_var(ENV_MAX_CONCURRENT_CAPTURES, "3");
            std::env::set_var(ENV_MAX_ARTIFACTS, "77");
            std::env::set_var(ENV_MAX_ARTIFACT_BYTES, "5000");
            std::env::set_var(ENV_ARTIFACT_DIR, "/tmp/zeuxis-env");
            std::env::set_var(ENV_ARTIFACT_HMAC_KEY, "hmac-key");
            std::env::set_var(ENV_BLOCKING_TASK_TIMEOUT_MS, "1700");
            std::env::set_var(ENV_CAPTURE_SOUND_FILE, "/tmp/zeuxis-capture.aiff");
            std::env::set_var(ENV_WORKER_KILL_GRACE_MS, "900");
            std::env::set_var(ENV_MAX_WORKER_STDOUT_BYTES, "262144");
        }

        let config = RuntimeConfig::from_env();
        assert_eq!(config.max_concurrent_captures, 3);
        assert_eq!(config.max_artifacts, 77);
        assert_eq!(config.max_artifact_bytes, 5000);
        assert_eq!(config.artifact_dir, Some(PathBuf::from("/tmp/zeuxis-env")));
        assert_eq!(config.artifact_hmac_key, Some(b"hmac-key".to_vec()));
        assert_eq!(config.blocking_task_timeout_ms, 1700);
        assert_eq!(
            config.capture_sound_file,
            Some(PathBuf::from("/tmp/zeuxis-capture.aiff"))
        );
        assert_eq!(config.worker_kill_grace_ms, 900);
        assert_eq!(config.max_worker_stdout_bytes, 262144);

        unsafe {
            std::env::remove_var(ENV_MAX_CONCURRENT_CAPTURES);
            std::env::remove_var(ENV_MAX_ARTIFACTS);
            std::env::remove_var(ENV_MAX_ARTIFACT_BYTES);
            std::env::remove_var(ENV_ARTIFACT_DIR);
            std::env::remove_var(ENV_ARTIFACT_HMAC_KEY);
            std::env::remove_var(ENV_BLOCKING_TASK_TIMEOUT_MS);
            std::env::remove_var(ENV_CAPTURE_SOUND_FILE);
            std::env::remove_var(ENV_WORKER_KILL_GRACE_MS);
            std::env::remove_var(ENV_MAX_WORKER_STDOUT_BYTES);
        }
    }
}
