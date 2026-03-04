#![allow(dead_code)]

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use image::{Rgba, RgbaImage};

use zeuxis::{
    capture::{
        backend::CaptureBackend,
        region::{GlobalRect, Point},
    },
    cursor::CursorProvider,
    mcp::{errors::ServerError, server::ZeuxisScreenshotServer},
    platform::PermissionGate,
    storage::{PngStorage, StoredArtifact},
};

pub struct TestHarness {
    pub server: ZeuxisScreenshotServer,
    pub backend: Arc<MockCaptureBackend>,
    pub cursor: Arc<MockCursorProvider>,
    pub permission: Arc<MockPermissionGate>,
    pub storage: Arc<MockStorage>,
}

pub fn create_test_harness() -> TestHarness {
    let backend = Arc::new(MockCaptureBackend::new());
    let cursor = Arc::new(MockCursorProvider::new(Point::new(50, 60)));
    let permission = Arc::new(MockPermissionGate::new(Ok(())));
    let storage = Arc::new(MockStorage::new());

    let server = ZeuxisScreenshotServer::with_components(
        backend.clone(),
        cursor.clone(),
        permission.clone(),
        storage.clone(),
    );

    TestHarness {
        server,
        backend,
        cursor,
        permission,
        storage,
    }
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

#[derive(Debug)]
pub struct MockCaptureBackend {
    pub screen_error: Mutex<Option<ServerError>>,
    pub active_error: Mutex<Option<ServerError>>,
    pub window_error: Mutex<Option<ServerError>>,
    pub cursor_region_error: Mutex<Option<ServerError>>,
    pub rect_error: Mutex<Option<ServerError>>,
    pub last_window_cursor: Mutex<Option<Point>>,
    pub last_cursor_region: Mutex<Option<(Point, u32)>>,
    pub last_rect: Mutex<Option<GlobalRect>>,
}

impl MockCaptureBackend {
    pub fn new() -> Self {
        Self {
            screen_error: Mutex::new(None),
            active_error: Mutex::new(None),
            window_error: Mutex::new(None),
            cursor_region_error: Mutex::new(None),
            rect_error: Mutex::new(None),
            last_window_cursor: Mutex::new(None),
            last_cursor_region: Mutex::new(None),
            last_rect: Mutex::new(None),
        }
    }

    fn image(&self) -> RgbaImage {
        RgbaImage::from_pixel(8, 6, Rgba([10, 20, 30, 255]))
    }
}

impl CaptureBackend for MockCaptureBackend {
    fn capture_screen(&self) -> Result<RgbaImage, ServerError> {
        if let Some(error) = self.screen_error.lock().expect("lock").clone() {
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
    pub calls: AtomicUsize,
    pub last_mode: Mutex<Option<String>>,
}

impl MockStorage {
    pub fn new() -> Self {
        Self {
            error: Mutex::new(None),
            calls: AtomicUsize::new(0),
            last_mode: Mutex::new(None),
        }
    }
}

impl PngStorage for MockStorage {
    fn write_png(
        &self,
        image: &RgbaImage,
        capture_mode: &str,
    ) -> Result<StoredArtifact, ServerError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_mode.lock().expect("lock") = Some(capture_mode.to_owned());

        if let Some(error) = self.error.lock().expect("lock").clone() {
            return Err(error);
        }

        Ok(StoredArtifact {
            path: std::path::PathBuf::from(format!("/tmp/{capture_mode}.png")),
            uri: format!("file:///tmp/{capture_mode}.png"),
            width: image.width(),
            height: image.height(),
        })
    }
}
