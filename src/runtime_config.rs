use std::path::PathBuf;

pub const ENV_MAX_CONCURRENT_CAPTURES: &str = "ZEUXIS_MAX_CONCURRENT_CAPTURES";
pub const ENV_MAX_ARTIFACTS: &str = "ZEUXIS_MAX_ARTIFACTS";
pub const ENV_MAX_ARTIFACT_BYTES: &str = "ZEUXIS_MAX_ARTIFACT_BYTES";
pub const ENV_ARTIFACT_DIR: &str = "ZEUXIS_ARTIFACT_DIR";
pub const ENV_ARTIFACT_HMAC_KEY: &str = "ZEUXIS_ARTIFACT_HMAC_KEY";
pub const ENV_BLOCKING_TASK_TIMEOUT_MS: &str = "ZEUXIS_BLOCKING_TASK_TIMEOUT_MS";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub max_concurrent_captures: usize,
    pub max_artifacts: usize,
    pub max_artifact_bytes: u64,
    pub artifact_dir: Option<PathBuf>,
    pub artifact_hmac_key: Option<Vec<u8>>,
    pub blocking_task_timeout_ms: u64,
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
        }
    }
}

impl RuntimeConfig {
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
    lookup(name)
        .map(|value| value.into_bytes())
        .filter(|value| !value.is_empty())
}

fn parse_lookup_usize<F>(lookup: &F, name: &str, default: usize, min: usize, max: usize) -> usize
where
    F: Fn(&str) -> Option<String>,
{
    lookup(name)
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| (*value >= min) && (*value <= max))
        .unwrap_or(default)
}

fn parse_lookup_u64<F>(lookup: &F, name: &str, default: u64, min: u64, max: u64) -> u64
where
    F: Fn(&str) -> Option<String>,
{
    lookup(name)
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (*value >= min) && (*value <= max))
        .unwrap_or(default)
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
        }

        let config = RuntimeConfig::from_env();
        assert_eq!(config.max_concurrent_captures, 3);
        assert_eq!(config.max_artifacts, 77);
        assert_eq!(config.max_artifact_bytes, 5000);
        assert_eq!(config.artifact_dir, Some(PathBuf::from("/tmp/zeuxis-env")));
        assert_eq!(config.artifact_hmac_key, Some(b"hmac-key".to_vec()));
        assert_eq!(config.blocking_task_timeout_ms, 1700);

        unsafe {
            std::env::remove_var(ENV_MAX_CONCURRENT_CAPTURES);
            std::env::remove_var(ENV_MAX_ARTIFACTS);
            std::env::remove_var(ENV_MAX_ARTIFACT_BYTES);
            std::env::remove_var(ENV_ARTIFACT_DIR);
            std::env::remove_var(ENV_ARTIFACT_HMAC_KEY);
            std::env::remove_var(ENV_BLOCKING_TASK_TIMEOUT_MS);
        }
    }
}
