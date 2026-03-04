use std::{
    fs,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

use hmac::{Hmac, Mac};
use image::{DynamicImage, RgbaImage};
use sha2::{Digest, Sha256};
use tempfile::Builder;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::warn;
use url::Url;

use crate::{
    mcp::errors::ServerError,
    runtime_config::{
        DEFAULT_MAX_ARTIFACT_BYTES, DEFAULT_MAX_ARTIFACTS, MAX_MAX_ARTIFACT_BYTES,
        MAX_MAX_ARTIFACTS, MIN_MAX_ARTIFACT_BYTES, MIN_MAX_ARTIFACTS,
    },
};

const ARTIFACT_PREFIX: &str = "zeuxis-";
const ARTIFACT_SUFFIXES: [&str; 3] = [".png", ".jpg", ".webp"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureOutputFormat {
    Png,
    Jpeg,
    Webp,
}

impl CaptureOutputFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
        }
    }

    pub const fn file_suffix(self) -> &'static str {
        match self {
            Self::Png => ".png",
            Self::Jpeg => ".jpg",
            Self::Webp => ".webp",
        }
    }

    pub const fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureOutputOptions {
    pub format: CaptureOutputFormat,
    pub jpeg_quality: u8,
}

impl Default for CaptureOutputOptions {
    fn default() -> Self {
        Self {
            format: CaptureOutputFormat::Png,
            jpeg_quality: 82,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredArtifact {
    pub path: PathBuf,
    pub uri: String,
    pub output_format: String,
    pub mime_type: String,
    pub artifact_sha256: String,
    pub artifact_hmac_sha256: Option<String>,
    pub width: u32,
    pub height: u32,
    pub captured_at_utc: String,
}

pub trait PngStorage: Send + Sync {
    fn write_image(
        &self,
        image: RgbaImage,
        capture_mode: &str,
        output: CaptureOutputOptions,
    ) -> Result<StoredArtifact, ServerError>;

    fn latest_artifact(&self) -> Result<StoredArtifact, ServerError>;
}

#[derive(Debug, Clone)]
pub struct TempPngStorage {
    retention_policy: RetentionPolicy,
    artifact_dir: Option<PathBuf>,
    artifact_hmac_key: Option<Vec<u8>>,
    latest_artifact_cache: Arc<Mutex<Option<StoredArtifact>>>,
}

impl TempPngStorage {
    pub fn new() -> Self {
        Self::with_settings(
            DEFAULT_MAX_ARTIFACTS,
            DEFAULT_MAX_ARTIFACT_BYTES,
            None,
            None,
        )
    }

    pub fn with_retention_policy(max_artifacts: usize, max_total_bytes: u64) -> Self {
        Self::with_settings(max_artifacts, max_total_bytes, None, None)
    }

    pub fn with_settings(
        max_artifacts: usize,
        max_total_bytes: u64,
        artifact_dir: Option<PathBuf>,
        artifact_hmac_key: Option<Vec<u8>>,
    ) -> Self {
        Self {
            retention_policy: RetentionPolicy {
                max_artifacts: max_artifacts.clamp(MIN_MAX_ARTIFACTS, MAX_MAX_ARTIFACTS),
                max_total_bytes: max_total_bytes
                    .clamp(MIN_MAX_ARTIFACT_BYTES, MAX_MAX_ARTIFACT_BYTES),
            },
            artifact_dir,
            artifact_hmac_key,
            latest_artifact_cache: Arc::new(Mutex::new(None)),
        }
    }
}

impl Default for TempPngStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl PngStorage for TempPngStorage {
    fn write_image(
        &self,
        image: RgbaImage,
        capture_mode: &str,
        output: CaptureOutputOptions,
    ) -> Result<StoredArtifact, ServerError> {
        let prefix = format!("{ARTIFACT_PREFIX}{capture_mode}-");
        let mut file = self.create_temp_file(&prefix, output.format.file_suffix())?;

        let width = image.width();
        let height = image.height();
        let dynamic = DynamicImage::ImageRgba8(image);
        let encoded = match output.format {
            CaptureOutputFormat::Png => {
                dynamic.write_to(file.as_file_mut(), image::ImageFormat::Png)
            }
            CaptureOutputFormat::Jpeg => {
                let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                    file.as_file_mut(),
                    output.jpeg_quality,
                );
                encoder.encode_image(&dynamic)
            }
            CaptureOutputFormat::Webp => {
                dynamic.write_to(file.as_file_mut(), image::ImageFormat::WebP)
            }
        };
        encoded.map_err(|err| {
            ServerError::encode_failed(format!(
                "failed to encode {}: {err}",
                output.format.as_str()
            ))
        })?;

        let (_, path) = file.keep().map_err(|err| {
            ServerError::storage_failed(format!("failed to keep temp file: {err}"))
        })?;

        let (artifact_sha256, artifact_hmac_sha256) =
            compute_integrity_fields(&path, self.artifact_hmac_key.as_deref())?;

        prune_artifacts(&path, self.retention_policy);

        let uri = Url::from_file_path(&path).map_err(|_| {
            ServerError::storage_failed(format!(
                "failed to convert path into file URI: {}",
                path.display()
            ))
        })?;

        let artifact = StoredArtifact {
            path,
            uri: uri.to_string(),
            output_format: output.format.as_str().to_owned(),
            mime_type: output.format.mime_type().to_owned(),
            artifact_sha256,
            artifact_hmac_sha256,
            width,
            height,
            captured_at_utc: now_rfc3339_utc(),
        };

        if let Ok(mut latest) = self.latest_artifact_cache.lock() {
            *latest = Some(artifact.clone());
        }

        Ok(artifact)
    }

    fn latest_artifact(&self) -> Result<StoredArtifact, ServerError> {
        let mut latest = self
            .latest_artifact_cache
            .lock()
            .map_err(|_| ServerError::storage_failed("latest artifact cache lock poisoned"))?;

        let Some(artifact) = latest.clone() else {
            return Err(ServerError::no_capture_yet(
                "no screenshot has been captured in this server session",
            ));
        };

        if !artifact.path.exists() {
            *latest = None;
            return Err(ServerError::no_capture_yet(
                "latest screenshot artifact is unavailable; capture a new screenshot",
            ));
        }

        Ok(artifact)
    }
}

impl TempPngStorage {
    fn create_temp_file(
        &self,
        prefix: &str,
        suffix: &str,
    ) -> Result<tempfile::NamedTempFile, ServerError> {
        let mut builder = Builder::new();
        builder.prefix(prefix).suffix(suffix);

        match &self.artifact_dir {
            Some(artifact_dir) => {
                fs::create_dir_all(artifact_dir).map_err(|err| {
                    ServerError::storage_failed(format!(
                        "failed to create artifact directory {}: {err}",
                        artifact_dir.display()
                    ))
                })?;
                builder.tempfile_in(artifact_dir).map_err(|err| {
                    ServerError::storage_failed(format!(
                        "failed to create temp file in {}: {err}",
                        artifact_dir.display()
                    ))
                })
            }
            None => builder.tempfile().map_err(|err| {
                ServerError::storage_failed(format!("failed to create temp file: {err}"))
            }),
        }
    }
}

fn compute_integrity_fields(
    path: &Path,
    hmac_key: Option<&[u8]>,
) -> Result<(String, Option<String>), ServerError> {
    type HmacSha256 = Hmac<Sha256>;

    let file = fs::File::open(path).map_err(|err| {
        ServerError::storage_failed(format!(
            "failed to open artifact for hashing {}: {err}",
            path.display()
        ))
    })?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut hmac = match hmac_key {
        Some(key) => Some(HmacSha256::new_from_slice(key).map_err(|_| {
            ServerError::storage_failed("failed to initialize artifact hmac signer")
        })?),
        None => None,
    };

    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let bytes_read = reader.read(&mut buffer).map_err(|err| {
            ServerError::storage_failed(format!(
                "failed to read artifact for hashing {}: {err}",
                path.display()
            ))
        })?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
        if let Some(mac) = hmac.as_mut() {
            mac.update(&buffer[..bytes_read]);
        }
    }

    let artifact_sha256 = hex_lower(&hasher.finalize());
    let artifact_hmac_sha256 = hmac.map(|mac| hex_lower(&mac.finalize().into_bytes()));
    Ok((artifact_sha256, artifact_hmac_sha256))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn now_rfc3339_utc() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

#[derive(Debug, Clone, Copy)]
struct RetentionPolicy {
    max_artifacts: usize,
    max_total_bytes: u64,
}

#[derive(Debug, Clone)]
struct ArtifactEntry {
    path: PathBuf,
    modified: SystemTime,
    bytes: u64,
}

fn should_manage_artifact(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            name.starts_with(ARTIFACT_PREFIX)
                && ARTIFACT_SUFFIXES
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
        })
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
        match fs::remove_file(&victim.path) {
            Ok(()) => {
                total_bytes = total_bytes.saturating_sub(victim.bytes);
            }
            Err(err) => {
                warn!(
                    path = %victim.path.display(),
                    error = %err,
                    "artifact retention prune could not remove candidate"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use tempfile::tempdir;

    use super::*;

    fn write_artifact(dir: &Path, name: &str, bytes: usize) -> PathBuf {
        let path = dir.join(format!("{ARTIFACT_PREFIX}{name}{}", ARTIFACT_SUFFIXES[0]));
        fs::write(&path, vec![0_u8; bytes]).expect("write artifact");
        thread::sleep(Duration::from_millis(5));
        path
    }

    fn sample_image() -> RgbaImage {
        RgbaImage::from_pixel(4, 3, image::Rgba([1, 2, 3, 255]))
    }

    #[test]
    fn storage_write_image_rejects_artifact_dir_that_is_not_a_directory() {
        let temp = tempdir().expect("tempdir");
        let non_dir_path = temp.path().join("not-a-directory");
        fs::write(&non_dir_path, b"not-a-directory").expect("seed file");

        let storage = TempPngStorage::with_settings(
            DEFAULT_MAX_ARTIFACTS,
            DEFAULT_MAX_ARTIFACT_BYTES,
            Some(non_dir_path),
            None,
        );

        let error = storage
            .write_image(
                sample_image(),
                "capture_screen",
                CaptureOutputOptions::default(),
            )
            .expect_err("write should fail when artifact dir is not a directory");

        assert_eq!(error.error_code(), "storage_failed");
    }

    #[cfg(unix)]
    #[test]
    fn storage_write_image_rejects_read_only_artifact_dir() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().expect("tempdir");
        let artifact_dir = temp.path().join("readonly");
        fs::create_dir_all(&artifact_dir).expect("create readonly dir");

        let mut perms = fs::metadata(&artifact_dir).expect("metadata").permissions();
        perms.set_mode(0o500);
        fs::set_permissions(&artifact_dir, perms).expect("set readonly perms");

        let storage = TempPngStorage::with_settings(
            DEFAULT_MAX_ARTIFACTS,
            DEFAULT_MAX_ARTIFACT_BYTES,
            Some(artifact_dir.clone()),
            None,
        );

        let error = storage
            .write_image(
                sample_image(),
                "capture_screen",
                CaptureOutputOptions::default(),
            )
            .expect_err("write should fail in readonly directory");

        assert_eq!(error.error_code(), "storage_failed");

        let mut restore = fs::metadata(&artifact_dir)
            .expect("metadata after write")
            .permissions();
        restore.set_mode(0o700);
        fs::set_permissions(&artifact_dir, restore).expect("restore writable perms");
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

    #[test]
    fn storage_retention_does_not_delete_current_when_it_is_the_only_candidate() {
        let dir = tempdir().expect("tempdir");
        let current = write_artifact(dir.path(), "current", 10);

        prune_artifacts_in_dir(
            dir.path(),
            &current,
            RetentionPolicy {
                max_artifacts: 0,
                max_total_bytes: 0,
            },
        );

        assert!(current.exists(), "current file should remain");
    }

    #[cfg(unix)]
    #[test]
    fn storage_retention_keeps_candidate_when_delete_fails() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("tempdir");
        let oldest = write_artifact(dir.path(), "oldest", 8);
        let current = write_artifact(dir.path(), "current", 8);

        let mut perms = fs::metadata(dir.path()).expect("metadata").permissions();
        perms.set_mode(0o500);
        fs::set_permissions(dir.path(), perms).expect("set readonly dir");

        prune_artifacts_in_dir(
            dir.path(),
            &current,
            RetentionPolicy {
                max_artifacts: 1,
                max_total_bytes: u64::MAX,
            },
        );

        assert!(
            oldest.exists(),
            "oldest file should remain when deletion fails"
        );
        assert!(current.exists(), "current file should remain");

        let mut restore = fs::metadata(dir.path())
            .expect("metadata after prune")
            .permissions();
        restore.set_mode(0o700);
        fs::set_permissions(dir.path(), restore).expect("restore writable perms");
    }

    #[test]
    fn storage_latest_artifact_returns_no_capture_yet_before_first_write() {
        let storage = TempPngStorage::new();
        let error = storage
            .latest_artifact()
            .expect_err("latest artifact should be missing");
        assert_eq!(error.error_code(), "no_capture_yet");
    }

    #[test]
    fn storage_latest_artifact_returns_written_artifact_and_handles_missing_file() {
        let dir = tempdir().expect("tempdir");
        let storage = TempPngStorage::with_settings(
            DEFAULT_MAX_ARTIFACTS,
            DEFAULT_MAX_ARTIFACT_BYTES,
            Some(dir.path().to_path_buf()),
            None,
        );

        let written = storage
            .write_image(
                sample_image(),
                "capture_screen",
                CaptureOutputOptions::default(),
            )
            .expect("write artifact");
        let latest = storage.latest_artifact().expect("latest artifact");
        assert_eq!(latest.path, written.path);

        fs::remove_file(&written.path).expect("remove artifact");
        let error = storage
            .latest_artifact()
            .expect_err("latest artifact should fail when file is gone");
        assert_eq!(error.error_code(), "no_capture_yet");
    }

    #[test]
    fn storage_capture_output_format_metadata_matches_expected_values() {
        assert_eq!(CaptureOutputFormat::Png.as_str(), "png");
        assert_eq!(CaptureOutputFormat::Png.file_suffix(), ".png");
        assert_eq!(CaptureOutputFormat::Png.mime_type(), "image/png");

        assert_eq!(CaptureOutputFormat::Jpeg.as_str(), "jpeg");
        assert_eq!(CaptureOutputFormat::Jpeg.file_suffix(), ".jpg");
        assert_eq!(CaptureOutputFormat::Jpeg.mime_type(), "image/jpeg");

        assert_eq!(CaptureOutputFormat::Webp.as_str(), "webp");
        assert_eq!(CaptureOutputFormat::Webp.file_suffix(), ".webp");
        assert_eq!(CaptureOutputFormat::Webp.mime_type(), "image/webp");
    }

    #[test]
    fn storage_write_image_supports_jpeg_and_webp_and_hmac() {
        let dir = tempdir().expect("tempdir");
        let storage = TempPngStorage::with_settings(
            DEFAULT_MAX_ARTIFACTS,
            DEFAULT_MAX_ARTIFACT_BYTES,
            Some(dir.path().to_path_buf()),
            Some(b"secret-key".to_vec()),
        );

        let jpeg = storage
            .write_image(
                sample_image(),
                "capture_screen",
                CaptureOutputOptions {
                    format: CaptureOutputFormat::Jpeg,
                    jpeg_quality: 90,
                },
            )
            .expect("jpeg write");
        assert_eq!(jpeg.output_format, "jpeg");
        assert_eq!(jpeg.mime_type, "image/jpeg");
        assert!(jpeg.path.to_string_lossy().ends_with(".jpg"));
        assert!(jpeg.artifact_hmac_sha256.is_some());

        let webp = storage
            .write_image(
                sample_image(),
                "capture_screen",
                CaptureOutputOptions {
                    format: CaptureOutputFormat::Webp,
                    jpeg_quality: 82,
                },
            )
            .expect("webp write");
        assert_eq!(webp.output_format, "webp");
        assert_eq!(webp.mime_type, "image/webp");
        assert!(webp.path.to_string_lossy().ends_with(".webp"));
        assert!(webp.artifact_hmac_sha256.is_some());
    }

    #[test]
    fn storage_write_image_with_default_storage_uses_system_temp_dir() {
        let storage = TempPngStorage::new();
        let artifact = storage
            .write_image(
                sample_image(),
                "capture_screen",
                CaptureOutputOptions::default(),
            )
            .expect("write image");
        assert!(artifact.path.exists());
    }

    #[test]
    fn storage_default_constructs_and_writes_png() {
        let storage = TempPngStorage::default();
        let artifact = storage
            .write_image(
                sample_image(),
                "capture_screen",
                CaptureOutputOptions::default(),
            )
            .expect("write image");
        assert_eq!(artifact.output_format, "png");
        assert_eq!(artifact.mime_type, "image/png");
        assert!(artifact.path.exists());
    }

    #[test]
    fn storage_with_retention_policy_clamps_to_supported_range() {
        let storage = TempPngStorage::with_retention_policy(0, 0);
        assert_eq!(storage.retention_policy.max_artifacts, MIN_MAX_ARTIFACTS);
        assert_eq!(
            storage.retention_policy.max_total_bytes,
            MIN_MAX_ARTIFACT_BYTES
        );
    }

    #[test]
    fn storage_compute_integrity_fields_returns_error_when_file_missing() {
        let missing = PathBuf::from("/tmp/zeuxis-does-not-exist");
        let error = compute_integrity_fields(&missing, None).expect_err("missing file should fail");
        assert_eq!(error.error_code(), "storage_failed");
    }

    #[test]
    fn storage_hex_lower_encodes_bytes_as_expected() {
        assert_eq!(hex_lower(&[0x00, 0x0a, 0xff]), "000aff");
    }

    #[test]
    fn storage_should_manage_artifact_filters_prefix_and_suffix() {
        assert!(should_manage_artifact(Path::new(
            "zeuxis-capture_screen-1.png"
        )));
        assert!(should_manage_artifact(Path::new(
            "zeuxis-capture_screen-1.jpg"
        )));
        assert!(should_manage_artifact(Path::new(
            "zeuxis-capture_screen-1.webp"
        )));
        assert!(!should_manage_artifact(Path::new("capture_screen-1.png")));
        assert!(!should_manage_artifact(Path::new(
            "zeuxis-capture_screen-1.gif"
        )));
    }

    #[test]
    fn storage_collect_artifacts_skips_non_files_and_non_matching_entries() {
        let dir = tempdir().expect("tempdir");
        let _artifact = write_artifact(dir.path(), "ok", 5);
        fs::write(dir.path().join("not-managed.txt"), b"x").expect("write not managed");
        fs::create_dir_all(dir.path().join("zeuxis-folder.png")).expect("mkdir");

        let artifacts = collect_artifacts(dir.path());
        assert_eq!(artifacts.len(), 1);
        assert!(artifacts[0].path.to_string_lossy().contains("zeuxis-ok"));
    }

    #[test]
    fn storage_collect_artifacts_returns_empty_when_directory_missing() {
        let missing = PathBuf::from("/tmp/zeuxis-missing-dir-for-collect-artifacts");
        let artifacts = collect_artifacts(&missing);
        assert!(artifacts.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn storage_collect_artifacts_skips_broken_symlink_entries() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().expect("tempdir");
        let broken = dir.path().join("zeuxis-broken.png");
        symlink(dir.path().join("missing-target"), &broken).expect("create symlink");

        let artifacts = collect_artifacts(dir.path());
        assert!(artifacts.is_empty());
    }
}
