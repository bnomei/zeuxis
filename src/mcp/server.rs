//! Server composition, component injection, and rmcp stdio serving.
//!
//! Production construction runs capture work in a subprocess worker, while
//! component constructors keep capture inline for tests and embedders that own
//! their backend, storage, and permission boundaries.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::router::tool::ToolRouter,
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool_handler,
};

use crate::{
    capture::{
        backend::{CaptureBackend, WindowInfo},
        xcap_backend::XcapBackend,
    },
    cursor::{CursorProvider, DeviceQueryCursorProvider},
    platform::{PermissionGate, PlatformPermissionGate},
    runtime_config::{
        MAX_BLOCKING_TASK_TIMEOUT_MS, MAX_MAX_WORKER_STDOUT_BYTES, MAX_WORKER_KILL_GRACE_MS,
        MIN_BLOCKING_TASK_TIMEOUT_MS, MIN_MAX_CONCURRENT_CAPTURES, MIN_MAX_WORKER_STDOUT_BYTES,
        MIN_WORKER_KILL_GRACE_MS, RuntimeConfig,
    },
    storage::{PngStorage, TempPngStorage},
};

/// Side-effect boundary for successful-capture feedback.
pub trait CaptureFeedbackEmitter: Send + Sync {
    /// Emits best-effort feedback after a successful capture.
    fn emit_capture(&self);
}

/// Feedback emitter that writes an ASCII bell to stderr.
#[derive(Debug, Default)]
pub struct TerminalBellFeedbackEmitter;

impl CaptureFeedbackEmitter for TerminalBellFeedbackEmitter {
    fn emit_capture(&self) {
        eprint!("\x07");
    }
}

/// Feedback emitter that tries a platform shutter sound before falling back to bell.
#[derive(Debug, Clone, Default)]
pub struct PlatformSoundFeedbackEmitter {
    capture_sound_file: Option<PathBuf>,
}

impl PlatformSoundFeedbackEmitter {
    /// Creates a sound emitter with an optional operator-supplied sound file.
    pub const fn new(capture_sound_file: Option<PathBuf>) -> Self {
        Self { capture_sound_file }
    }
}

impl CaptureFeedbackEmitter for PlatformSoundFeedbackEmitter {
    fn emit_capture(&self) {
        if !try_emit_platform_feedback_sound(self.capture_sound_file.as_deref()) {
            eprint!("\x07");
        }
    }
}

fn try_emit_platform_feedback_sound(capture_sound_file: Option<&std::path::Path>) -> bool {
    if let Some(path) = capture_sound_file
        && try_emit_custom_sound_file(path)
    {
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        spawn_feedback_sound_process("afplay", &["/System/Library/Sounds/Glass.aiff"])
    }

    #[cfg(target_os = "linux")]
    {
        spawn_feedback_sound_process("canberra-gtk-play", &["-i", "camera-shutter"])
            || spawn_feedback_sound_process(
                "paplay",
                &["/usr/share/sounds/freedesktop/stereo/camera-shutter.oga"],
            )
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

fn try_emit_custom_sound_file(path: &std::path::Path) -> bool {
    let sound_path = path.to_string_lossy().into_owned();
    #[cfg(target_os = "macos")]
    {
        spawn_feedback_sound_process("afplay", &[&sound_path])
    }

    #[cfg(target_os = "linux")]
    {
        spawn_feedback_sound_process("paplay", &[&sound_path])
            || spawn_feedback_sound_process("aplay", &[&sound_path])
            || spawn_feedback_sound_process("canberra-gtk-play", &["--file", &sound_path])
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

fn spawn_feedback_sound_process(command: &str, args: &[&str]) -> bool {
    match std::process::Command::new(command)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::sleep(Duration::from_millis(25));
            match child.try_wait() {
                Ok(Some(status)) => status.success(),
                Ok(None) => {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    true
                }
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

/// Latest `list_windows` snapshot accepted by `capture_window`.
#[derive(Debug, Clone)]
pub(crate) struct WindowSnapshotState {
    pub snapshot_id: String,
    pub id_scope: String,
    pub listed_at_utc: String,
    pub windows: Vec<WindowInfo>,
}

/// Execution path for blocking capture work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureExecutionMode {
    /// Run capture in `spawn_blocking` inside the MCP server process.
    Inline,
    /// Run capture in the hidden `__worker` subprocess protocol.
    SubprocessWorker,
}

/// Latest successful capture's artifact and context, stored under one lock so
/// concurrent `get_latest_capture` calls cannot pair mismatched metadata.
#[derive(Clone)]
pub(crate) struct LatestCapture {
    pub(crate) artifact: crate::storage::StoredArtifact,
    pub(crate) context: crate::mcp::result::CaptureContextPayload,
}

/// MCP screenshot server with injected capture, cursor, permission, and storage boundaries.
#[derive(Clone)]
pub struct ZeuxisScreenshotServer {
    pub(crate) backend: Arc<dyn CaptureBackend>,
    pub(crate) cursor_provider: Arc<dyn CursorProvider>,
    pub(crate) permission_gate: Arc<dyn PermissionGate>,
    pub(crate) storage: Arc<dyn PngStorage>,
    pub(crate) last_capture: Arc<Mutex<Option<LatestCapture>>>,
    pub(crate) last_window_snapshot: Arc<Mutex<Option<WindowSnapshotState>>>,
    pub(crate) capture_slots: Arc<tokio::sync::Semaphore>,
    pub(crate) blocking_task_timeout: Duration,
    pub(crate) capture_execution_mode: CaptureExecutionMode,
    pub(crate) worker_executable: Option<PathBuf>,
    #[allow(dead_code)]
    pub(crate) worker_kill_grace: Duration,
    #[allow(dead_code)]
    pub(crate) max_worker_stdout_bytes: u64,
    pub(crate) feedback_emitter: Arc<dyn CaptureFeedbackEmitter>,
    pub(crate) tool_router: ToolRouter<Self>,
}

struct ServerSettings {
    max_concurrent_captures: usize,
    blocking_task_timeout: Duration,
    capture_execution_mode: CaptureExecutionMode,
    worker_executable: Option<PathBuf>,
    worker_kill_grace: Duration,
    max_worker_stdout_bytes: u64,
    feedback_emitter: Arc<dyn CaptureFeedbackEmitter>,
}

impl ZeuxisScreenshotServer {
    /// Creates a production server from process environment runtime config.
    pub fn new() -> Self {
        Self::with_runtime_config(RuntimeConfig::from_env())
    }

    /// Creates a production server from explicit runtime config.
    ///
    /// Capture work runs in subprocess worker mode, and storage retention/HMAC
    /// settings are copied from the provided config.
    pub fn with_runtime_config(config: RuntimeConfig) -> Self {
        let worker_executable = std::env::current_exe().ok();
        Self::with_components_and_settings(
            Arc::new(XcapBackend::new()),
            Arc::new(DeviceQueryCursorProvider::new()),
            Arc::new(PlatformPermissionGate::new()),
            Arc::new(TempPngStorage::with_settings(
                config.max_artifacts,
                config.max_artifact_bytes,
                config.artifact_dir.clone(),
                config.artifact_hmac_key.clone(),
            )),
            ServerSettings {
                max_concurrent_captures: config.max_concurrent_captures,
                blocking_task_timeout: Duration::from_millis(config.blocking_task_timeout_ms),
                capture_execution_mode: CaptureExecutionMode::SubprocessWorker,
                worker_executable,
                worker_kill_grace: Duration::from_millis(config.worker_kill_grace_ms),
                max_worker_stdout_bytes: config.max_worker_stdout_bytes,
                feedback_emitter: Arc::new(PlatformSoundFeedbackEmitter::new(
                    config.capture_sound_file.clone(),
                )),
            },
        )
    }

    /// Creates a server from injected components using inline capture execution.
    pub fn with_components(
        backend: Arc<dyn CaptureBackend>,
        cursor_provider: Arc<dyn CursorProvider>,
        permission_gate: Arc<dyn PermissionGate>,
        storage: Arc<dyn PngStorage>,
    ) -> Self {
        let config = RuntimeConfig::from_env();
        Self::with_components_and_settings(
            backend,
            cursor_provider,
            permission_gate,
            storage,
            ServerSettings {
                max_concurrent_captures: config.max_concurrent_captures,
                blocking_task_timeout: Duration::from_millis(config.blocking_task_timeout_ms),
                capture_execution_mode: CaptureExecutionMode::Inline,
                worker_executable: None,
                worker_kill_grace: Duration::from_millis(config.worker_kill_grace_ms),
                max_worker_stdout_bytes: config.max_worker_stdout_bytes,
                feedback_emitter: Arc::new(PlatformSoundFeedbackEmitter::new(
                    config.capture_sound_file.clone(),
                )),
            },
        )
    }

    /// Creates an inline server from injected components with explicit parallelism.
    pub fn with_components_and_parallelism(
        backend: Arc<dyn CaptureBackend>,
        cursor_provider: Arc<dyn CursorProvider>,
        permission_gate: Arc<dyn PermissionGate>,
        storage: Arc<dyn PngStorage>,
        max_concurrent_captures: usize,
    ) -> Self {
        let config = RuntimeConfig::from_env();
        Self::with_components_and_limits(
            backend,
            cursor_provider,
            permission_gate,
            storage,
            max_concurrent_captures,
            Duration::from_millis(config.blocking_task_timeout_ms),
            Arc::new(PlatformSoundFeedbackEmitter::new(
                config.capture_sound_file.clone(),
            )),
        )
    }

    /// Creates an inline server from injected components and feedback emitter.
    pub fn with_components_and_feedback(
        backend: Arc<dyn CaptureBackend>,
        cursor_provider: Arc<dyn CursorProvider>,
        permission_gate: Arc<dyn PermissionGate>,
        storage: Arc<dyn PngStorage>,
        max_concurrent_captures: usize,
        feedback_emitter: Arc<dyn CaptureFeedbackEmitter>,
    ) -> Self {
        let config = RuntimeConfig::from_env();
        Self::with_components_and_limits(
            backend,
            cursor_provider,
            permission_gate,
            storage,
            max_concurrent_captures,
            Duration::from_millis(config.blocking_task_timeout_ms),
            feedback_emitter,
        )
    }

    /// Creates an inline server with explicit capture concurrency and timeout limits.
    pub fn with_components_and_limits(
        backend: Arc<dyn CaptureBackend>,
        cursor_provider: Arc<dyn CursorProvider>,
        permission_gate: Arc<dyn PermissionGate>,
        storage: Arc<dyn PngStorage>,
        max_concurrent_captures: usize,
        blocking_task_timeout: Duration,
        feedback_emitter: Arc<dyn CaptureFeedbackEmitter>,
    ) -> Self {
        let config = RuntimeConfig::from_env();
        Self::with_components_and_settings(
            backend,
            cursor_provider,
            permission_gate,
            storage,
            ServerSettings {
                max_concurrent_captures,
                blocking_task_timeout,
                capture_execution_mode: CaptureExecutionMode::Inline,
                worker_executable: None,
                worker_kill_grace: Duration::from_millis(config.worker_kill_grace_ms),
                max_worker_stdout_bytes: config.max_worker_stdout_bytes,
                feedback_emitter,
            },
        )
    }

    fn with_components_and_settings(
        backend: Arc<dyn CaptureBackend>,
        cursor_provider: Arc<dyn CursorProvider>,
        permission_gate: Arc<dyn PermissionGate>,
        storage: Arc<dyn PngStorage>,
        settings: ServerSettings,
    ) -> Self {
        let max_concurrent_captures = settings
            .max_concurrent_captures
            .max(MIN_MAX_CONCURRENT_CAPTURES);
        let blocking_task_timeout = normalize_blocking_task_timeout(settings.blocking_task_timeout);
        let worker_kill_grace = normalize_worker_kill_grace(settings.worker_kill_grace);
        let max_worker_stdout_bytes =
            normalize_max_worker_stdout_bytes(settings.max_worker_stdout_bytes);
        Self {
            backend,
            cursor_provider,
            permission_gate,
            storage,
            last_capture: Arc::new(Mutex::new(None)),
            last_window_snapshot: Arc::new(Mutex::new(None)),
            capture_slots: Arc::new(tokio::sync::Semaphore::new(max_concurrent_captures)),
            blocking_task_timeout,
            capture_execution_mode: settings.capture_execution_mode,
            worker_executable: settings.worker_executable,
            worker_kill_grace,
            max_worker_stdout_bytes,
            feedback_emitter: settings.feedback_emitter,
            tool_router: Self::build_tool_router(),
        }
    }

    /// Serves MCP over stdio until the client disconnects.
    pub async fn serve_stdio(self) -> Result<(), rmcp::RmcpError> {
        let service = self.serve(rmcp::transport::stdio()).await?;
        service.waiting().await?;
        Ok(())
    }
}

fn normalize_blocking_task_timeout(timeout: Duration) -> Duration {
    let clamped_ms = timeout.as_millis().clamp(
        u128::from(MIN_BLOCKING_TASK_TIMEOUT_MS),
        u128::from(MAX_BLOCKING_TASK_TIMEOUT_MS),
    ) as u64;
    Duration::from_millis(clamped_ms)
}

fn normalize_worker_kill_grace(timeout: Duration) -> Duration {
    let clamped_ms = timeout.as_millis().clamp(
        u128::from(MIN_WORKER_KILL_GRACE_MS),
        u128::from(MAX_WORKER_KILL_GRACE_MS),
    ) as u64;
    Duration::from_millis(clamped_ms)
}

fn normalize_max_worker_stdout_bytes(max_worker_stdout_bytes: u64) -> u64 {
    max_worker_stdout_bytes.clamp(MIN_MAX_WORKER_STDOUT_BYTES, MAX_MAX_WORKER_STDOUT_BYTES)
}

impl Default for ZeuxisScreenshotServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ZeuxisScreenshotServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("zeuxis", env!("CARGO_PKG_VERSION"))
                    .with_title("Zeuxis Screenshot Server")
                    .with_description("Read-only local MCP screenshot server"),
            )
            .with_instructions(
                "Provides local screenshot capture tools only. No remote upload, OCR, or automation.",
            )
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{
            Arc, Mutex, OnceLock,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use image::{Rgba, RgbaImage};
    use rmcp::handler::server::wrapper::Parameters;

    use super::*;
    use crate::{
        capture::{
            backend::{CaptureBackend, MonitorInfo, WindowInfo},
            region::{GlobalRect, Point},
        },
        cursor::CursorProvider,
        mcp::{
            errors::ServerError,
            tools::{CaptureScreenParams, CommonCaptureParams},
        },
        platform::PermissionGate,
        runtime_config::{
            DEFAULT_BLOCKING_TASK_TIMEOUT_MS, ENV_BLOCKING_TASK_TIMEOUT_MS,
            ENV_MAX_CONCURRENT_CAPTURES, ENV_MAX_WORKER_STDOUT_BYTES, ENV_WORKER_KILL_GRACE_MS,
            MAX_BLOCKING_TASK_TIMEOUT_MS, MAX_MAX_WORKER_STDOUT_BYTES, MAX_WORKER_KILL_GRACE_MS,
            MIN_BLOCKING_TASK_TIMEOUT_MS, MIN_MAX_WORKER_STDOUT_BYTES, MIN_WORKER_KILL_GRACE_MS,
            RuntimeConfig,
        },
        storage::{CaptureOutputFormat, CaptureOutputOptions, PngStorage, StoredArtifact},
    };

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[derive(Debug)]
    struct DummyBackend;

    impl CaptureBackend for DummyBackend {
        fn list_monitors(&self) -> Result<Vec<MonitorInfo>, ServerError> {
            Ok(vec![MonitorInfo {
                id: 1,
                name: "Dummy".to_owned(),
                x: 0,
                y: 0,
                width: 32,
                height: 24,
                is_primary: true,
                is_builtin: true,
            }])
        }

        fn list_windows(&self) -> Result<Vec<WindowInfo>, ServerError> {
            Ok(vec![WindowInfo {
                id: 7,
                title: "Dummy".to_owned(),
                app: "Zeuxis".to_owned(),
                x: 0,
                y: 0,
                width: 32,
                height: 24,
                is_focused: true,
                is_minimized: false,
            }])
        }

        fn capture_screen(&self, _monitor_id: Option<u32>) -> Result<RgbaImage, ServerError> {
            Ok(RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])))
        }

        fn capture_window(&self, _window_id: u32) -> Result<RgbaImage, ServerError> {
            self.capture_screen(None)
        }

        fn capture_monitor_region(
            &self,
            _monitor_id: u32,
            _x: u32,
            _y: u32,
            _width: u32,
            _height: u32,
        ) -> Result<RgbaImage, ServerError> {
            self.capture_screen(None)
        }

        fn capture_active_window(&self) -> Result<RgbaImage, ServerError> {
            self.capture_screen(None)
        }

        fn capture_window_at_cursor(&self, _cursor: Point) -> Result<RgbaImage, ServerError> {
            self.capture_screen(None)
        }

        fn capture_cursor_region(
            &self,
            _cursor: Point,
            _size: u32,
        ) -> Result<RgbaImage, ServerError> {
            self.capture_screen(None)
        }

        fn capture_rect(&self, _rect: GlobalRect) -> Result<RgbaImage, ServerError> {
            self.capture_screen(None)
        }
    }

    #[derive(Debug)]
    struct DummyCursor;

    impl CursorProvider for DummyCursor {
        fn cursor_position(&self) -> Result<Point, ServerError> {
            Ok(Point::new(1, 1))
        }
    }

    #[derive(Debug)]
    struct DummyPermission;

    impl PermissionGate for DummyPermission {
        fn ensure_capture_allowed(&self) -> Result<(), ServerError> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct DummyStorage;

    impl PngStorage for DummyStorage {
        fn write_image(
            &self,
            image: RgbaImage,
            capture_mode: &str,
            output: CaptureOutputOptions,
        ) -> Result<StoredArtifact, ServerError> {
            let suffix = match output.format {
                CaptureOutputFormat::Png => "png",
                CaptureOutputFormat::Jpeg => "jpg",
                CaptureOutputFormat::Webp => "webp",
            };
            Ok(StoredArtifact {
                artifact_id: format!("{capture_mode}.{suffix}"),
                capture_mode: capture_mode.to_owned(),
                path: PathBuf::from(format!("/tmp/{capture_mode}.{suffix}")),
                uri: format!("file:///tmp/{capture_mode}.{suffix}"),
                output_format: output.format.as_str().to_owned(),
                mime_type: output.format.mime_type().to_owned(),
                artifact_sha256: "00".repeat(32),
                artifact_hmac_sha256: None,
                width: image.width(),
                height: image.height(),
                captured_at_utc: "2026-01-01T00:00:00Z".to_owned(),
            })
        }

        fn adopt_artifact(
            &self,
            path: PathBuf,
            capture_mode: &str,
            output: CaptureOutputOptions,
        ) -> Result<StoredArtifact, ServerError> {
            let (width, height) = image::image_dimensions(&path).unwrap_or((2, 2));
            Ok(StoredArtifact {
                artifact_id: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("adopted.png")
                    .to_owned(),
                capture_mode: capture_mode.to_owned(),
                uri: format!("file://{}", path.display()),
                path,
                output_format: output.format.as_str().to_owned(),
                mime_type: output.format.mime_type().to_owned(),
                artifact_sha256: "00".repeat(32),
                artifact_hmac_sha256: None,
                width,
                height,
                captured_at_utc: "2026-01-01T00:00:00Z".to_owned(),
            })
        }

        fn latest_artifact(&self) -> Result<StoredArtifact, ServerError> {
            Ok(StoredArtifact {
                artifact_id: "latest.png".to_owned(),
                capture_mode: "capture_screen".to_owned(),
                path: PathBuf::from("/tmp/latest.png"),
                uri: "file:///tmp/latest.png".to_owned(),
                output_format: "png".to_owned(),
                mime_type: "image/png".to_owned(),
                artifact_sha256: "00".repeat(32),
                artifact_hmac_sha256: None,
                width: 2,
                height: 2,
                captured_at_utc: "2026-01-01T00:00:00Z".to_owned(),
            })
        }

        fn list_session_artifacts(&self) -> Result<Vec<StoredArtifact>, ServerError> {
            Ok(vec![])
        }

        fn clear_session_artifacts(&self) -> Result<usize, ServerError> {
            Ok(0)
        }

        fn artifact_dir(&self) -> PathBuf {
            std::env::temp_dir()
        }
    }

    #[derive(Debug, Default)]
    struct CountingFeedbackEmitter {
        capture_calls: AtomicUsize,
    }

    impl CaptureFeedbackEmitter for CountingFeedbackEmitter {
        fn emit_capture(&self) {
            self.capture_calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn mcp_server_with_components_and_parallelism_sets_permit_count() {
        let server = ZeuxisScreenshotServer::with_components_and_parallelism(
            Arc::new(DummyBackend),
            Arc::new(DummyCursor),
            Arc::new(DummyPermission),
            Arc::new(DummyStorage),
            5,
        );

        assert_eq!(server.capture_slots.available_permits(), 5);
    }

    #[test]
    fn mcp_server_get_info_returns_expected_metadata() {
        let server = ZeuxisScreenshotServer::with_components_and_parallelism(
            Arc::new(DummyBackend),
            Arc::new(DummyCursor),
            Arc::new(DummyPermission),
            Arc::new(DummyStorage),
            1,
        );

        let info_debug = format!("{:?}", server.get_info());
        assert!(info_debug.contains("Zeuxis Screenshot Server"));
        assert!(info_debug.contains("zeuxis"));
    }

    #[test]
    fn mcp_server_terminal_bell_feedback_emitter_is_callable() {
        let emitter = TerminalBellFeedbackEmitter;
        emitter.emit_capture();
    }

    #[test]
    fn mcp_server_platform_sound_feedback_emitter_is_callable() {
        let emitter = PlatformSoundFeedbackEmitter::default();
        emitter.emit_capture();
    }

    #[test]
    fn mcp_server_dummy_components_are_callable_for_all_capture_paths() {
        let backend = DummyBackend;
        assert_eq!(backend.list_monitors().expect("monitors").len(), 1);
        assert_eq!(backend.list_windows().expect("windows").len(), 1);
        assert_eq!(backend.capture_screen(None).expect("screen").width(), 2);
        assert_eq!(backend.capture_window(7).expect("window by id").height(), 2);
        assert_eq!(
            backend
                .capture_monitor_region(1, 0, 0, 1, 1)
                .expect("monitor region")
                .width(),
            2
        );
        assert_eq!(backend.capture_active_window().expect("active").height(), 2);
        assert_eq!(
            backend
                .capture_window_at_cursor(Point::new(5, 6))
                .expect("window")
                .width(),
            2
        );
        assert_eq!(
            backend
                .capture_cursor_region(Point::new(5, 6), 42)
                .expect("cursor region")
                .width(),
            2
        );
        assert_eq!(
            backend
                .capture_rect(GlobalRect {
                    x: 1,
                    y: 2,
                    width: 3,
                    height: 4
                })
                .expect("rect")
                .height(),
            2
        );

        let cursor = DummyCursor;
        assert_eq!(cursor.cursor_position().expect("cursor"), Point::new(1, 1));

        let permission = DummyPermission;
        assert!(permission.ensure_capture_allowed().is_ok());
    }

    #[test]
    fn mcp_server_dummy_storage_encodes_output_metadata_variants() {
        let storage = DummyStorage;
        let image = RgbaImage::from_pixel(4, 3, Rgba([9, 9, 9, 255]));

        let png = storage
            .write_image(
                image.clone(),
                "capture_screen",
                CaptureOutputOptions {
                    format: CaptureOutputFormat::Png,
                    jpeg_quality: 82,
                },
            )
            .expect("png");
        assert_eq!(png.output_format, "png");
        assert_eq!(png.mime_type, "image/png");

        let jpeg = storage
            .write_image(
                image.clone(),
                "capture_screen",
                CaptureOutputOptions {
                    format: CaptureOutputFormat::Jpeg,
                    jpeg_quality: 90,
                },
            )
            .expect("jpeg");
        assert_eq!(jpeg.output_format, "jpeg");
        assert_eq!(jpeg.mime_type, "image/jpeg");

        let webp = storage
            .write_image(
                image,
                "capture_screen",
                CaptureOutputOptions {
                    format: CaptureOutputFormat::Webp,
                    jpeg_quality: 82,
                },
            )
            .expect("webp");
        assert_eq!(webp.output_format, "webp");
        assert_eq!(webp.mime_type, "image/webp");

        let latest = storage.latest_artifact().expect("latest");
        assert_eq!(latest.path, PathBuf::from("/tmp/latest.png"));
        assert_eq!(
            storage
                .clear_session_artifacts()
                .expect("clear session artifacts"),
            0
        );
    }

    #[test]
    fn mcp_server_with_runtime_config_applies_parallelism() {
        let server = ZeuxisScreenshotServer::with_runtime_config(RuntimeConfig {
            max_concurrent_captures: 7,
            max_artifacts: 99,
            max_artifact_bytes: 10_000,
            artifact_dir: Some(PathBuf::from("/tmp/zeuxis-server-config")),
            artifact_hmac_key: Some(b"key".to_vec()),
            blocking_task_timeout_ms: DEFAULT_BLOCKING_TASK_TIMEOUT_MS,
            capture_sound_file: Some(PathBuf::from("/tmp/capture.aiff")),
            worker_kill_grace_ms: 900,
            max_worker_stdout_bytes: 200_000,
        });

        assert_eq!(server.capture_slots.available_permits(), 7);
        assert_eq!(server.worker_kill_grace, Duration::from_millis(900));
        assert_eq!(server.max_worker_stdout_bytes, 200_000);
    }

    #[test]
    fn mcp_server_worker_runtime_limits_are_clamped() {
        let min_server = ZeuxisScreenshotServer::with_runtime_config(RuntimeConfig {
            worker_kill_grace_ms: 0,
            max_worker_stdout_bytes: 0,
            ..RuntimeConfig::default()
        });
        assert_eq!(
            min_server.worker_kill_grace,
            Duration::from_millis(MIN_WORKER_KILL_GRACE_MS)
        );
        assert_eq!(
            min_server.max_worker_stdout_bytes,
            MIN_MAX_WORKER_STDOUT_BYTES
        );

        let max_server = ZeuxisScreenshotServer::with_runtime_config(RuntimeConfig {
            worker_kill_grace_ms: MAX_WORKER_KILL_GRACE_MS + 1,
            max_worker_stdout_bytes: MAX_MAX_WORKER_STDOUT_BYTES + 1,
            ..RuntimeConfig::default()
        });
        assert_eq!(
            max_server.worker_kill_grace,
            Duration::from_millis(MAX_WORKER_KILL_GRACE_MS)
        );
        assert_eq!(
            max_server.max_worker_stdout_bytes,
            MAX_MAX_WORKER_STDOUT_BYTES
        );
    }

    #[test]
    fn mcp_server_zero_parallelism_is_clamped_to_one() {
        let server = ZeuxisScreenshotServer::with_components_and_parallelism(
            Arc::new(DummyBackend),
            Arc::new(DummyCursor),
            Arc::new(DummyPermission),
            Arc::new(DummyStorage),
            0,
        );

        assert_eq!(server.capture_slots.available_permits(), 1);
    }

    #[test]
    fn mcp_server_timeout_limits_are_clamped() {
        let min_server = ZeuxisScreenshotServer::with_components_and_limits(
            Arc::new(DummyBackend),
            Arc::new(DummyCursor),
            Arc::new(DummyPermission),
            Arc::new(DummyStorage),
            1,
            Duration::from_millis(0),
            Arc::new(TerminalBellFeedbackEmitter),
        );
        assert_eq!(
            min_server.blocking_task_timeout,
            Duration::from_millis(MIN_BLOCKING_TASK_TIMEOUT_MS)
        );

        let max_server = ZeuxisScreenshotServer::with_components_and_limits(
            Arc::new(DummyBackend),
            Arc::new(DummyCursor),
            Arc::new(DummyPermission),
            Arc::new(DummyStorage),
            1,
            Duration::from_millis(MAX_BLOCKING_TASK_TIMEOUT_MS + 1),
            Arc::new(TerminalBellFeedbackEmitter),
        );
        assert_eq!(
            max_server.blocking_task_timeout,
            Duration::from_millis(MAX_BLOCKING_TASK_TIMEOUT_MS)
        );
    }

    #[test]
    fn mcp_server_with_components_uses_env_parallelism() {
        let _guard = env_lock().lock().expect("lock env");
        unsafe {
            std::env::set_var(ENV_MAX_CONCURRENT_CAPTURES, "4");
            std::env::set_var(ENV_WORKER_KILL_GRACE_MS, "1200");
            std::env::set_var(ENV_MAX_WORKER_STDOUT_BYTES, "300000");
        }

        let server = ZeuxisScreenshotServer::with_components(
            Arc::new(DummyBackend),
            Arc::new(DummyCursor),
            Arc::new(DummyPermission),
            Arc::new(DummyStorage),
        );
        assert_eq!(server.capture_slots.available_permits(), 4);
        assert_eq!(server.worker_kill_grace, Duration::from_millis(1200));
        assert_eq!(server.max_worker_stdout_bytes, 300000);

        unsafe {
            std::env::remove_var(ENV_MAX_CONCURRENT_CAPTURES);
            std::env::remove_var(ENV_WORKER_KILL_GRACE_MS);
            std::env::remove_var(ENV_MAX_WORKER_STDOUT_BYTES);
        }
    }

    #[tokio::test]
    async fn mcp_server_with_components_and_feedback_uses_custom_emitter_and_env_timeout() {
        let feedback = Arc::new(CountingFeedbackEmitter::default());
        let server = {
            let _guard = env_lock().lock().expect("lock env");
            unsafe {
                std::env::set_var(ENV_BLOCKING_TASK_TIMEOUT_MS, "1700");
                std::env::set_var(ENV_WORKER_KILL_GRACE_MS, "800");
                std::env::set_var(ENV_MAX_WORKER_STDOUT_BYTES, "240000");
            }

            let server = ZeuxisScreenshotServer::with_components_and_feedback(
                Arc::new(DummyBackend),
                Arc::new(DummyCursor),
                Arc::new(DummyPermission),
                Arc::new(DummyStorage),
                2,
                feedback.clone(),
            );

            unsafe {
                std::env::remove_var(ENV_BLOCKING_TASK_TIMEOUT_MS);
                std::env::remove_var(ENV_WORKER_KILL_GRACE_MS);
                std::env::remove_var(ENV_MAX_WORKER_STDOUT_BYTES);
            }
            server
        };

        assert_eq!(server.blocking_task_timeout, Duration::from_millis(1700));
        assert_eq!(server.worker_kill_grace, Duration::from_millis(800));
        assert_eq!(server.max_worker_stdout_bytes, 240000);

        let result = server
            .capture_screen(Parameters(CaptureScreenParams {
                common: CommonCaptureParams {
                    play_sound: Some(true),
                    ..CommonCaptureParams::default()
                },
                monitor_id: None,
            }))
            .await
            .expect("tool call");
        assert_eq!(result.is_error, Some(false));
        assert_eq!(feedback.capture_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn mcp_server_capture_returns_storage_failed_when_capture_slots_closed() {
        let server = ZeuxisScreenshotServer::with_components_and_limits(
            Arc::new(DummyBackend),
            Arc::new(DummyCursor),
            Arc::new(DummyPermission),
            Arc::new(DummyStorage),
            1,
            Duration::from_millis(DEFAULT_BLOCKING_TASK_TIMEOUT_MS),
            Arc::new(TerminalBellFeedbackEmitter),
        );
        server.capture_slots.close();

        let result = server
            .capture_screen(Parameters(CaptureScreenParams::default()))
            .await
            .expect("tool call");

        assert_eq!(result.is_error, Some(true));
        let structured = result.structured_content.expect("structured");
        assert_eq!(structured["error_code"], "storage_failed");
        assert!(
            structured["message"]
                .as_str()
                .unwrap_or_default()
                .contains("capture slot coordination failed")
        );
    }

    #[test]
    fn mcp_server_new_and_default_construct_servers() {
        let server = ZeuxisScreenshotServer::new();
        assert!(server.capture_slots.available_permits() >= 1);
        assert!(server.worker_kill_grace >= Duration::from_millis(MIN_WORKER_KILL_GRACE_MS));
        assert!(server.worker_kill_grace <= Duration::from_millis(MAX_WORKER_KILL_GRACE_MS));
        assert!(server.max_worker_stdout_bytes >= MIN_MAX_WORKER_STDOUT_BYTES);
        assert!(server.max_worker_stdout_bytes <= MAX_MAX_WORKER_STDOUT_BYTES);

        let default_server = ZeuxisScreenshotServer::default();
        assert!(default_server.capture_slots.available_permits() >= 1);
        assert!(
            default_server.worker_kill_grace >= Duration::from_millis(MIN_WORKER_KILL_GRACE_MS)
        );
        assert!(
            default_server.worker_kill_grace <= Duration::from_millis(MAX_WORKER_KILL_GRACE_MS)
        );
        assert!(default_server.max_worker_stdout_bytes >= MIN_MAX_WORKER_STDOUT_BYTES);
        assert!(default_server.max_worker_stdout_bytes <= MAX_MAX_WORKER_STDOUT_BYTES);
    }
}
