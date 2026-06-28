#![allow(dead_code)]

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::{thread, time::Duration};

use image::{Rgba, RgbaImage};

use zeuxis::{
    capture::{
        backend::{CaptureBackend, MonitorInfo, WindowInfo},
        region::{GlobalRect, Point},
    },
    cursor::CursorProvider,
    mcp::{
        errors::ServerError,
        server::{CaptureFeedbackEmitter, ZeuxisScreenshotServer},
    },
    platform::PermissionGate,
    storage::{CaptureOutputOptions, PngStorage, StoredArtifact},
};

pub struct TestHarness {
    pub server: ZeuxisScreenshotServer,
    pub backend: Arc<MockCaptureBackend>,
    pub cursor: Arc<MockCursorProvider>,
    pub permission: Arc<MockPermissionGate>,
    pub storage: Arc<MockStorage>,
    pub feedback: Arc<MockFeedbackEmitter>,
}

pub fn create_test_harness() -> TestHarness {
    create_test_harness_with_parallelism(2)
}

pub fn create_test_harness_with_parallelism(max_concurrent_captures: usize) -> TestHarness {
    create_test_harness_with_parallelism_and_timeout(
        max_concurrent_captures,
        Duration::from_millis(15_000),
    )
}

pub fn create_test_harness_with_parallelism_and_timeout(
    max_concurrent_captures: usize,
    blocking_task_timeout: Duration,
) -> TestHarness {
    init_test_tracing();
    let backend = Arc::new(MockCaptureBackend::new());
    let cursor = Arc::new(MockCursorProvider::new(Point::new(50, 60)));
    let permission = Arc::new(MockPermissionGate::new(Ok(())));
    let storage = Arc::new(MockStorage::new());
    let feedback = Arc::new(MockFeedbackEmitter::new());

    let server = ZeuxisScreenshotServer::with_components_and_limits(
        backend.clone(),
        cursor.clone(),
        permission.clone(),
        storage.clone(),
        max_concurrent_captures,
        blocking_task_timeout,
        feedback.clone(),
    );

    TestHarness {
        server,
        backend,
        cursor,
        permission,
        storage,
        feedback,
    }
}

fn init_test_tracing() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .with_test_writer()
            .try_init();
    });
}

pub fn extract_error_code(result: &rmcp::model::CallToolResult) -> String {
    result
        .structured_content
        .as_ref()
        .and_then(|value| value.get("error_code"))
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_owned()
}

pub fn extract_capture_mode(result: &rmcp::model::CallToolResult) -> String {
    result
        .structured_content
        .as_ref()
        .and_then(|value| value.get("capture_mode"))
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_owned()
}

pub fn extract_monitor_count(result: &rmcp::model::CallToolResult) -> usize {
    result
        .structured_content
        .as_ref()
        .and_then(|value| value.get("monitor_count"))
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
        .unwrap_or_default()
}

type MonitorRegionCapture = (u32, u32, u32, u32, u32);

#[derive(Debug)]
pub struct MockCaptureBackend {
    pub monitors: Mutex<Vec<MonitorInfo>>,
    pub windows: Mutex<Vec<WindowInfo>>,
    pub monitors_error: Mutex<Option<ServerError>>,
    pub monitors_panic: Mutex<bool>,
    pub windows_error: Mutex<Option<ServerError>>,
    pub screen_error: Mutex<Option<ServerError>>,
    pub screen_panic: Mutex<bool>,
    pub active_error: Mutex<Option<ServerError>>,
    pub window_error: Mutex<Option<ServerError>>,
    pub cursor_region_error: Mutex<Option<ServerError>>,
    pub monitor_region_error: Mutex<Option<ServerError>>,
    pub rect_error: Mutex<Option<ServerError>>,
    pub last_screen_monitor_id: Mutex<Option<Option<u32>>>,
    pub last_window_id: Mutex<Option<u32>>,
    pub last_window_cursor: Mutex<Option<Point>>,
    pub last_cursor_region: Mutex<Option<(Point, u32)>>,
    pub last_monitor_region: Mutex<Option<MonitorRegionCapture>>,
    pub last_rect: Mutex<Option<GlobalRect>>,
    pub screen_capture_delay: Mutex<Option<Duration>>,
    pub active_screen_captures: AtomicUsize,
    pub max_active_screen_captures: AtomicUsize,
}

impl MockCaptureBackend {
    pub fn new() -> Self {
        Self {
            monitors: Mutex::new(vec![
                MonitorInfo {
                    id: 100,
                    name: "Primary".to_owned(),
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                    is_primary: true,
                    is_builtin: true,
                },
                MonitorInfo {
                    id: 200,
                    name: "Secondary".to_owned(),
                    x: 1920,
                    y: 0,
                    width: 1280,
                    height: 1024,
                    is_primary: false,
                    is_builtin: false,
                },
            ]),
            windows: Mutex::new(vec![
                WindowInfo {
                    id: 300,
                    title: "Editor".to_owned(),
                    app: "Code".to_owned(),
                    x: 10,
                    y: 20,
                    width: 800,
                    height: 600,
                    is_focused: true,
                    is_minimized: false,
                },
                WindowInfo {
                    id: 400,
                    title: "Browser".to_owned(),
                    app: "Safari".to_owned(),
                    x: 900,
                    y: 50,
                    width: 900,
                    height: 700,
                    is_focused: false,
                    is_minimized: false,
                },
            ]),
            monitors_error: Mutex::new(None),
            monitors_panic: Mutex::new(false),
            windows_error: Mutex::new(None),
            screen_error: Mutex::new(None),
            screen_panic: Mutex::new(false),
            active_error: Mutex::new(None),
            window_error: Mutex::new(None),
            cursor_region_error: Mutex::new(None),
            monitor_region_error: Mutex::new(None),
            rect_error: Mutex::new(None),
            last_screen_monitor_id: Mutex::new(None),
            last_window_id: Mutex::new(None),
            last_window_cursor: Mutex::new(None),
            last_cursor_region: Mutex::new(None),
            last_monitor_region: Mutex::new(None),
            last_rect: Mutex::new(None),
            screen_capture_delay: Mutex::new(None),
            active_screen_captures: AtomicUsize::new(0),
            max_active_screen_captures: AtomicUsize::new(0),
        }
    }

    fn image(&self) -> RgbaImage {
        RgbaImage::from_pixel(8, 6, Rgba([10, 20, 30, 255]))
    }
}

impl CaptureBackend for MockCaptureBackend {
    fn list_monitors(&self) -> Result<Vec<MonitorInfo>, ServerError> {
        if *self.monitors_panic.lock().expect("lock") {
            panic!("mock monitor panic");
        }
        if let Some(error) = self.monitors_error.lock().expect("lock").clone() {
            return Err(error);
        }
        Ok(self.monitors.lock().expect("lock").clone())
    }

    fn list_windows(&self) -> Result<Vec<WindowInfo>, ServerError> {
        if let Some(error) = self.windows_error.lock().expect("lock").clone() {
            return Err(error);
        }
        Ok(self.windows.lock().expect("lock").clone())
    }

    fn capture_screen(&self, monitor_id: Option<u32>) -> Result<RgbaImage, ServerError> {
        *self.last_screen_monitor_id.lock().expect("lock") = Some(monitor_id);
        if *self.screen_panic.lock().expect("lock") {
            panic!("mock capture panic");
        }
        if let Some(error) = self.screen_error.lock().expect("lock").clone() {
            return Err(error);
        }
        if let Some(delay) = *self.screen_capture_delay.lock().expect("lock") {
            let active = self.active_screen_captures.fetch_add(1, Ordering::SeqCst) + 1;
            update_max_atomic(&self.max_active_screen_captures, active);
            thread::sleep(delay);
            self.active_screen_captures.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(self.image())
    }

    fn capture_window(&self, window_id: u32) -> Result<RgbaImage, ServerError> {
        *self.last_window_id.lock().expect("lock") = Some(window_id);
        if let Some(error) = self.window_error.lock().expect("lock").clone() {
            return Err(error);
        }

        let exists = self
            .windows
            .lock()
            .expect("lock")
            .iter()
            .any(|window| window.id == window_id);
        if !exists {
            return Err(ServerError::window_not_found(format!(
                "window with id {window_id} not found"
            )));
        }

        Ok(self.image())
    }

    fn capture_monitor_region(
        &self,
        monitor_id: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage, ServerError> {
        *self.last_monitor_region.lock().expect("lock") = Some((monitor_id, x, y, width, height));
        if let Some(error) = self.monitor_region_error.lock().expect("lock").clone() {
            return Err(error);
        }
        Ok(self.image())
    }

    fn capture_active_window(&self) -> Result<RgbaImage, ServerError> {
        if let Some(error) = self.active_error.lock().expect("lock").clone() {
            return Err(error);
        }
        Ok(self.image())
    }

    fn capture_window_at_cursor(&self, cursor: Point) -> Result<RgbaImage, ServerError> {
        *self.last_window_cursor.lock().expect("lock") = Some(cursor);
        if let Some(error) = self.window_error.lock().expect("lock").clone() {
            return Err(error);
        }
        Ok(self.image())
    }

    fn capture_cursor_region(&self, cursor: Point, size: u32) -> Result<RgbaImage, ServerError> {
        *self.last_cursor_region.lock().expect("lock") = Some((cursor, size));
        if let Some(error) = self.cursor_region_error.lock().expect("lock").clone() {
            return Err(error);
        }
        Ok(self.image())
    }

    fn capture_rect(&self, rect: GlobalRect) -> Result<RgbaImage, ServerError> {
        *self.last_rect.lock().expect("lock") = Some(rect);
        if let Some(error) = self.rect_error.lock().expect("lock").clone() {
            return Err(error);
        }
        Ok(self.image())
    }
}

#[derive(Debug)]
pub struct MockCursorProvider {
    pub result: Mutex<Result<Point, ServerError>>,
}

impl MockCursorProvider {
    pub fn new(point: Point) -> Self {
        Self {
            result: Mutex::new(Ok(point)),
        }
    }
}

impl CursorProvider for MockCursorProvider {
    fn cursor_position(&self) -> Result<Point, ServerError> {
        self.result.lock().expect("lock").clone()
    }
}

#[derive(Debug)]
pub struct MockPermissionGate {
    pub result: Mutex<Result<(), ServerError>>,
    pub calls: AtomicUsize,
}

impl MockPermissionGate {
    pub fn new(result: Result<(), ServerError>) -> Self {
        Self {
            result: Mutex::new(result),
            calls: AtomicUsize::new(0),
        }
    }
}

impl PermissionGate for MockPermissionGate {
    fn ensure_capture_allowed(&self) -> Result<(), ServerError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.result.lock().expect("lock").clone()
    }
}

#[derive(Debug)]
pub struct MockStorage {
    pub error: Mutex<Option<ServerError>>,
    pub panic_on_write: Mutex<bool>,
    pub latest_error: Mutex<Option<ServerError>>,
    pub panic_on_latest: Mutex<bool>,
    pub calls: AtomicUsize,
    pub latest_calls: AtomicUsize,
    pub list_calls: AtomicUsize,
    pub last_mode: Mutex<Option<String>>,
    pub last_output: Mutex<Option<CaptureOutputOptions>>,
    pub latest_artifact: Mutex<Option<StoredArtifact>>,
    pub session_artifacts: Mutex<Vec<StoredArtifact>>,
}

impl MockStorage {
    pub fn new() -> Self {
        Self {
            error: Mutex::new(None),
            panic_on_write: Mutex::new(false),
            latest_error: Mutex::new(None),
            panic_on_latest: Mutex::new(false),
            calls: AtomicUsize::new(0),
            latest_calls: AtomicUsize::new(0),
            list_calls: AtomicUsize::new(0),
            last_mode: Mutex::new(None),
            last_output: Mutex::new(None),
            latest_artifact: Mutex::new(None),
            session_artifacts: Mutex::new(Vec::new()),
        }
    }
}

impl PngStorage for MockStorage {
    fn write_image(
        &self,
        image: RgbaImage,
        capture_mode: &str,
        output: CaptureOutputOptions,
    ) -> Result<StoredArtifact, ServerError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_mode.lock().expect("lock") = Some(capture_mode.to_owned());
        *self.last_output.lock().expect("lock") = Some(output);
        if *self.panic_on_write.lock().expect("lock") {
            panic!("mock storage write panic");
        }

        if let Some(error) = self.error.lock().expect("lock").clone() {
            return Err(error);
        }

        let artifact = StoredArtifact {
            artifact_id: format!("{capture_mode}.png"),
            capture_mode: capture_mode.to_owned(),
            path: std::path::PathBuf::from(format!("/tmp/{capture_mode}.png")),
            uri: format!("file:///tmp/{capture_mode}.png"),
            output_format: output.format.as_str().to_owned(),
            mime_type: output.format.mime_type().to_owned(),
            artifact_sha256: "00".repeat(32),
            artifact_hmac_sha256: None,
            width: image.width(),
            height: image.height(),
            captured_at_utc: "2026-01-01T00:00:00Z".to_owned(),
        };
        *self.latest_artifact.lock().expect("lock") = Some(artifact.clone());
        let mut session_artifacts = self.session_artifacts.lock().expect("lock");
        session_artifacts.retain(|entry| entry.artifact_id != artifact.artifact_id);
        session_artifacts.push(artifact.clone());
        Ok(artifact)
    }

    fn adopt_artifact(
        &self,
        path: std::path::PathBuf,
        capture_mode: &str,
        output: CaptureOutputOptions,
    ) -> Result<StoredArtifact, ServerError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_mode.lock().expect("lock") = Some(capture_mode.to_owned());
        *self.last_output.lock().expect("lock") = Some(output);
        if let Some(error) = self.error.lock().expect("lock").clone() {
            return Err(error);
        }

        let (width, height) = image::image_dimensions(&path).unwrap_or((8, 6));
        let artifact = StoredArtifact {
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
        };
        *self.latest_artifact.lock().expect("lock") = Some(artifact.clone());
        let mut session_artifacts = self.session_artifacts.lock().expect("lock");
        session_artifacts.retain(|entry| entry.artifact_id != artifact.artifact_id);
        session_artifacts.push(artifact.clone());
        Ok(artifact)
    }

    fn latest_artifact(&self) -> Result<StoredArtifact, ServerError> {
        self.latest_calls.fetch_add(1, Ordering::SeqCst);
        if *self.panic_on_latest.lock().expect("lock") {
            panic!("mock storage latest panic");
        }
        if let Some(error) = self.latest_error.lock().expect("lock").clone() {
            return Err(error);
        }

        self.latest_artifact
            .lock()
            .expect("lock")
            .clone()
            .ok_or_else(|| ServerError::no_capture_yet("no screenshot has been captured yet"))
    }

    fn list_session_artifacts(&self) -> Result<Vec<StoredArtifact>, ServerError> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        let mut artifacts = self.session_artifacts.lock().expect("lock").clone();
        artifacts.sort_by(|a, b| b.captured_at_utc.cmp(&a.captured_at_utc));
        Ok(artifacts)
    }

    fn clear_session_artifacts(&self) -> Result<usize, ServerError> {
        let deleted = self.session_artifacts.lock().expect("lock").len();
        self.session_artifacts.lock().expect("lock").clear();
        self.latest_artifact.lock().expect("lock").take();
        Ok(deleted)
    }

    fn artifact_dir(&self) -> std::path::PathBuf {
        std::env::temp_dir()
    }
}

#[derive(Debug)]
pub struct MockFeedbackEmitter {
    pub calls: AtomicUsize,
    pub capture_calls: AtomicUsize,
}

impl MockFeedbackEmitter {
    pub fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            capture_calls: AtomicUsize::new(0),
        }
    }
}

impl CaptureFeedbackEmitter for MockFeedbackEmitter {
    fn emit_capture(&self) {
        self.capture_calls.fetch_add(1, Ordering::SeqCst);
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

fn update_max_atomic(maximum: &AtomicUsize, observed: usize) {
    let mut current = maximum.load(Ordering::SeqCst);
    while observed > current {
        match maximum.compare_exchange(current, observed, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => break,
            Err(latest) => current = latest,
        }
    }
}
