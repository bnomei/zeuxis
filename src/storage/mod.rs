use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::SystemTime,
};

use image::{DynamicImage, RgbaImage};
use tempfile::Builder;
use url::Url;

use crate::mcp::errors::ServerError;

const ARTIFACT_PREFIX: &str = "zeuxis-";
const ARTIFACT_SUFFIX: &str = ".png";
const DEFAULT_MAX_ARTIFACTS: usize = 64;
const DEFAULT_MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ARTIFACTS_LIMIT: usize = 10_000;
const MAX_TOTAL_BYTES_LIMIT: u64 = 10 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredArtifact {
    pub path: PathBuf,
    pub uri: String,
    pub width: u32,
    pub height: u32,
}

pub trait PngStorage: Send + Sync {
    fn write_png(
        &self,
        image: &RgbaImage,
        capture_mode: &str,
    ) -> Result<StoredArtifact, ServerError>;
}

#[derive(Debug, Clone, Default)]
pub struct TempPngStorage;

impl TempPngStorage {
    pub const fn new() -> Self {
        Self
    }
}

impl PngStorage for TempPngStorage {
    fn write_png(
        &self,
        image: &RgbaImage,
        capture_mode: &str,
    ) -> Result<StoredArtifact, ServerError> {
        let prefix = format!("{ARTIFACT_PREFIX}{capture_mode}-");
        let mut file = Builder::new()
            .prefix(&prefix)
            .suffix(ARTIFACT_SUFFIX)
            .tempfile()
            .map_err(|err| {
                ServerError::storage_failed(format!("failed to create temp file: {err}"))
            })?;

        DynamicImage::ImageRgba8(image.clone())
            .write_to(file.as_file_mut(), image::ImageFormat::Png)
            .map_err(|err| ServerError::encode_failed(format!("failed to encode png: {err}")))?;

        let (_, path) = file.keep().map_err(|err| {
            ServerError::storage_failed(format!("failed to keep temp file: {err}"))
        })?;

        prune_artifacts(&path, retention_policy());

        let uri = Url::from_file_path(&path).map_err(|_| {
            ServerError::storage_failed(format!(
                "failed to convert path into file URI: {}",
                path.display()
            ))
        })?;

        Ok(StoredArtifact {
            path,
            uri: uri.to_string(),
            width: image.width(),
            height: image.height(),
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct RetentionPolicy {
    max_artifacts: usize,
    max_total_bytes: u64,
}

impl RetentionPolicy {
    fn from_env() -> Self {
        Self {
            max_artifacts: parse_env_usize(
                "ZEUXIS_MAX_ARTIFACTS",
                DEFAULT_MAX_ARTIFACTS,
                1,
                MAX_ARTIFACTS_LIMIT,
            ),
            max_total_bytes: parse_env_u64(
                "ZEUXIS_MAX_ARTIFACT_BYTES",
                DEFAULT_MAX_TOTAL_BYTES,
                1024,
                MAX_TOTAL_BYTES_LIMIT,
            ),
        }
    }
}

#[derive(Debug, Clone)]
struct ArtifactEntry {
    path: PathBuf,
    modified: SystemTime,
    bytes: u64,
}

fn parse_env_usize(name: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| (*value >= min) && (*value <= max))
        .unwrap_or(default)
}

fn parse_env_u64(name: &str, default: u64, min: u64, max: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (*value >= min) && (*value <= max))
        .unwrap_or(default)
}

fn retention_policy() -> RetentionPolicy {
    static POLICY: OnceLock<RetentionPolicy> = OnceLock::new();
    *POLICY.get_or_init(RetentionPolicy::from_env)
}

fn should_manage_artifact(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with(ARTIFACT_PREFIX) && name.ends_with(ARTIFACT_SUFFIX))
        .unwrap_or(false)
}

fn collect_artifacts(dir: &Path) -> Vec<ArtifactEntry> {
    let mut artifacts = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return artifacts;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !should_manage_artifact(&path) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let Ok(modified) = metadata.modified() else {
            continue;
        };

        artifacts.push(ArtifactEntry {
            path,
            modified,
            bytes: metadata.len(),
        });
    }

    artifacts.sort_by(|a, b| {
        a.modified
            .cmp(&b.modified)
            .then_with(|| a.path.cmp(&b.path))
    });
    artifacts
}

fn prune_artifacts(current_path: &Path, policy: RetentionPolicy) {
    if let Some(dir) = current_path.parent() {
        prune_artifacts_in_dir(dir, current_path, policy);
    }
}

fn prune_artifacts_in_dir(dir: &Path, current_path: &Path, policy: RetentionPolicy) {
    let mut artifacts = collect_artifacts(dir);
    let mut total_bytes: u64 = artifacts.iter().map(|entry| entry.bytes).sum();

    loop {
        let exceeds_count = artifacts.len() > policy.max_artifacts;
        let exceeds_bytes = total_bytes > policy.max_total_bytes;
        if !exceeds_count && !exceeds_bytes {
            break;
        }

        let Some(index) = artifacts
            .iter()
            .position(|entry| entry.path != current_path)
        else {
            break;
        };
        let victim = artifacts.remove(index);
        if fs::remove_file(&victim.path).is_ok() {
            total_bytes = total_bytes.saturating_sub(victim.bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use tempfile::tempdir;

    use super::*;

    fn write_artifact(dir: &Path, name: &str, bytes: usize) -> PathBuf {
        let path = dir.join(format!("{ARTIFACT_PREFIX}{name}{ARTIFACT_SUFFIX}"));
        fs::write(&path, vec![0_u8; bytes]).expect("write artifact");
        thread::sleep(Duration::from_millis(5));
        path
    }

    #[test]
    fn storage_retention_prunes_oldest_files_by_count_and_keeps_current() {
        let dir = tempdir().expect("tempdir");
        let oldest = write_artifact(dir.path(), "oldest", 8);
        let middle = write_artifact(dir.path(), "middle", 8);
        let current = write_artifact(dir.path(), "current", 8);

        prune_artifacts_in_dir(
            dir.path(),
            &current,
            RetentionPolicy {
                max_artifacts: 2,
                max_total_bytes: u64::MAX,
            },
        );

        assert!(!oldest.exists(), "oldest file should be pruned");
        assert!(middle.exists(), "middle file should remain");
        assert!(current.exists(), "current file should remain");
    }

    #[test]
    fn storage_retention_prunes_to_total_bytes_limit_and_keeps_current() {
        let dir = tempdir().expect("tempdir");
        let oldest = write_artifact(dir.path(), "oldest", 10);
        let middle = write_artifact(dir.path(), "middle", 10);
        let current = write_artifact(dir.path(), "current", 10);

        prune_artifacts_in_dir(
            dir.path(),
            &current,
            RetentionPolicy {
                max_artifacts: 10,
                max_total_bytes: 15,
            },
        );

        assert!(!oldest.exists(), "oldest file should be pruned");
        assert!(!middle.exists(), "middle file should be pruned");
        assert!(current.exists(), "current file should remain");
    }
}
