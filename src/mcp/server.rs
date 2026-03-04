use std::{sync::Arc, time::Duration};

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::router::tool::ToolRouter,
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool_handler,
};

use crate::{
    capture::{backend::CaptureBackend, xcap_backend::XcapBackend},
    cursor::{CursorProvider, DeviceQueryCursorProvider},
    platform::{PermissionGate, PlatformPermissionGate},
    runtime_config::{
        MAX_BLOCKING_TASK_TIMEOUT_MS, MIN_BLOCKING_TASK_TIMEOUT_MS, MIN_MAX_CONCURRENT_CAPTURES,
        RuntimeConfig,
    },
    storage::{PngStorage, TempPngStorage},
};

pub trait CaptureFeedbackEmitter: Send + Sync {
    fn emit(&self);
}

#[derive(Debug, Default)]
pub struct TerminalBellFeedbackEmitter;

impl CaptureFeedbackEmitter for TerminalBellFeedbackEmitter {
    fn emit(&self) {
        eprint!("\x07");
    }
}

#[derive(Clone)]
pub struct ZeuxisScreenshotServer {
    pub(crate) backend: Arc<dyn CaptureBackend>,
    pub(crate) cursor_provider: Arc<dyn CursorProvider>,
    pub(crate) permission_gate: Arc<dyn PermissionGate>,
    pub(crate) storage: Arc<dyn PngStorage>,
    pub(crate) capture_slots: Arc<tokio::sync::Semaphore>,
    pub(crate) blocking_task_timeout: Duration,
    pub(crate) feedback_emitter: Arc<dyn CaptureFeedbackEmitter>,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl ZeuxisScreenshotServer {
    pub fn new() -> Self {
        Self::with_runtime_config(RuntimeConfig::from_env())
    }

    pub fn with_runtime_config(config: RuntimeConfig) -> Self {
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
            config.max_concurrent_captures,
            Duration::from_millis(config.blocking_task_timeout_ms),
            Arc::new(TerminalBellFeedbackEmitter),
        )
    }

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
            config.max_concurrent_captures,
            Duration::from_millis(config.blocking_task_timeout_ms),
            Arc::new(TerminalBellFeedbackEmitter),
        )
    }

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
            Arc::new(TerminalBellFeedbackEmitter),
        )
    }

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

    pub fn with_components_and_limits(
        backend: Arc<dyn CaptureBackend>,
        cursor_provider: Arc<dyn CursorProvider>,
        permission_gate: Arc<dyn PermissionGate>,
        storage: Arc<dyn PngStorage>,
        max_concurrent_captures: usize,
        blocking_task_timeout: Duration,
        feedback_emitter: Arc<dyn CaptureFeedbackEmitter>,
    ) -> Self {
        Self::with_components_and_settings(
            backend,
            cursor_provider,
            permission_gate,
            storage,
            max_concurrent_captures,
            blocking_task_timeout,
            feedback_emitter,
        )
    }

    fn with_components_and_settings(
        backend: Arc<dyn CaptureBackend>,
        cursor_provider: Arc<dyn CursorProvider>,
        permission_gate: Arc<dyn PermissionGate>,
        storage: Arc<dyn PngStorage>,
        max_concurrent_captures: usize,
        blocking_task_timeout: Duration,
        feedback_emitter: Arc<dyn CaptureFeedbackEmitter>,
    ) -> Self {
        let max_concurrent_captures = max_concurrent_captures.max(MIN_MAX_CONCURRENT_CAPTURES);
        let blocking_task_timeout = normalize_blocking_task_timeout(blocking_task_timeout);
        Self {
            backend,
            cursor_provider,
            permission_gate,
            storage,
            capture_slots: Arc::new(tokio::sync::Semaphore::new(max_concurrent_captures)),
            blocking_task_timeout,
            feedback_emitter,
            tool_router: Self::build_tool_router(),
        }
    }

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
            backend::{CaptureBackend, MonitorInfo},
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
            ENV_MAX_CONCURRENT_CAPTURES, MAX_BLOCKING_TASK_TIMEOUT_MS,
            MIN_BLOCKING_TASK_TIMEOUT_MS, RuntimeConfig,
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

        fn capture_screen(&self, _monitor_id: Option<u32>) -> Result<RgbaImage, ServerError> {
            Ok(RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])))
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

        fn latest_artifact(&self) -> Result<StoredArtifact, ServerError> {
            Ok(StoredArtifact {
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
    }

    #[derive(Debug, Default)]
    struct CountingFeedbackEmitter {
        calls: AtomicUsize,
    }

    impl CaptureFeedbackEmitter for CountingFeedbackEmitter {
        fn emit(&self) {
            self.calls.fetch_add(1, Ordering::SeqCst);
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
        emitter.emit();
    }

    #[test]
    fn mcp_server_dummy_components_are_callable_for_all_capture_paths() {
        let backend = DummyBackend;
        assert_eq!(backend.list_monitors().expect("monitors").len(), 1);
        assert_eq!(backend.capture_screen(None).expect("screen").width(), 2);
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
        });

        assert_eq!(server.capture_slots.available_permits(), 7);
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
        }

        let server = ZeuxisScreenshotServer::with_components(
            Arc::new(DummyBackend),
            Arc::new(DummyCursor),
            Arc::new(DummyPermission),
            Arc::new(DummyStorage),
        );
        assert_eq!(server.capture_slots.available_permits(), 4);

        unsafe {
            std::env::remove_var(ENV_MAX_CONCURRENT_CAPTURES);
        }
    }

    #[tokio::test]
    async fn mcp_server_with_components_and_feedback_uses_custom_emitter_and_env_timeout() {
        let feedback = Arc::new(CountingFeedbackEmitter::default());
        let server = {
            let _guard = env_lock().lock().expect("lock env");
            unsafe {
                std::env::set_var(ENV_BLOCKING_TASK_TIMEOUT_MS, "1700");
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
            }
            server
        };

        assert_eq!(server.blocking_task_timeout, Duration::from_millis(1700));

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
        assert_eq!(feedback.calls.load(Ordering::SeqCst), 1);
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

        let default_server = ZeuxisScreenshotServer::default();
        assert!(default_server.capture_slots.available_permits() >= 1);
    }
}
