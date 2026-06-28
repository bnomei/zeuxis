//! Capture backend contract for monitor/window discovery and screenshot acquisition.
//!
//! Implementations expose logical desktop coordinates and convert platform
//! failures into stable `ServerError` values before MCP results are built.

use image::RgbaImage;
use serde::Serialize;

use crate::{
    capture::region::{GlobalRect, Point},
    mcp::errors::ServerError,
};

/// Monitor metadata surfaced by `list_monitors` and capture target payloads.
///
/// Coordinates use global logical desktop points. IDs are backend-provided and
/// are intended for the current process/session rather than durable storage.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MonitorInfo {
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
    pub is_builtin: bool,
}

/// Window metadata surfaced by `list_windows` and capture target payloads.
///
/// Coordinates use global logical desktop points. Window IDs are only stable for
/// the backend snapshot that produced them.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    pub app: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_focused: bool,
    pub is_minimized: bool,
}

/// Platform capture abstraction used by MCP tools and subprocess workers.
///
/// Backends should preserve the requested capture semantics and report
/// unsupported operations with stable `ServerError` codes rather than panicking.
pub trait CaptureBackend: Send + Sync {
    /// Lists monitors available to the current graphical session.
    fn list_monitors(&self) -> Result<Vec<MonitorInfo>, ServerError>;
    /// Lists windows when the platform backend can expose them.
    fn list_windows(&self) -> Result<Vec<WindowInfo>, ServerError> {
        Err(ServerError::capture_unsupported_on_platform(
            "listing windows is not supported by this capture backend",
        ))
    }
    /// Captures a full monitor, defaulting to the backend's primary monitor.
    fn capture_screen(&self, monitor_id: Option<u32>) -> Result<RgbaImage, ServerError>;
    /// Captures a window by a backend window ID from a current window listing.
    fn capture_window(&self, _window_id: u32) -> Result<RgbaImage, ServerError> {
        Err(ServerError::capture_unsupported_on_platform(
            "capturing a window by id is not supported by this capture backend",
        ))
    }
    /// Captures a monitor-local rectangle in logical desktop points.
    fn capture_monitor_region(
        &self,
        _monitor_id: u32,
        _x: u32,
        _y: u32,
        _width: u32,
        _height: u32,
    ) -> Result<RgbaImage, ServerError> {
        Err(ServerError::capture_unsupported_on_platform(
            "capturing a monitor region by monitor id is not supported by this capture backend",
        ))
    }
    /// Captures the focused, non-minimized window.
    fn capture_active_window(&self) -> Result<RgbaImage, ServerError>;
    /// Captures the topmost window containing a global cursor point.
    fn capture_window_at_cursor(&self, cursor: Point) -> Result<RgbaImage, ServerError>;
    /// Captures a square region centered on a global cursor point.
    fn capture_cursor_region(&self, cursor: Point, size: u32) -> Result<RgbaImage, ServerError>;
    /// Captures an exact global desktop rectangle in logical points.
    fn capture_rect(&self, rect: GlobalRect) -> Result<RgbaImage, ServerError>;
}
